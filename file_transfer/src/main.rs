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

use crate::common::{OrcaNetConfig, OrcaNetEvent, OrcaNetResponse, Utils};
use crate::network_client::NetworkClient;
use crate::request_handler::RequestHandlerLoop;

mod request_handler;
mod network_client;
mod network;
mod common;
mod db_client;

#[derive(Parser)]
struct Opts {
    #[arg(long, default_value_t = 4)]
    seed: u64,
}

macro_rules! expect_input {
    ($exp:expr, $name:literal, $func:expr) => {
        {
            match $exp {
                Some(input) => $func(input),
                None => {
                    eprintln!("Expected {}", $name);
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

    let (mut event_sender, event_receiver) = mpsc::channel::<OrcaNetEvent>(0);
    let (mut network_client, network_event_loop) = network::new(opts.seed, event_sender.clone()).await?;
    let mut request_handler_loop = RequestHandlerLoop::new(network_client.clone(), event_receiver);

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
            let file_id = expect_input!(args.next(), "file_id", String::from);
            let peer_id = expect_input!(args.next(), "peer_id", Utils::get_peer_id_from_input);

            match client.send_request(peer_id, file_id).await {
                Ok(res) => {
                    // println!("Got file name: {}, content: {}", res.file_name,
                    //          String::from_utf8(res.content).unwrap());

                    match res {
                        OrcaNetResponse::FileResponse {
                            file_name,
                            fee_rate_per_kb,
                            recipient_address,
                            content
                        } => {
                            // Write file
                            let path = Path::new(&OrcaNetConfig::get_app_data_path()).
                                join(file_name.clone());

                            match std::fs::write(&path, &content) {
                                Ok(_) => println!("Wrote file {} to {:?}", file_name, path),
                                Err(e) => eprintln!("Error writing file {:?}", e)
                            }

                            let size = content.len() / 1000;
                            println!("Received file with size {} KB", size);
                            // Send payment after computing size
                        }
                        OrcaNetResponse::Error { message } => {
                            println!("Failed to fetch file {}", message);
                        }
                        // _ => {
                        //     panic!("Wrong response for file request")
                        // }
                    }
                }
                Err(e) => eprintln!("Error when getting file: {:?}", e)
            }
        }
        Some("providefile") => {
            let file_id = expect_input!(args.next(), "file_id", Utils::get_key_with_ns);
            let file_path = expect_input!(args.next(), "file_path", String::from);

            let _ = client.start_providing(file_id.clone()).await;
            let _ = event_sender.send(OrcaNetEvent::ProvideFile { file_id, file_path }).await;
        }
        Some("advertise") => {
            let _ = client.advertise_provided_files().await;
        }
        Some("exit") => {
            exit(0);
        }
        _ => {
            eprintln!("Invalid command {:?}", command);
        }
    }
}