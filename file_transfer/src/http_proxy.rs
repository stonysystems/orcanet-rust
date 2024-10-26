use std::net::SocketAddr;

use bytes::Bytes;
use futures::channel::mpsc::Receiver;
use futures::StreamExt;
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, Request, Response, StatusCode};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioIo;
use rocket::yansi::Paint;
use tokio::net::TcpListener;
use tokio::select;

use crate::common::OrcaNetEvent;

const ORCA_NET_AUTH_KEY_HEADER: &str = "orca-net-token";
const PROXY_PORT: u16 = 3000;

async fn handle_request(request: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Get token
    println!("Request headers: {:?}", request.headers());
    let orca_net_token = match request.headers().get(ORCA_NET_AUTH_KEY_HEADER) {
        Some(token) => token,
        None => {
            let body = Bytes::from("Orca-Net-Token not found in headers".to_string());

            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(body))
                .expect("Couldn't build body"));
        }
    };

    // Validate token

    // Send the request
    let path = request.uri().path();
    println!("Request path: {path}");

    let client = Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(HttpsConnector::new());
    let response = client.request(request).await.unwrap();

    let (parts, body) = response.into_parts();
    let bytes = body.collect().await?.to_bytes();

    Ok(Response::from_parts(parts, Full::new(bytes)))
}

pub async fn start_proxy(mut receiver: Receiver<OrcaNetEvent>) {
    let addr = SocketAddr::from(([127, 0, 0, 1], PROXY_PORT));
    let listener = TcpListener::bind(addr).await.unwrap();
    println!("Listening on http://{}", addr);

    loop {
        select! {
            event = receiver.next() => match event {
                Some(ev) => {
                    if let OrcaNetEvent::StopProxy = ev {
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
                        .serve_connection(io, service_fn(handle_request))
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