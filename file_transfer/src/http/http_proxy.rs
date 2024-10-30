use std::net::SocketAddr;

use bytes::Bytes;
use futures::channel::mpsc::Receiver;
use futures::StreamExt;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, StatusCode};
use hyper::body::{Body, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::Client;
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

trait ProxyRequestHandler {
    async fn serve(stream: TcpStream, io: TokioIo<TcpStream>);

    async fn handle_request(request: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error>;
}

struct ProxyProvider;
impl ProxyRequestHandler for ProxyProvider {
    async fn serve(stream: TcpStream, io: TokioIo<TcpStream>) {
        todo!()
    }
}

struct ProxyUser {}

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

async fn handle_provide_request(request: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Get client id and auth token
    tracing::info!("Request headers: {:?}", request.headers());

    let client_id = match request.headers()
        .get(ORCA_NET_CLIENT_ID_HEADER)
        .and_then(|v| v.to_str().ok()) {
        Some(client_id) => client_id,
        None => {
            return Ok(bad_request_with_err(
                OrcaNetError::InvalidClientId("Orca-Net-Client-ID not found in headers".to_string())
            ));
        }
    };
    let token_in_header = match request.headers()
        .get(ORCA_NET_AUTH_KEY_HEADER)
        .and_then(|v| v.to_str().ok()) {
        Some(token) => token,
        None => {
            return Ok(bad_request_with_err(
                OrcaNetError::InvalidAuthToken("Orca-Net-Token not found in headers".to_string())
            ));
        }
    };

    // Validate token
    let mut proxy_clients_table = ProxyClientsTable::new(None);
    match proxy_clients_table.get_client_auth_token(client_id) {
        Some(token) => {
            if token_in_header != token {
                return Ok(bad_request_with_err(
                    OrcaNetError::InvalidAuthToken("Orca-Net-Token mismatch".to_string())
                ));
            }
        }
        None => {
            return Ok(bad_request_with_err(
                OrcaNetError::InvalidClientId("Client is not registered".to_string())
            ));
        }
    }

    tracing::info!("Request body size {:?}", request.size_hint().exact());

    // Send the request
    let path = request.uri().path();
    tracing::info!("Request path: {path}");

    let client = Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(HttpsConnector::new());
    let response = client.request(request)
        .await
        .unwrap();

    let (parts, body) = response.into_parts();
    let bytes = body.collect()
        .await?
        .to_bytes();

    tracing::info!("Response body size: {:?}", bytes.len());

    Ok(Response::from_parts(parts, Full::new(bytes)))
}

async fn handle_use_request(
    request: Request<Incoming>,
    proxy_client_config: ProxyClientConfig,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Get token
    tracing::info!("Request headers: {:?}", request.headers());

    // Send the request through proxy
    let path = request.uri().path();
    tracing::info!("Request path: {path}");

    let client = Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(HttpsConnector::new());
    let response = client.request(request)
        .await
        .unwrap();

    let (parts, body) = response.into_parts();
    let bytes = body.collect()
        .await?
        .to_bytes();

    tracing::info!("Response body size: {:?}", bytes.len());

    Ok(Response::from_parts(parts, Full::new(bytes)))
}

pub async fn start_http_proxy(mode: ProxyMode, mut receiver: Receiver<OrcaNetEvent>) {
    let addr = SocketAddr::from(([0, 0, 0, 0], PROXY_PORT)); // Listen on all addresses
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

                tokio::task::spawn(async move {
                    if let Err(err) = http1::Builder::new()
                        .serve_connection(io, service_fn(handle_provide_request))
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