use crate::btc_rpc::RPCWrapper;
use crate::common::{
    ConfigKey, OrcaNetConfig, OrcaNetError, OrcaNetRequest, OrcaNetResponse, PaymentNotification,
    PaymentRequest, PrePaymentResponse, ProxyClientConfig,
};
use crate::db::{
    PaymentCategory, PaymentInfo, PaymentStatus, PaymentsTable, ProxyClientsTable,
    ProxySessionInfo, ProxySessionsTable,
};
use crate::network_client::NetworkClient;
use crate::utils::Utils;
use bitcoin::Txid;
use bytes::Bytes;
use futures::channel::mpsc::Receiver;
use futures::StreamExt;
use headers::Authorization;
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Incoming};
use hyper::header::{HeaderValue, AUTHORIZATION};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Error, Request, Response, StatusCode};
use hyper_http_proxy::{Intercept, Proxy, ProxyConnector};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioIo;
use libp2p_swarm::derive_prelude::PeerId;
use serde_json::json;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{interval, Duration};
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait RequestHandler: Send + Sync {
    async fn handle_request(
        &self,
        request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error>;

    async fn clean_up(&self) {}
}

pub struct ProxyProvider {
    http_client: Client<HttpConnector, Incoming>,
}

impl ProxyProvider {
    pub fn new() -> Self {
        Self {
            http_client: Client::builder(hyper_util::rt::TokioExecutor::new())
                .build(HttpConnector::new()),
        }
    }
}

#[async_trait]
impl RequestHandler for ProxyProvider {
    async fn handle_request(
        &self,
        request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        tracing::info!("Request headers: {:?}", request.headers());
        // Get auth token
        let auth_token = match extract_bearer_token(&request) {
            Ok(token) => token,
            Err(e) => {
                return Ok(bad_request_with_err(OrcaNetError::AuthorizationFailed(e)));
            }
        };

        // Validate auth token (must be present in table)
        let mut proxy_clients_table = ProxyClientsTable::new(None);
        let client_info = match proxy_clients_table.get_client_by_auth_token(auth_token.as_str()) {
            Ok(client_info) => client_info,
            Err(e) => {
                tracing::error!("Error {:?}", e);
                return Ok(bad_request_with_err(OrcaNetError::AuthorizationFailed(
                    "Auth token verification failed".to_string(),
                )));
            }
        };

        // Check proxy session status
        match client_info.status {
            0 => {
                // Inactive proxy provider session. Ask client to create new session
                // Either party could have terminated the contract
                return Ok(bad_request_with_err(OrcaNetError::AuthorizationFailed(
                    "Received auth token for inactive session. Start a new session.".to_string(),
                )));
            }
            -1 => {
                // Server decided to terminate the contract due to client dishonesty
                return Ok(bad_request_with_err(
                    OrcaNetError::SessionTerminatedByProvider,
                ));
            }
            _ => {}
        }

        tracing::info!("Request body size {:?}", request.size_hint().exact());

        // Send the request
        let path = request.uri().path();
        tracing::info!("Request path: {path}");

        let response = self.http_client.request(request).await.unwrap();

        let (parts, body) = response.into_parts();
        let bytes = body.collect().await?.to_bytes();

        // Update client info in DB
        let size_kb = (bytes.len() as f64) / 1000f64;
        proxy_clients_table
            .update_data_transfer_info(client_info.client_id.as_str(), size_kb)
            .expect("Data transfer info to be updated in DB"); // TODO: May be failure is too strict ?

        tracing::info!("Response body size: {:?}", bytes.len());

        Ok(Response::from_parts(parts, Full::new(bytes)))
    }
}

pub struct ProxyClient {
    http_client: Client<ProxyConnector<HttpConnector>, Incoming>,
    session_info: ProxySessionInfo, // We don't record any data here, but only use configuration values like client_id, auth_token etc
    payment_loop_cancellation_token: CancellationToken,
}

impl ProxyClient {
    pub fn new(
        session_info: ProxySessionInfo,
        payment_loop_cancellation_token: CancellationToken,
    ) -> Self {
        // Configure the proxy
        let proxy_uri = session_info
            .proxy_address
            .clone()
            .parse()
            .expect("Proxy address to be valid proxy URI");
        let mut proxy = Proxy::new(Intercept::All, proxy_uri);
        let authorization = Authorization::bearer(session_info.auth_token.as_str())
            .expect("Authorization token to be valid");
        proxy.set_authorization(authorization);

        // Create the http client
        let proxy_connector = ProxyConnector::from_proxy(HttpConnector::new(), proxy)
            .expect("Proxy connector creation to be successful");
        let http_client =
            Client::builder(hyper_util::rt::TokioExecutor::new()).build(proxy_connector);

        Self {
            http_client,
            session_info,
            payment_loop_cancellation_token,
        }
    }
}

#[async_trait]
impl RequestHandler for ProxyClient {
    async fn handle_request(
        &self,
        mut request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, Error> {
        tracing::info!("Request headers: {:?}", request.headers());

        let path = request.uri().path();
        tracing::info!("Request path: {path}");

        // Add Bearer token
        // TODO: For some reason set_authorization in proxy is not setting the header. Check later.
        let token = format!("Bearer {}", self.session_info.auth_token.as_str());
        let token_hdr_value = HeaderValue::from_str(token.as_str())
            .expect("token value to be valid header value when parsed from string");
        request.headers_mut().insert(AUTHORIZATION, token_hdr_value);

        // Send the request through proxy
        let response = self
            .http_client
            .request(request)
            .await
            .expect("Response to be valid");

        let (parts, body) = response.into_parts();
        let bytes = body.collect().await?.to_bytes();

        // Update usage info in DB
        let size_kb = (bytes.len() as f64) / 1000f64;
        let mut proxy_sessions_table = ProxySessionsTable::new(None);
        proxy_sessions_table
            .update_data_transfer_info(self.session_info.session_id.as_str(), size_kb)
            .expect("Data transfer info to be updated in DB");

        tracing::info!("Response body size: {:?}", bytes.len());

        Ok(Response::from_parts(parts, Full::new(bytes)))
    }

    async fn clean_up(&self) {
        self.payment_loop_cancellation_token.cancelled().await;
    }
}

pub struct ProxyPaymentLoop {
    pub session_id: String,
    pub network_client: NetworkClient,
    pub proxy_sessions_table: ProxySessionsTable,
    pub cancellation_token: CancellationToken,
}

impl ProxyPaymentLoop {
    pub async fn start_loop(mut self) {
        let mut interval = interval(Duration::from_secs(
            OrcaNetConfig::PROXY_PAYMENT_INTERVAL_SECS,
        ));

        loop {
            tokio::select! {
                _ = self.cancellation_token.cancelled() => {
                    // Settle up
                    // Cancel payment loop
                    return;
                }

                interval_event = interval.tick() => {
                    tracing::info!("Attempting payment for HTTP proxy");

                    match self.process_payment().await {
                        Ok(payment_reference) => {
                            tracing::info!(
                                "Payment attempt succeeded. Reference: {}",
                                payment_reference
                            );
                        }
                        Err(e) => {
                            tracing::error!("Payment attempt failed: {:?}", e);
                        }
                    }
                }
            }
        }
    }

    async fn process_payment(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        // Get session info
        let mut proxy_sessions_table = ProxySessionsTable::new(None);
        let session_info = proxy_sessions_table.get_session_info(self.session_id.as_str())?;
        let provider_peer = session_info
            .provider_peer_id
            .parse()
            .expect("Provider peer id to be valid");

        // Send pre-payment request to make sure the server agrees and to get amount to be paid
        let payment_request = self.pre_payment_step(provider_peer, &session_info).await?;

        // Create a transaction for the requested amount
        let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());
        let comment = format!(
            "Payment for proxy. Reference: {:?}",
            payment_request.payment_reference
        );
        let tx_id = rpc_wrapper.send_to_address(
            payment_request.recipient_address.as_str(),
            payment_request.amount_to_send,
            Some(comment.as_str()),
        )?;

        // Persist in database
        let mut payments_table = PaymentsTable::new(None);
        let payment_info = PaymentInfo {
            payment_id: Utils::new_uuid(), // To make sure server doesn't mess up our payment by giving old/existing reference
            tx_id: Some(tx_id.to_string()),
            to_address: payment_request.recipient_address.clone(),
            amount_btc: Some(payment_request.amount_to_send),
            category: PaymentCategory::Send.to_string(),
            status: PaymentStatus::TransactionPending.to_string(),
            payment_reference: Some(payment_request.payment_reference.clone()),
            from_peer: None,
            to_peer: Some(session_info.provider_peer_id),
            ..Default::default()
        };
        payments_table.insert_payment_info(&payment_info)?;

        // Send post payment notification to the server
        let _ = self.network_client.send_request(
            provider_peer,
            OrcaNetRequest::HTTPProxyPostPaymentNotification {
                client_id: session_info.client_id.clone(),
                auth_token: session_info.auth_token.clone(),
                payment_notification: PaymentNotification {
                    sender_address: OrcaNetConfig::get_btc_address(),
                    receiver_address: payment_request.recipient_address,
                    amount_transferred: payment_request.amount_to_send,
                    tx_id: tx_id.to_string(),
                    payment_reference: payment_request.payment_reference.clone(),
                },
            },
        );

        Ok(payment_request.payment_reference)
    }

    async fn pre_payment_step(
        &mut self,
        peer_id: PeerId,
        session_info: &ProxySessionInfo,
    ) -> Result<PaymentRequest, Box<dyn std::error::Error>> {
        let resp = self
            .network_client
            .send_request(
                peer_id,
                OrcaNetRequest::HTTPProxyPrePaymentRequest {
                    client_id: session_info.client_id.clone(),
                    auth_token: session_info.auth_token.clone(),
                    fee_owed: session_info.get_fee_owed(),
                    data_transferred_kb: session_info.data_transferred_kb,
                },
            )
            .await
            .map_err(|e| e as Box<dyn std::error::Error>)?;

        match resp {
            OrcaNetResponse::HTTPProxyPrePaymentResponse {
                fee_owed,
                data_transferred_kb,
                pre_payment_response,
            } => {
                tracing::info!(
                    "Received pre payment response {:?}. Data transferred: {}, fee_owed: {}",
                    pre_payment_response,
                    data_transferred_kb,
                    fee_owed
                );

                // Check if server's data differs too much
                // if data_transferred_kb != session_info.data_transferred_kb {
                //
                // }

                // If it does, send all remaining amount and terminate the connection
                // TODO: Note down the incident so we can factor this into reputation calculation

                // If not, proceed to handle the server's response
                match pre_payment_response {
                    PrePaymentResponse::Accepted(payment_req) => Ok(payment_req),
                    PrePaymentResponse::RejectedDataTransferDiffers => {
                        // Adjust to what the server says
                        // At this point, we've decided that the server's values are not too different, so it's fine
                        todo!()
                    }
                    PrePaymentResponse::RejectedFeeOwedDiffers => {
                        // Adjust to what the server says
                        todo!()
                    }
                    PrePaymentResponse::ServerTerminatingConnection(payment_req) => {
                        todo!()
                    }
                }
            }
            OrcaNetResponse::Error(e) => {
                Err(format!("Got error for pre payment request {:?}", e).into())
            }
            // Err(e) => Err(format!("Got error for pre payment request {:?}", e).into()),
            _ => Err("Got invalid response for pre payment request".into()),
        }
    }
}

fn bad_request_with_err(err: OrcaNetError) -> Response<Full<Bytes>> {
    let json_resp = json!({
        "error": err,
    });
    let body = Bytes::from(json_resp.to_string());

    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Full::new(body))
        .expect("Couldn't build body")
}

fn extract_bearer_token(req: &Request<Incoming>) -> Result<String, String> {
    // Get the Authorization header
    let auth_header = req
        .headers()
        .get(AUTHORIZATION)
        .ok_or("Missing authorization header".to_string())?;

    let auth_str = auth_header
        .to_str()
        .map_err(|_| "Invalid authorization header".to_string())?;

    // Extract the token
    if !auth_str.starts_with("Bearer ") {
        return Err("Invalid authorization format - must be Bearer token".to_string());
    }

    Ok(auth_str[7..].to_string())
}
