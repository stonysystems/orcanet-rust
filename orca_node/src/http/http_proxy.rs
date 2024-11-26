use std::cmp::PartialEq;
use std::error::Error;
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
use tokio_util::sync::CancellationToken;
use tracing_subscriber::fmt::format;

use crate::common::{OrcaNetEvent, ProxyMode};
use crate::db::{ProxySessionStatus, ProxySessionsTable};
use crate::http::proxy_handlers::*;
use crate::network_client::NetworkClient;

const ORCA_NET_CLIENT_ID_HEADER: &str = "orca-net-client-id";
const ORCA_NET_AUTH_KEY_HEADER: &str = "orca-net-token";
const PROXY_PORT: u16 = 3000;

fn get_handler(
    mode: ProxyMode,
    network_client: NetworkClient,
) -> Result<Box<dyn RequestHandler>, String> {
    match mode {
        ProxyMode::ProxyProvider => Ok(Box::new(ProxyProvider::new())),
        ProxyMode::ProxyClient { session_id } => {
            let mut proxy_sessions_table = ProxySessionsTable::new(None);
            let session_info = proxy_sessions_table
                .get_session_info(session_id.as_str())
                .map_err(|e| e.to_string())?;

            // TODO: Move this into proxy client set up
            let cancellation_token = CancellationToken::new();
            let proxy_payment_loop = ProxyPaymentLoop {
                session_id,
                network_client,
                proxy_sessions_table,
                cancellation_token: cancellation_token.clone(),
            };

            // TODO: Move this into proxy client set up
            tokio::spawn(proxy_payment_loop.start_loop());

            let proxy_status = ProxySessionStatus::try_from(session_info.status)
                .expect("Status to be valid proxy status");

            if proxy_status != ProxySessionStatus::Active {
                Err("Received attempt to start closed session".to_string())
            } else {
                Ok(Box::new(ProxyClient::new(session_info, cancellation_token)))
            }
        }
    }
}

pub async fn start_http_proxy(
    mode: ProxyMode,
    network_client: NetworkClient,
    mut receiver: Receiver<OrcaNetEvent>,
) -> Result<(), String> {
    let addr = match mode {
        ProxyMode::ProxyProvider => SocketAddr::from(([0, 0, 0, 0], PROXY_PORT)), // Listen on all addresses
        ProxyMode::ProxyClient { .. } => SocketAddr::from(([127, 0, 0, 1], PROXY_PORT)), // Only loopback address
    };
    tracing::info!("Creating handler");
    let handler = Arc::new(get_handler(mode, network_client)?);
    let listener = TcpListener::bind(addr)
        .await
        .expect(format!("Tcp listener to be bound to {:?}", addr).as_str());

    tracing::info!("Proxy server listening on http://{}", addr);

    loop {
        select! {
            event = receiver.next() => match event {
                Some(OrcaNetEvent::StopProxy) => {
                    tracing::info!("Stopping proxy server");
                    handler.clean_up().await;
                    return Ok(());
                }
                _ => {}
            },

            stream_event = listener.accept() => {
                let (stream, _) = stream_event
                    .expect("Stream event to be valid");
                let io = TokioIo::new(stream);
                let handler_inner = handler.clone();

                tokio::task::spawn(async move {
                    let resp = http1::Builder::new()
                        .serve_connection(
                            io,
                            service_fn(move |req| {
                                let handler_inner_clone = handler_inner.clone();
                                async move { handler_inner_clone.handle_request(req).await }
                            }),
                        )
                        .with_upgrades()
                        .await;

                    if let Err(err) = resp {
                        tracing::error!("Error serving connection: {:?}", err);
                    }
                });
            }
        }
    }
}
