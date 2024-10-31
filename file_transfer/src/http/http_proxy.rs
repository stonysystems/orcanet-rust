use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use futures::channel::mpsc::Receiver;
use futures::StreamExt;
use headers::Authorization;
use http_body_util::{BodyExt, Full};
use hyper::{Error, Request, Response, StatusCode};
use hyper::body::{Body, Incoming};
use hyper::header::AUTHORIZATION;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_http_proxy::{Intercept, Proxy, ProxyConnector};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioIo;
use rocket::yansi::Paint;
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};
use tokio::select;

use crate::common::{OrcaNetError, OrcaNetEvent, ProxyClientConfig, ProxyMode};
use crate::db::ProxyClientsTable;

const ORCA_NET_CLIENT_ID_HEADER: &str = "orca-net-client-id";
const ORCA_NET_AUTH_KEY_HEADER: &str = "orca-net-token";
const PROXY_PORT: u16 = 3000;

#[async_trait]
trait RequestHandler: Send + Sync {
    async fn handle_request(
        &self,
        request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error>;
}

struct ProxyProvider {
    http_client: Client<HttpConnector, Incoming>,
}

impl ProxyProvider {
    fn new() -> Self {
        Self {
            http_client: Client::builder(hyper_util::rt::TokioExecutor::new())
                .build(HttpConnector::new())
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
                return Ok(bad_request_with_err(
                    OrcaNetError::AuthorizationFailed(e)
                ));
            }
        };

        // Validate auth token
        let mut proxy_clients_table = ProxyClientsTable::new(None);
        let client_info = match proxy_clients_table.get_client_by_auth_token(auth_token) {
            Ok(client_info) => client_info,
            Err(_) => {
                return Ok(bad_request_with_err(
                    OrcaNetError::AuthorizationFailed("Auth token verification failed".to_string())
                ));
            }
        };

        tracing::info!("Request body size {:?}", request.size_hint().exact());

        // Send the request
        let path = request.uri().path();
        tracing::info!("Request path: {path}");

        let response = self.http_client
            .request(request)
            .await
            .unwrap();

        let (parts, body) = response.into_parts();
        let bytes = body.collect()
            .await?
            .to_bytes();

        // Update client info in DB

        tracing::info!("Response body size: {:?}", bytes.len());

        Ok(Response::from_parts(parts, Full::new(bytes)))
    }
}

struct ProxyClient {
    http_client: Client<ProxyConnector<HttpConnector>, Incoming>,
    config: Arc<ProxyClientConfig>,
}

impl ProxyClient {
    fn new(config: ProxyClientConfig) -> Self {
        // Configure the proxy
        let proxy_uri = config.proxy_address.clone()
            .parse()
            .expect("Proxy address to be valid proxy URI");
        let mut proxy = Proxy::new(Intercept::All, proxy_uri);
        let authorization = Authorization::bearer(config.auth_token.as_str())
            .expect("Authorization token to be valid");
        proxy.set_authorization(authorization);

        // Create the http client
        let proxy_connector = ProxyConnector::from_proxy(HttpConnector::new(), proxy)
            .expect("Proxy connector creation to be successful");
        let http_client = Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(proxy_connector);

        Self {
            http_client,
            config: Arc::new(config),
        }
    }
}

#[async_trait]
impl RequestHandler for ProxyClient {
    async fn handle_request(
        &self,
        request: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, Error> {
        // Get token
        tracing::info!("Request headers: {:?}", request.headers());

        // Send the request through proxy
        let path = request.uri().path();
        tracing::info!("Request path: {path}");

        let response = self.http_client
            .request(request)
            .await
            .unwrap();

        let (parts, body) = response.into_parts();
        let bytes = body.collect()
            .await?
            .to_bytes();

        tracing::info!("Response body size: {:?}", bytes.len());

        Ok(Response::from_parts(parts, Full::new(bytes)))
    }
}

fn get_handler(mode: ProxyMode) -> Box<dyn RequestHandler> {
    match mode {
        ProxyMode::ProxyProvider => Box::new(ProxyProvider::new()),
        ProxyMode::ProxyClient(config) => Box::new(ProxyClient::new(config))
    }
}

pub async fn start_http_proxy(mode: ProxyMode, mut receiver: Receiver<OrcaNetEvent>) {
    let addr = SocketAddr::from(([0, 0, 0, 0], PROXY_PORT)); // Listen on all addresses
    let handler = Arc::new(get_handler(mode));
    let listener = TcpListener::bind(addr)
        .await
        .unwrap();
    tracing::info!("Proxy server listening on http://{}", addr);

    loop {
        select! {
            event = receiver.next() => match event {
                Some(ev) => {
                    if let OrcaNetEvent::StopProxyServer = ev {
                        tracing::info!("Stopping proxy server");
                        return;
                    }
                }
                _ => {
                    tracing::info!("Proxy received unsupported event");
                }
            },

            stream_event = listener.accept() => {
                tracing::info!("Got listener accept");
                let (stream, _) = stream_event.unwrap();
                let io = TokioIo::new(stream);
                let handler_inner = handler.clone();

                tokio::task::spawn(async move {
                    if let Err(err) = http1::Builder::new()
                        .serve_connection(io, service_fn(move |req| {
                        let handler_inner_clone = handler_inner.clone();
                        async move {
                            handler_inner_clone.handle_request(req).await
                        }
                    }))
                        .await {
                        tracing::error!("Error serving connection: {:?}", err);
                    }
                });
            }
        }
    }

    // loop {
    //     let (stream, _) = listener.accept().await.unwrap();
    //     let io = TokioIo::new(stream);
    //
    //     tokio::task::spawn(async move {
    //         if let Err(err) = http1::Builder::new()
    //             .serve_connection(io, service_fn(handle_request))
    //             .await {
    //             println!("Error serving connection: {:?}", err);
    //         }
    //     });
    // }
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
    let auth_header = req.headers()
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