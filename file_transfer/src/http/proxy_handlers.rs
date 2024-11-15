use std::net::SocketAddr;
use std::sync::Arc;

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
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};

use crate::common::{OrcaNetError, ProxyClientConfig};
use crate::db::{ProxyClientsTable, ProxySessionInfo, ProxySessionsTable};

#[async_trait]
pub trait RequestHandler: Send + Sync {
    async fn handle_request(
        &self,
        request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error>;
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
        let size_kb = (bytes.len() as f32) / 1000f32;
        let amount_owed = client_info.fee_rate_per_kb * size_kb;
        proxy_clients_table
            .update_data_transfer_info(client_info.client_id.as_str(), size_kb, amount_owed)
            .expect("Owed amount to be updated in DB"); // TODO: May be failure is too strict ?

        tracing::info!("Response body size: {:?}", bytes.len());

        Ok(Response::from_parts(parts, Full::new(bytes)))
    }
}

pub struct ProxyClient {
    http_client: Client<ProxyConnector<HttpConnector>, Incoming>,
    session_info: ProxySessionInfo, // We don't record any data here, but only use configuration values like client_id, auth_token etc
}

impl ProxyClient {
    pub fn new(session_info: ProxySessionInfo) -> Self {
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
        let size_kb = (bytes.len() as f32) / 1000f32;
        let amount_owed = self.session_info.fee_rate_per_kb * size_kb;
        let mut proxy_sessions_table = ProxySessionsTable::new(None);
        proxy_sessions_table
            .update_data_transfer_info(self.session_info.session_id.as_str(), size_kb, amount_owed)
            .expect("Amount owed to be updated to DB");

        tracing::info!("Response body size: {:?}", bytes.len());

        Ok(Response::from_parts(parts, Full::new(bytes)))
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
