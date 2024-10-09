use std::error::Error;
use std::process::exit;
use std::str::FromStr;
use async_std::task::block_on;
use clap::Parser;
use futures::StreamExt;
use libp2p::PeerId;
use tokio::{io, select};
use tokio::io::AsyncBufReadExt;
use tracing_subscriber::EnvFilter;
use crate::client::NetworkClient;
use crate::common::Utils;

mod request_handlers;
mod client;
mod network;
mod common;

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

    let (mut network_client, mut network_events, network_event_loop) =
        network::new(opts.seed).await?;

    tokio::task::spawn(network_event_loop.run());

    // block_on(async {
    //     loop {
    //         select! {
    //             event = network_events.select_next_some() => {
    //                 println!("Got event");
    //             }
    //         }
    //     }
    // });

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
            let key = {
                match args.next() {
                    Some(key) => Utils::get_key_with_ns(key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };
            let value = {
                match args.next() {
                    Some(value) => value.as_bytes().to_vec(),
                    None => {
                        eprintln!("Expected value");
                        return;
                    }
                }
            };

            let _ = client.put_kv_pair(key, value).await;
        }
        Some("get") => {
            let key = {
                match args.next() {
                    Some(key) => Utils::get_key_with_ns(key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };

            match client.get_value(key).await {
                Ok(v) => {
                    println!("Got value {}", String::from_utf8(v).unwrap());
                }
                _ => {}
            }
        }
        Some("add_peer") => {
            let peer_id = {
                match args.next() {
                    Some(peer_id) => PeerId::from_str(peer_id).unwrap(),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };

            let peer_addr = Utils::get_address_through_relay(&peer_id, None);
            let _ = client.dial(peer_id, peer_addr).await;
        }
        Some("start_providing") => {
            let key = {
                match args.next() {
                    Some(key) => String::from(key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };

            let _ = client.start_providing(key).await;
        }
        Some("get_providers") => {
            let key = {
                match args.next() {
                    Some(key) => String::from(key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };

            let providers = client.get_providers(key.clone()).await;
            println!("Got providers for {} {:?}", key, providers);
        }
        Some("exit") => {
            exit(0);
        }
        _ => {}
    }
}