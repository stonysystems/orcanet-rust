use std::net::SocketAddr;
use std::sync::Arc;

use futures::channel::mpsc::Receiver;
use futures::StreamExt;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};
use tokio::select;
use tracing_subscriber::fmt::format;

use crate::common::{OrcaNetEvent, ProxyMode};
use crate::http::proxy_handlers::*;

const ORCA_NET_CLIENT_ID_HEADER: &str = "orca-net-client-id";
const ORCA_NET_AUTH_KEY_HEADER: &str = "orca-net-token";
const PROXY_PORT: u16 = 3000;


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
                Some(OrcaNetEvent:: StopProxy) => {
                    tracing::info!("Stopping proxy server");
                            return;
                }
                _ => {
                    tracing::info!("Proxy received unsupported event. Ignoring.");
                }
            },

            stream_event = listener.accept() => {
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
}