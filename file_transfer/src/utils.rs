use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::str::FromStr;

use libp2p::{identity, Multiaddr, PeerId};
use libp2p::multiaddr::Protocol;
use ring::digest::{Context, SHA256};
use uuid::Uuid;

use crate::btc_rpc::{BTCNetwork, RPCWrapper};
use crate::common::{ConfigKey, OrcaNetConfig, OrcaNetResponse};
use crate::db::{DownloadedFileInfo, DownloadedFilesTable};

pub struct Utils;

impl Utils {
    pub fn get_address_through_relay(peer_id: &PeerId, relay_address_override: Option<Multiaddr>) -> Multiaddr {
        let relay_address = relay_address_override.unwrap_or(OrcaNetConfig::get_relay_address());
        relay_address.clone()
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
            Err(_) => PeerId::from_str(input).unwrap()
        }
    }

    pub async fn start_async_block_gen() {
        let address = OrcaNetConfig::get_str_from_config(ConfigKey::BTCAddress);
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

    // pub fn get_file_metadata(file_id: String, db_client: &mut DBClient) -> Option<(FileInfo, FileMetadata)> {
    //     match db_client.get_provided_file_info(file_id.as_str()) {
    //         Ok(file_info) => {
    //             let file_name = file_info.file_name.clone();
    //             Some((file_info, FileMetadata {
    //                 file_id,
    //                 file_name,
    //                 fee_rate_per_kb: OrcaNetConfig::get_fee_rate(),
    //                 recipient_address: OrcaNetConfig::get_str_from_config(ConfigKey::BTCAddress),
    //             }))
    //         }
    //         Err(_) => None
    //     }
    // }

    //TODO: Move to a better struct
    //TODO: Return saved file path?
    /// Expects the response to be a file content response. Saves the file in dest_path
    pub fn handle_file_content_response(peer_id: PeerId, resp: OrcaNetResponse, dest_path: Option<String>) {
        match resp {
            OrcaNetResponse::FileContentResponse {
                metadata,
                content
            } => {
                let path = match dest_path {
                    Some(dest_path) => Path::new(&dest_path).to_path_buf(),
                    None => {
                        let app_data_path = OrcaNetConfig::get_str_from_config(ConfigKey::AppDataPath);
                        Path::new(&app_data_path)
                            .join(OrcaNetConfig::FILE_SAVE_DIR)
                            .join(format!("{}_{}", &metadata.file_id[..16], metadata.file_name.clone())) // Use file_id and name
                    }
                };

                let size_kb = (content.len() as f64) / 1000f64;
                tracing::info!("Received file with size {} KB", size_kb);

                // Store the file
                match std::fs::write(&path, &content) {
                    Ok(_) => {
                        tracing::info!("Wrote file {} to {:?}", metadata.file_name, path);
                    }
                    Err(e) => tracing::error!("Error writing file {:?}", e)
                }

                // Send payment after computing size
                let btc_wrapper = RPCWrapper::new(BTCNetwork::RegTest);
                let btc_addr = OrcaNetConfig::get_str_from_config(ConfigKey::BTCAddress);
                let cost_btc = metadata.fee_rate_per_kb * size_kb;
                let comment = format!("Payment for {}", metadata.file_id);
                tracing::info!("Initiating transfer of {:?} BTC to {}", cost_btc, btc_addr);

                let payment_tx_id = match btc_wrapper
                    .send_to_address(metadata.recipient_address.as_str(), cost_btc, Some(comment.as_str())) {
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
                    file_size_kb: size_kb.clone() as f32,
                    file_path: path.to_str().unwrap().to_string(),
                    fee_rate_per_kb: Some(metadata.fee_rate_per_kb.clone() as f32),
                    peer_id: peer_id.to_string(),
                    price: Some(cost_btc as f32), // Will change if we use per-file price instead of rate
                    payment_tx_id,
                    download_timestamp: Utils::get_unix_timestamp(),
                };

                match downloaded_files_table.insert_downloaded_file(&downloaded_file_info) {
                    Ok(_) => tracing::info!("Inserted record for downloaded file"),
                    Err(e) => tracing::error!("Error inserting download record {:?}", e)
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