use crate::btc_rpc::RPCWrapper;
use crate::common::{
    ConfigKey, OrcaNetConfig, OrcaNetError, OrcaNetRequest, OrcaNetResponse, PaymentNotification,
    PaymentRequest, PrePaymentResponse, ProxyClientConfig,
};
use crate::db::{
    PaymentCategory, PaymentInfo, PaymentStatus, PaymentsTable, ProxyClientsTable,
    ProxySessionInfo, ProxySessionStatus, ProxySessionsTable,
};
use crate::network_client::NetworkClient;
use crate::utils::Utils;
use bitcoin::Txid;
use bytes::Bytes;
use clap::builder::styling::Reset;
use futures::channel::mpsc::Receiver;
use futures::StreamExt;
use headers::Authorization;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::{Body, Incoming};
use hyper::header::{HeaderValue, PROXY_AUTHORIZATION};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Error, Method, Request, Response, StatusCode};
use hyper_http_proxy::{Intercept, Proxy, ProxyConnector};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioIo;
use libp2p_swarm::derive_prelude::PeerId;
use rocket::form::{FromForm, Options};
use serde_json::json;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
    http_client: Client<HttpsConnector<HttpConnector>, Incoming>,
}

impl ProxyProvider {
    pub fn new() -> Self {
        Self {
            http_client: Client::builder(hyper_util::rt::TokioExecutor::new())
                .build(HttpsConnector::new()),
        }
    }

    fn validate_auth_token(
        &self,
        request: &Request<Incoming>,
    ) -> Result<(), Response<Full<Bytes>>> {
        // Get auth token
        let auth_token = match extract_bearer_token(&request) {
            Ok(token) => token,
            Err(e) => {
                return Err(bad_request_with_err(OrcaNetError::AuthorizationFailed(e)));
            }
        };

        // Validate auth token (must be present in table)
        let mut proxy_clients_table = ProxyClientsTable::new(None);
        let client_info = match proxy_clients_table.get_client_by_auth_token(auth_token.as_str()) {
            Ok(client_info) => client_info,
            Err(e) => {
                tracing::error!("Error {:?}", e);
                return Err(bad_request_with_err(OrcaNetError::AuthorizationFailed(
                    "Auth token verification failed".to_string(),
                )));
            }
        };

        let proxy_session_status = ProxySessionStatus::try_from(client_info.status);

        // Check proxy session status
        match proxy_session_status {
            Ok(ProxySessionStatus::TerminatedByClient) => {
                // Inactive proxy provider session. Ask client to create new session
                // Either party could have terminated the contract
                Err(bad_request_with_err(OrcaNetError::AuthorizationFailed(
                    "Received auth token for inactive session. Start a new session.".to_string(),
                )))
            }
            Ok(ProxySessionStatus::TerminatedByServer) => {
                // Server decided to terminate the contract due to client dishonesty
                Err(bad_request_with_err(
                    OrcaNetError::SessionTerminatedByProvider,
                ))
            }
            _ => Ok(()),
        }
    }

    async fn handle_standard_http_request(
        &self,
        request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        let response = self.http_client.request(request).await.unwrap();

        let (parts, body) = response.into_parts();
        let bytes = body.collect().await?.to_bytes();

        // Update client info in DB
        // let size_kb = (bytes.len() as f64) / 1000f64;

        // proxy_clients_table
        //     .update_data_transfer_info(client_info.client_id.as_str(), size_kb)
        //     .expect("Data transfer info to be updated in DB"); // TODO: May be failure is too strict ?

        tracing::info!("Response body size: {:?}", bytes.len());

        Ok(Response::from_parts(parts, Full::new(bytes)))
    }

    async fn handle_http_connect(
        &self,
        req: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        tracing::info!("Got HTTP CONNECT request");
        let authority = req.uri().authority().unwrap().clone();

        match TcpStream::connect(authority.as_str()).await {
            Ok(target_stream) => {
                tracing::info!("Connected to authority");

                tokio::task::spawn(async move {
                    tracing::info!("Upgrading request {:?}", req);

                    match hyper::upgrade::on(req).await {
                        Ok(upgraded) => {
                            tracing::info!("Upgraded connection");
                            tunnel(upgraded, target_stream).await
                        }
                        Err(e) => tracing::error!("Hyper upgrade error: {:?}", e),
                    }
                });

                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(Full::default())
                    .expect("Response creation failed"))
            }
            Err(_) => Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::default())
                .expect("Response creation failed")),
        }
    }
}

async fn tunnel(client_stream: hyper::upgrade::Upgraded, mut target_stream: TcpStream) {
    let mut client_stream = TokioIo::new(client_stream);

    match tokio::io::copy_bidirectional(&mut client_stream, &mut target_stream).await {
        Ok((from_client, from_target)) => {
            tracing::info!(
                "Client wrote {} bytes and target wrote {} bytes",
                from_client,
                from_target
            );
        }
        Err(e) => {
            tracing::error!("Error in tunnel: {}", e);
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
        tracing::info!("Request body size {:?}", request.size_hint().exact());

        // Validate auth token and error out if it fails
        // if let Err(error_response) = self.validate_auth_token(&request) {
        //     return Ok(error_response);
        // }

        // Send the request
        let path = request.uri().path();
        tracing::info!("Request path: {path}");

        if request.method() == Method::CONNECT {
            let resp = self.handle_http_connect(request).await;
            tracing::info!("Connect response: {:?}", resp);
            resp
        } else {
            self.handle_standard_http_request(request).await
        }
    }
}

pub struct ProxyClient {
    http_client: Client<ProxyConnector<HttpConnector>, Incoming>,
    http_connect_client: Client<ProxyConnector<HttpConnector>, Full<Bytes>>,
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
            .expect("Proxy connector creation failed");
        let http_client =
            Client::builder(hyper_util::rt::TokioExecutor::new()).build(proxy_connector.clone());
        let http_connect_client =
            Client::builder(hyper_util::rt::TokioExecutor::new()).build(proxy_connector);

        Self {
            http_client,
            http_connect_client,
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
        if request.method() == Method::CONNECT {
            self.handle_http_connect(request).await
        } else {
            self.handle_standard_http_request(request).await
        }
    }

    async fn clean_up(&self) {
        tracing::info!("Sending loop cancellation");
        self.payment_loop_cancellation_token.cancelled().await;
    }
}

impl ProxyClient {
    async fn handle_standard_http_request(
        &self,
        mut request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        tracing::info!("Request headers: {:?}", request.headers());

        let path = request.uri().path();
        tracing::info!("Request path: {path}");

        // // Add Bearer token
        // // TODO: For some reason set_authorization in proxy is not setting the header. Check later.
        let token = format!("Bearer {}", self.session_info.auth_token.as_str());
        let token_hdr_value = HeaderValue::from_str(token.as_str())
            .expect("token value to be valid header value when parsed from string");
        request
            .headers_mut()
            .insert(PROXY_AUTHORIZATION, token_hdr_value);

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

    async fn handle_http_connect(
        &self,
        mut request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        println!("Got HTTP connect request");
        // Forward CONNECT to remote proxy with auth
        let mut connect_req = Request::builder()
            .method(Method::CONNECT)
            .uri(request.uri().clone())
            .header(hyper::header::CONNECTION, "keep-alive")
            .header("proxy-connection", "keep-alive")
            .body(Full::default())
            .expect("Connect request creation needs to succeed to proceed");

        let token = format!("Bearer {}", self.session_info.auth_token.as_str());
        connect_req.headers_mut().insert(
            PROXY_AUTHORIZATION,
            HeaderValue::from_str(token.as_str()).unwrap(),
        );

        let session_id = self.session_info.session_id.clone();

        match self.http_connect_client.request(connect_req).await {
            Ok(remote_response) => {
                println!("Remote response: {:?}", remote_response);

                match remote_response.status() {
                    StatusCode::OK => {
                        tokio::task::spawn(async move {
                            tracing::info!("Upgrading request {:?} {:?}", request, remote_response);
                            if let (Ok(client_stream), Ok(remote_stream)) = (
                                hyper::upgrade::on(request).await,
                                hyper::upgrade::on(remote_response).await,
                            ) {
                                println!("Upgrade complete");
                                tunnel2(session_id, client_stream, remote_stream).await;
                            }
                        });

                        Ok(Response::builder()
                            .status(StatusCode::OK)
                            .body(Full::default())
                            .expect("Response creation failed"))
                    }
                    _ => Ok(Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .body(Full::default())
                        .expect("Response creation failed")),
                }
            }
            Err(e) => Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::default())
                .unwrap()),
        }
    }
}

async fn tunnel2(
    session_id: String,
    client_stream: hyper::upgrade::Upgraded,
    remote_stream: hyper::upgrade::Upgraded,
) {
    println!("In tunnel 2 response");
    let mut client_stream = TokioIo::new(client_stream);
    let mut remote_stream = TokioIo::new(remote_stream);

    match tokio::io::copy_bidirectional(&mut client_stream, &mut remote_stream).await {
        Ok((from_client, from_remote)) => {
            // let total_kb = (from_client + from_remote) as f64 / 1000f64;
            // let mut proxy_sessions_table = ProxySessionsTable::new(None);
            // proxy_sessions_table
            //     .update_data_transfer_info(session_id, total_kb)
            //     .expect("Data transfer info to be updated in DB");
            tracing::info!(
                "Client wrote {} bytes and target wrote {} bytes",
                from_client,
                from_remote
            );
        }
        Err(e) => {
            tracing::error!("Error in tunnel: {}", e);
        }
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
                    tracing::info!("Received loop cancellation");
                    // Settle up
                    // Cancel payment loop
                    return;
                }

                interval_event = interval.tick() => {
                    tracing::info!("Attempting payment for HTTP proxy");

                    match self.process_payment().await {
                        Ok(payment_reference) => {
                            if payment_reference.is_some() {
                                 tracing::info!(
                                "Payment attempt succeeded. Reference: {:?}",
                                payment_reference
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!("Payment attempt failed: {:?}", e);
                        }
                    }
                }
            }
        }
    }

    async fn process_payment(&mut self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        // Get session info
        let mut proxy_sessions_table = ProxySessionsTable::new(None);
        let session_info = proxy_sessions_table.get_session_info(self.session_id.as_str())?;

        if session_info.get_fee_owed() == 0f64 {
            tracing::info!("No pending amount. Payment not required.");
            return Ok(None);
        }

        tracing::info!(
            "Processing payment for {} Fee owed {}",
            session_info.session_id,
            session_info.get_fee_owed()
        );

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
            None,
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

        tracing::info!("Inserted payment record");

        let mut sessions_table = ProxySessionsTable::new(None);
        sessions_table.update_total_fee_sent_unconfirmed(
            self.session_id.as_str(),
            payment_request.amount_to_send,
        )?;

        tracing::info!("Updated session record");

        // Send post payment notification to the server
        let res = self
            .network_client
            .send_request(
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
            )
            .await;

        tracing::info!("Sent post payment notification");

        Ok(Some(payment_request.payment_reference))
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
        .get(PROXY_AUTHORIZATION)
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
