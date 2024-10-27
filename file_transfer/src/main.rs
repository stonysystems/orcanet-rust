#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;

use std::error::Error;
use std::path::Path;
use std::process::exit;
use std::str::FromStr;

use async_std::task::block_on;
use clap::Parser;
use futures::{SinkExt, StreamExt};
use futures::channel::mpsc;
use tokio::{io, select};
use tokio::io::AsyncBufReadExt;
use tracing_subscriber::EnvFilter;

use crate::common::{OrcaNetConfig, OrcaNetEvent, Utils};
use crate::http::start_http_server;
use crate::network_client::NetworkClient;
use crate::request_handler::RequestHandlerLoop;

mod request_handler;
mod network_client;
mod network;
mod common;
mod btc_rpc;
mod macros;
mod http;
mod db;

#[derive(Parser)]
struct Opts {
    #[arg(long, default_value_t = 4)]
    seed: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let opts = Opts::parse();

    let (mut event_sender, event_receiver) = mpsc::channel::<OrcaNetEvent>(0);
    let (mut network_client, network_event_loop) = network::new(opts.seed, event_sender.clone()).await?;
    let mut request_handler_loop = RequestHandlerLoop::new(network_client.clone(), event_receiver);

    // Network event loop
    tokio::task::spawn(network_event_loop.run());

    // OrcaNet requests event loop
    tokio::task::spawn(request_handler_loop.run());

    // Start HTTP server
    if OrcaNetConfig::should_start_http_server() {
        tokio::task::spawn(start_http_server(network_client.clone(), event_sender.clone()));
    }

    let mut stdin = io::BufReader::new(io::stdin()).lines();

    block_on(async {
        loop {
            select! {
                Ok(Some(line)) = stdin.next_line() => {
                    handle_input_line(&mut network_client, &mut event_sender, line).await;
                }
            }
        }
    });

    Ok(())
}

async fn handle_input_line(
    client: &mut NetworkClient,
    event_sender: &mut mpsc::Sender<OrcaNetEvent>,
    line: String,
) {
    let mut args = line.split(' ');
    let command = args.next();

    // println!("Got command {:?}", command);

    if command.is_none() {
        return;
    }

    match command {
        Some("put") => {
            let key = expect_input!(args.next(), "key", String::from);
            let value = expect_input!(args.next(), "value", |value: &str| value.as_bytes().to_vec());

            let _ = client.put_kv_pair(key, value).await;
        }
        Some("get") => {
            let key = expect_input!(args.next(), "key", String::from);

            match client.get_value(key).await {
                Ok(v) => {
                    println!("Got value {}", String::from_utf8(v).unwrap());
                }
                _ => {}
            }
        }
        Some("addpeer") => {
            let peer_id = expect_input!(args.next(), "peer_id", Utils::get_peer_id_from_input);
            let peer_addr = Utils::get_address_through_relay(&peer_id, None);

            let _ = client.dial(peer_id, peer_addr).await;
        }
        Some("startproviding") => {
            let key = expect_input!(args.next(), "key", String::from);

            let _ = client.start_providing(key).await;
        }
        Some("getproviders") => {
            let key = expect_input!(args.next(), "key", String::from);

            let providers = client.get_providers(key.clone()).await;
            println!("Got providers for {} {:?}", key, providers);
        }
        Some("getfile") => {
            let file_id = expect_input!(args.next(), "file_id", String::from);

            if let Err(e) = client.download_file(file_id, None).await {
                eprintln!("Error getting file: {:?}", e);
            } else {
                println!("Got file");
            }
        }
        Some("providefile") => {
            let file_path = expect_input!(args.next(), "file_path", String::from);
            let path = Path::new(file_path.as_str());

            if !path.exists() {
                return;
            }

            if let Ok(file_id) = Utils::sha256_digest(path) {
                let _ = event_sender.send(OrcaNetEvent::ProvideFile { file_id, file_path }).await;
            }
        }
        Some("advertise") => {
            let _ = client.advertise_provided_files().await;
        }
        Some("startproxy") => {
            let _ = event_sender
                .send(OrcaNetEvent::StartProxyServer)
                .await;
        }
        Some("stopproxy") => {
            let _ = event_sender
                .send(OrcaNetEvent::StopProxyServer)
                .await;
        }
        Some("exit") => {
            exit(0);
        }
        _ => {
            eprintln!("Invalid command {:?}", command);
        }
    }
}