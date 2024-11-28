use crate::btc_rpc::RPCWrapper;
use crate::common::{
    ConfigKey, OrcaNetConfig, OrcaNetError, OrcaNetRequest, OrcaNetResponse, PaymentNotification,
    PaymentRequest, PrePaymentResponse, ProxyClientConfig,
};
use crate::db::{
    PaymentCategory, PaymentInfo, PaymentStatus, PaymentsTable, ProxyClientInfo, ProxyClientsTable,
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
use hyper::header::{HeaderValue, CONNECTION, PROXY_AUTHORIZATION};
use hyper::http::uri::Authority;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Error, Method, Request, Response, StatusCode, Uri};
use hyper_http_proxy::{Intercept, Proxy, ProxyConnector};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioIo;
use libp2p_swarm::derive_prelude::PeerId;
use rocket::form::{FromForm, Options};
use serde_json::json;
use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::net::SocketAddr;
use std::str::FromStr;
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
    ) -> Result<ProxyClientInfo, Response<Full<Bytes>>> {
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
            _ => Ok(client_info),
        }
    }

    async fn handle_standard_http_request(
        &self,
        client_info: ProxyClientInfo,
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

    async fn handle_http_connect_request(
        &self,
        client_info: ProxyClientInfo,
        mut request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        tracing::info!("Got HTTP CONNECT request");
        let authority = request.uri().authority().unwrap().as_str();

        match TcpStream::connect(authority).await {
            Ok(target_stream) => {
                tracing::info!("Connected to authority");
                let client_id = client_info.client_id.clone();

                // Wait for the request upgrade to succeed and start writing bidirectionally
                // We need to do this waiting in a new thread because we have to respond OK to the client
                // so that the client will initiate TLS handshake with the destination server
                // We can't block here
                tokio::task::spawn(async move {
                    tracing::info!("Upgrading request {:?}", request);

                    match hyper::upgrade::on(request).await {
                        Ok(upgraded) => {
                            tracing::info!("Upgraded connection");
                            tunnel_streams(upgraded, target_stream, move |data_transferred| {
                                let data_kb = data_transferred as f64 / 1000f64;
                                tracing::info!(
                                    "Data transferred kb {data_kb} for client: {client_id}"
                                );

                                let mut proxy_clients_table = ProxyClientsTable::new(None);
                                proxy_clients_table
                                    .update_data_transfer_info(client_id.as_str(), data_kb)
                                    .expect("Data transfer info to be updated in DB");
                            })
                            .await;
                        }
                        Err(e) => {
                            tracing::error!("Hyper upgrade error: {:?}", e);
                        }
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

#[async_trait]
impl RequestHandler for ProxyProvider {
    async fn handle_request(
        &self,
        request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        tracing::info!("Request headers: {:?}", request.headers());
        tracing::info!("Request body size {:?}", request.size_hint().exact());

        match self.validate_auth_token(&request) {
            Ok(client_info) => {
                let path = request.uri().path();
                tracing::info!("Request path: {path}");

                if request.method() == Method::CONNECT {
                    self.handle_http_connect_request(client_info, request).await
                } else {
                    self.handle_standard_http_request(client_info, request)
                        .await
                }
            }
            Err(error_response) => Ok(error_response),
        }
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
        let proxy_uri_str = format!("http://{}", session_info.proxy_address.as_str());
        let proxy_uri = proxy_uri_str
            .parse()
            .expect("Proxy address to be valid proxy URI");
        let proxy = Proxy::new(Intercept::All, proxy_uri);

        // Create the http client
        let proxy_connector = ProxyConnector::from_proxy(HttpConnector::new(), proxy)
            .expect("Proxy connector creation failed");
        let http_client =
            Client::builder(hyper_util::rt::TokioExecutor::new()).build(proxy_connector.clone());

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
        tracing::info!("Got request {:?}", request);
        if request.method() == Method::CONNECT {
            self.handle_http_connect_request(request).await
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

        // Add Bearer token
        let token_hdr_value =
            HeaderValue::from_str(format!("Bearer {}", self.session_info.auth_token).as_str())
                .expect("Token value to be valid header value when parsed from string");

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

    /// Create a connection to the proxy and ask the proxy to create a TCP connection to the destination.
    /// Returns a TcpStream to the remote proxy if successful
    async fn connect_to_proxy(
        &self,
        target_uri: Uri,
    ) -> Result<TcpStream, Box<dyn std::error::Error>> {
        let bearer_token = format!("Bearer {}", self.session_info.auth_token);
        let mut headers = HashMap::from([
            (PROXY_AUTHORIZATION.as_str(), bearer_token.as_str()),
            (CONNECTION.as_str(), "keep-alive"),
            ("proxy-connection", "keep-alive"),
        ]);

        // Create TCP connection to the proxy and send CONNECT request
        // If the proxy accepts, then the proxy should have created a TCP connection to the origin server
        // And we will have this TCP connection to the proxy. So when the client starts the TLS handshake,
        // it will be forwarded through the remote proxy to the origin server
        // Using HTTP request and then trying to upgrade the response did not work, probably cuz responses were never meant to be upgraded ?
        // So manual TCP connection is our best bet
        let mut proxy_stream = TcpStream::connect(self.session_info.proxy_address.as_str()).await?;
        let req_str = format!(
            "{} {} HTTP/1.1\r\n{}\r\n\r\n",
            Method::CONNECT,
            target_uri,
            headers
                .iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect::<Vec<_>>()
                .join("\r\n")
        );
        proxy_stream.write_all(req_str.as_bytes()).await?;

        // Handle the response. We need the proxy to say 200 OK to proceed as stated above.
        let response = Utils::read_stream_to_end(&mut proxy_stream).await;

        if response.starts_with(b"HTTP/1.1 200") {
            Ok(proxy_stream)
        } else {
            Err("Failed to connect to proxy".into())
        }
    }

    async fn handle_http_connect_request(
        &self,
        request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        match self.connect_to_proxy(request.uri().clone()).await {
            Ok(proxy_stream) => {
                let session_id = self.session_info.session_id.clone();

                // Upgrade the request and set up bidirectional copying between request stream and proxy_stream
                tokio::task::spawn(async move {
                    match hyper::upgrade::on(request).await {
                        Ok(client_stream) => {
                            tracing::info!("Upgrade succeeded");
                            tunnel_streams(client_stream, proxy_stream, move |data_transferred| {
                                let data_kb = data_transferred as f64 / 1000f64;
                                println!("Data transferred kb {data_kb} for session: {session_id}");

                                let mut proxy_sessions_table = ProxySessionsTable::new(None);
                                proxy_sessions_table
                                    .update_data_transfer_info(session_id.as_str(), data_kb)
                                    .expect("Data transfer info to be updated in DB");
                            })
                            .await;
                        }
                        Err(e) => {
                            tracing::error!("Upgrade failed: {:?}", e);
                        }
                    }
                });

                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(Full::default())
                    .unwrap())
            }
            Err(e) => {
                tracing::error!("Error in HTTP CONNECT to proxy: {:?}", e);

                Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Full::default())
                    .unwrap())
            }
        }
    }
}

async fn tunnel_streams<F>(
    client_stream: hyper::upgrade::Upgraded,
    mut remote_stream: TcpStream,
    on_close: F,
) where
    F: FnOnce(u64),
{
    println!("In tunnel start");
    let mut client_stream = TokioIo::new(client_stream);

    match tokio::io::copy_bidirectional(&mut client_stream, &mut remote_stream).await {
        Ok((from_client, from_remote)) => {
            tracing::info!(
                "Client wrote {} bytes and target wrote {} bytes",
                from_client,
                from_remote
            );

            on_close(from_client + from_remote);
        }
        Err(e) => {
            tracing::error!("Error in tunnel: {}", e);
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
