use crate::common::types::{OrcaNetEvent, ProxyMode};
use crate::common::{BitcoindUtil, Utils};
use crate::http_server::start_http_server;
use crate::network::{setup_network_event_loop, NetworkClient};
use crate::request_handler::RequestHandlerLoop;

use crate::common::btc_rpc::RPCWrapper;
use crate::common::config::{ConfigKey, OrcaNetConfig};
use crate::expect_input;
use async_std::task::block_on;
use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};
use std::error::Error;
use std::path::Path;
use std::process::exit;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::{io, select};
use tracing_subscriber::EnvFilter;

const BITCOIND_START_WAIT_SECS: u64 = 5;

pub async fn start_orca_node(seed: Option<u64>) -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let seed = seed.unwrap_or_else(OrcaNetConfig::get_secret_key_seed);

    let (mut event_sender, event_receiver) = mpsc::channel::<OrcaNetEvent>(0);
    let (mut network_client, network_event_loop) =
        setup_network_event_loop(seed, event_sender.clone()).await?;
    let mut request_handler_loop = RequestHandlerLoop::new(network_client.clone(), event_receiver);

    // Network event loop
    tokio::task::spawn(network_event_loop.run());

    // OrcaNet requests event loop
    tokio::task::spawn(request_handler_loop.run());

    // Start HTTP server if needed
    if OrcaNetConfig::should_start_http_server() {
        tokio::task::spawn(start_http_server(
            network_client.clone(),
            event_sender.clone(),
        ));
    }

    // Start Proxy server if needed
    if let Some(proxy_mode) = OrcaNetConfig::get_proxy_config() {
        event_sender
            .send(OrcaNetEvent::StartProxy(proxy_mode))
            .await
            .expect("Proxy start event to be sent");
    }

    // Start bitcoin daemon
    BitcoindUtil::start_bitcoin_daemon()
        .expect("Failed to start bitcoind. Cannot proceed with node start");

    // Wait for the bitcoin daemon to start and load the wallet
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(BITCOIND_START_WAIT_SECS)).await;

        let wallet_name = OrcaNetConfig::get_str_from_config(ConfigKey::BTCWalletName);
        let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());
        // We don't handle error at the moment as the likely cause is that the wallet is already loaded
        // TODO: Assert that the wallet is loaded
        rpc_wrapper.load_wallet(wallet_name.as_str());
    });

    // Listen for input from stdin (TODO: for testing only - comment out later)
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
            let value = expect_input!(args.next(), "value", |value: &str| value
                .as_bytes()
                .to_vec());

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
                let _ = event_sender
                    .send(OrcaNetEvent::ProvideFile { file_id, file_path })
                    .await;
            }
        }
        Some("advertise") => {
            let _ = client.advertise_provided_files().await;
        }
        Some("startproxyprovider") => {
            let _ = event_sender
                .send(OrcaNetEvent::StartProxy(ProxyMode::ProxyProvider))
                .await;
        }
        Some("startproxyclient") => {
            let _ = event_sender
                .send(OrcaNetEvent::StartProxy(ProxyMode::ProxyClient {
                    session_id: "sess1".to_string(),
                }))
                .await;
        }
        Some("stopproxy") => {
            // Don't know which so stop both
            // TODO: Change after adding persistence for proxy state
            let _ = event_sender.send(OrcaNetEvent::StopProxy).await;
        }
        Some("exit") => {
            tracing::info!("Node shutdown initiated");

            match BitcoindUtil::stop_bitcoin_daemon() {
                Ok(_) => tracing::info!("Bitcoind stopped"),
                Err(e) => tracing::error!("Failed to stop bitcoind: {:?}", e),
            }

            exit(0);
        }
        _ => {
            eprintln!("Invalid command {:?}", command);
        }
    }
}
