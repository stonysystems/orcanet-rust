use bitcoincore_rpc::RpcApi;
use libp2p::multiaddr::Protocol;
use libp2p::{identity, Multiaddr, PeerId};
use ring::digest::{Context, SHA256};
use serde::de::Error;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::process::Command;
use std::str::FromStr;
use tokio::net::TcpStream;
use uuid::Uuid;

use crate::common::btc_rpc::{BTCNetwork, RPCWrapper};
use crate::common::config::{ConfigKey, OrcaNetConfig};
use crate::common::types::{OrcaNetRequest, OrcaNetResponse};
use crate::db::{DownloadedFileInfo, DownloadedFilesTable};
use crate::network::NetworkClient;
use tokio::io::AsyncReadExt;

pub struct Utils;

impl Utils {
    pub fn get_address_through_relay(
        peer_id: &PeerId,
        relay_address_override: Option<Multiaddr>,
    ) -> Multiaddr {
        let relay_address = relay_address_override.unwrap_or(OrcaNetConfig::get_relay_address());
        relay_address
            .clone()
            .with(Protocol::P2pCircuit)
            .with(Protocol::P2p(peer_id.clone()))
    }

    pub fn generate_ed25519(secret_key_seed: u64) -> identity::Keypair {
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&secret_key_seed.to_le_bytes());

        identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
    }

    pub fn get_key_with_ns(key: &str) -> String {
        if key.starts_with(OrcaNetConfig::NAMESPACE) {
            key.to_string()
        } else {
            format!("{}/{}", OrcaNetConfig::NAMESPACE, key)
        }
    }

    pub fn get_peer_id_from_input(input: &str) -> PeerId {
        match u64::from_str(input) {
            Ok(seed) => {
                let keypair = Utils::generate_ed25519(seed);
                keypair.public().to_peer_id()
            }
            Err(_) => PeerId::from_str(input).unwrap(),
        }
    }

    pub async fn start_async_block_gen() {
        let address = OrcaNetConfig::get_btc_address();
        tokio::task::spawn(async move {
            let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());
            let _ = rpc_wrapper.generate_to_address(address.as_str());
        });
    }

    pub fn sha256_digest(file_path: &Path) -> Result<String, std::io::Error> {
        let input = File::open(file_path)?;
        let mut reader = BufReader::new(input);
        let mut context = Context::new(&SHA256);
        let mut buffer = [0; 1024];

        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            context.update(&buffer[..count]);
        }

        let digest = context.finish();
        Ok(hex::encode(digest.as_ref()))
    }

    pub fn new_uuid() -> String {
        Uuid::new_v4().to_string()
    }

    pub fn get_unix_timestamp() -> i64 {
        chrono::Utc::now().timestamp()
    }

    pub fn get_file_name_from_path(path: &Path) -> String {
        path.file_name()
            .and_then(|v| v.to_str())
            .and_then(|v| v.parse().ok())
            .unwrap()
    }

    /// Uses RequestResponse for small metadata requests.
    /// Should not be used when Stream Request is required.
    pub async fn request_from_peers(
        request: OrcaNetRequest,
        network_client: NetworkClient,
        peers: impl Iterator<Item = PeerId>,
    ) -> Vec<(PeerId, OrcaNetResponse)> {
        let mut results = Vec::new();

        for peer_id in peers {
            let response = network_client
                .clone()
                .send_request(peer_id.clone(), request.clone())
                .await;

            match response {
                Ok(resp) => {
                    results.push((peer_id, resp));
                }
                Err(e) => {
                    tracing::error!(
                        "Error getting file metadata from peer {:?}. Error: {:?}",
                        peer_id,
                        e
                    )
                }
                _ => {}
            }
        }

        results
    }

    pub fn get_percent_diff(value: f64, target: f64) -> f64 {
        if target == 0.0 {
            return 0.0;
        }

        let difference = ((value - target) / target).abs() * 100.0;
        difference
    }

    pub fn round(x: f64, decimals: u32) -> f64 {
        // May not be fully accurate for long number but good enough for our case
        let y = 10i32.pow(decimals) as f64;
        (x * y).round() / y
    }

    pub async fn read_stream_to_end(stream: &mut TcpStream) -> Vec<u8> {
        let mut data = Vec::new();
        let mut buffer = [0; 1024];

        loop {
            let n = stream
                .read(&mut buffer)
                .await
                .expect("Response to be read from TCP stream");
            data.extend_from_slice(&buffer[..n]);
            if data.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }

        data
    }

    //TODO: Move to a better struct
    //TODO: Return saved file path?
    /// Expects the response to be a file content response. Saves the file in dest_path
    pub fn handle_file_content_response(
        peer_id: PeerId,
        resp: OrcaNetResponse,
        dest_path: Option<String>,
    ) {
        match resp {
            OrcaNetResponse::FileContentResponse { metadata, content } => {
                let path = match dest_path {
                    Some(dest_path) => Path::new(&dest_path).to_path_buf(),
                    None => {
                        let app_data_path =
                            OrcaNetConfig::get_str_from_config(ConfigKey::AppDataPath);
                        Path::new(&app_data_path)
                            // .join(OrcaNetConfig::FILE_SAVE_DIR)
                            .join(format!(
                                "{}_{}",
                                &metadata.file_id[..16],
                                metadata.file_name.clone()
                            )) // Use file_id and name
                    }
                };

                let size_kb = (content.len() as f64) / 1000f64;
                tracing::info!("Received file with size {} KB", size_kb);

                // Store the file
                match std::fs::write(&path, &content) {
                    Ok(_) => {
                        tracing::info!("Wrote file {} to {:?}", metadata.file_name, path);
                    }
                    Err(e) => tracing::error!("Error writing file {:?}", e),
                }

                // Send payment after computing size
                let btc_wrapper = RPCWrapper::new(BTCNetwork::RegTest);
                let btc_addr = OrcaNetConfig::get_btc_address();
                let cost_btc = Utils::round(
                    metadata.fee_rate_per_kb * size_kb,
                    OrcaNetConfig::BTC_PRECISION,
                );
                let comment = format!("Payment for {}", metadata.file_id);
                tracing::info!("Initiating transfer of {:?} BTC to {}", cost_btc, btc_addr);

                let payment_tx_id = match btc_wrapper.send_to_address(
                    metadata.recipient_address.as_str(),
                    cost_btc,
                    Some(comment.as_str()),
                ) {
                    Ok(tx_id) => {
                        tracing::info!("sendtoaddress created transaction: {}", tx_id);
                        // TODO: Handle error case ?
                        let _ = btc_wrapper.generate_to_address(btc_addr.as_str());
                        Some(tx_id.to_string())
                    }
                    Err(e) => {
                        tracing::error!("Failed to send btc: {:?}", e);
                        None
                    }
                };

                // Record the download in DB
                let mut downloaded_files_table = DownloadedFilesTable::new(None);
                let downloaded_file_info = DownloadedFileInfo {
                    id: Utils::new_uuid(),
                    file_id: metadata.file_id.clone(),
                    file_name: metadata.file_name.clone(),
                    file_size_kb: size_kb.clone(),
                    file_path: path.to_str().unwrap().to_string(),
                    fee_rate_per_kb: Some(metadata.fee_rate_per_kb.clone()),
                    peer_id: peer_id.to_string(),
                    price: Some(cost_btc), // Will change if we use per-file price instead of rate
                    payment_tx_id,
                    download_timestamp: Utils::get_unix_timestamp(),
                };

                match downloaded_files_table.insert_downloaded_file(&downloaded_file_info) {
                    Ok(_) => tracing::info!("Inserted record for downloaded file"),
                    Err(e) => tracing::error!("Error inserting download record {:?}", e),
                }
            }
            OrcaNetResponse::Error(error) => {
                tracing::error!("Failed to fetch file: {:?}", error);
            }
            e => {
                tracing::error!("Expected file content response but got {:?}", e)
            }
        }
    }
}

pub struct BitcoindUtil {}

impl BitcoindUtil {
    fn get_bitcoind_command() -> Command {
        let mut cmd = Command::new("bitcoind");
        match OrcaNetConfig::get_network_type() {
            BTCNetwork::MainNet => {}
            BTCNetwork::TestNet => {
                cmd.arg("--testnet");
            }
            BTCNetwork::RegTest => {
                cmd.arg("--regtest");
            }
        }

        cmd
    }

    pub fn start_bitcoin_daemon() -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Self::get_bitcoind_command();
        cmd.args(["-server", "-fallbackfee=0.0002", "-txindex", "-daemon"]);

        tracing::info!("Executing bitcoind start command: {:?}", cmd);
        let out = cmd.output()?;

        if out.status.success() {
            tracing::info!("Started bitcoin daemon");
            Ok(())
        } else if String::from_utf8(out.stderr)
            .expect("stderr to be utf-8 text")
            .contains("already running")
        {
            tracing::info!("Bitcoind already running");
            Ok(())
        } else {
            tracing::error!("Failed to start bitcoind");
            Err("Failed to start bitcoind".into())
        }
    }

    pub fn stop_bitcoin_daemon() -> bitcoincore_rpc::Result<String> {
        let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());
        rpc_wrapper.get_client().stop()
    }
}
