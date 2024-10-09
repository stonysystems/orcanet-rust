use std::error::Error;
use std::process::exit;
use std::str::FromStr;

use async_std::task::block_on;
use clap::Parser;
use futures::StreamExt;
use tokio::{io, select};
use tokio::io::AsyncBufReadExt;
use tracing_subscriber::EnvFilter;

use crate::client::NetworkClient;
use crate::common::Utils;
use crate::request_handlers::RequestHandlerLoop;

mod request_handlers;
mod client;
mod network;
mod common;

#[derive(Parser)]
struct Opts {
    #[arg(long, default_value_t = 4)]
    seed: u64,
}

macro_rules! expect_input {
    ($exp:expr, $x:literal, $fn:expr) => {
        {
            match $exp {
                Some(input) => $fn(input),
                None => {
                    eprintln!("Expected {}", $x);
                    return;
                }
            }
        }
    };
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let opts = Opts::parse();

    let (mut network_client, mut network_events, network_event_loop) =
        network::new(opts.seed).await?;
    let mut request_handler_loop = RequestHandlerLoop::new(network_client.clone(), network_events);

    // Network event loop
    tokio::task::spawn(network_event_loop.run());

    // OrcaNet requests event loop
    tokio::task::spawn(request_handler_loop.run());

    let mut stdin = io::BufReader::new(io::stdin()).lines();

    block_on(async {
        loop {
            select! {
                Ok(Some(line)) = stdin.next_line() => {
                    // handle_input_line(&mut swarm.behaviour_mut().kademlia, line);
                    handle_input_line(&mut network_client, line).await;
                }
            }
        }
    });

    Ok(())
}

async fn handle_input_line(client: &mut NetworkClient, line: String) {
    let mut args = line.split(' ');
    let command = args.next();

    // println!("Got command {:?}", command);

    if command.is_none() {
        return;
    }

    match command {
        Some("put") => {
            let key = expect_input!(args.next(), "key", Utils::get_key_with_ns);
            let value = expect_input!(args.next(), "value", |value: &str| value.as_bytes().to_vec());

            let _ = client.put_kv_pair(key, value).await;
        }
        Some("get") => {
            let key = expect_input!(args.next(), "key", Utils::get_key_with_ns);

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
            let key = expect_input!(args.next(), "key", Utils::get_key_with_ns);

            let _ = client.start_providing(key).await;
        }
        Some("getproviders") => {
            let key = expect_input!(args.next(), "key", Utils::get_key_with_ns);

            let providers = client.get_providers(key.clone()).await;
            println!("Got providers for {} {:?}", key, providers);
        }
        Some("getfile") => {
            let file_id = expect_input!(args.next(), "file_id", Utils::get_key_with_ns);
            let peer_id = expect_input!(args.next(), "peer_id", Utils::get_peer_id_from_input);

            match client.request_file(peer_id, file_id).await {
                Ok(res) => {
                    // let a = String::from()
                    println!("Got file content: {}", String::from_utf8(res).unwrap());
                }
                Err(e) => eprintln!("Error when getting file: {:?}", e)
            }
        }
        Some("exit") => {
            exit(0);
        }
        _ => {}
    }
}