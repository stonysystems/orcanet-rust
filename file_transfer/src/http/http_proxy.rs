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
use tokio::net::TcpListener;
use tokio::select;

use crate::common::OrcaNetEvent;
use crate::db::ProxyClientsTable;

const ORCA_NET_CLIENT_ID_HEADER: &str = "orca-client-id";
const ORCA_NET_AUTH_KEY_HEADER: &str = "orca-net-token";
const PROXY_PORT: u16 = 3000;

pub enum ProxyMode {
    ProxyProvider,
    ProxyUser,
}

fn bad_request_with_message(message: &str) -> Response<Full<Bytes>> {
    let json_resp = json!({
        "error": message,
    });
    let body = Bytes::from(json_resp.to_string());

    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Full::new(body))
        .expect("Couldn't build body")
}

async fn handle_provide_request(request: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Get client id and auth token
    println!("Request headers: {:?}", request.headers());

    let client_id = match request.headers()
        .get(ORCA_NET_CLIENT_ID_HEADER)
        .map(|v| v.to_str().ok())
        .flatten() {
        Some(client_id) => client_id,
        None => {
            return Ok(bad_request_with_message("Orca-Net-Client-ID not found in headers"));
        }
    };
    let token_in_header = match request.headers()
        .get(ORCA_NET_AUTH_KEY_HEADER)
        .map(|v| v.to_str().ok())
        .flatten() {
        Some(token) => token,
        None => {
            return Ok(bad_request_with_message("Orca-Net-Token not found in headers"));
        }
    };

    // Validate token
    let mut proxy_clients_table = ProxyClientsTable::new(None);
    match proxy_clients_table.get_client_auth_token(client_id) {
        Some(token) => {
            if token_in_header != token {
                return Ok(bad_request_with_message("Orca-Net-Token mismatch"));
            }
        }
        None => {
            return Ok(bad_request_with_message("Client is not registered"));
        }
    }

    println!("Request body size {:?}", request.size_hint().exact());

    // Send the request
    let path = request.uri().path();
    println!("Request path: {path}");

    let client = Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(HttpsConnector::new());
    let response = client.request(request)
        .await
        .unwrap();

    let (parts, body) = response.into_parts();
    let bytes = body.collect()
        .await?
        .to_bytes();

    println!("Response body size: {:?}", bytes.len());

    Ok(Response::from_parts(parts, Full::new(bytes)))
}

async fn handle_use_request(request: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Get token
    println!("Request headers: {:?}", request.headers());

    // Send the request through proxy
    let path = request.uri().path();
    println!("Request path: {path}");

    let client = Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(HttpsConnector::new());
    let response = client.request(request)
        .await
        .unwrap();

    let (parts, body) = response.into_parts();
    let bytes = body.collect()
        .await?
        .to_bytes();

    println!("Response body size: {:?}", bytes.len());

    Ok(Response::from_parts(parts, Full::new(bytes)))
}

pub async fn start_http_proxy(mut receiver: Receiver<OrcaNetEvent>) {
    let addr = SocketAddr::from(([127, 0, 0, 1], PROXY_PORT));
    let listener = TcpListener::bind(addr).await.unwrap();
    println!("Proxy server listening on http://{}", addr);

    loop {
        select! {
            event = receiver.next() => match event {
                Some(ev) => {
                    if let OrcaNetEvent::StopProxyServer = ev {
                        println!("Stopping proxy server");
                        return;
                    }
                }
                _ => {
                    println!("Proxy received unsupported event");
                }
            },

            stream_event = listener.accept() => {
                let (stream, _) = stream_event.unwrap();
                let io = TokioIo::new(stream);

                tokio::task::spawn(async move {
                    if let Err(err) = http1::Builder::new()
                        .serve_connection(io, service_fn(handle_provide_request))
                        .await {
                        println!("Error serving connection: {:?}", err);
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