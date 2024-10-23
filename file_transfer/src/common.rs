use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use futures::channel::oneshot;
use libp2p::{identity, Multiaddr, PeerId};
use libp2p::multiaddr::Protocol;
use libp2p::request_response::ResponseChannel;
use ring::digest::{Context, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::btc_rpc::{BTCNetwork, RPCWrapper};
use crate::db_client::{DBClient, FileInfo};
use crate::impl_str_serde;

#[derive(Serialize, Deserialize, Clone)]
pub enum ConfigKey {
    DBPath,
    AppDataPath,
    BTCAddress,
    FeeRatePerKB,
    NetworkType,
    RunHTTPServer,
    TstFileSavePath, // For testing, remove later
}

impl_str_serde!(ConfigKey);

pub struct OrcaNetConfig;

impl OrcaNetConfig {
    pub const NAMESPACE: &'static str = "/orcanet";
    pub const STREAM_PROTOCOL: &'static str = "/orcanet/p2p";
    pub const SECRET_KEY_SEED: u64 = 4;
    pub const CONFIG_FILE_REL_PATH: &'static str = ".orcanet/config.json";
    pub const FILE_SAVE_DIR: &'static str = "file_store_dest";
    pub const MAX_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024; // 10 MB

    pub fn get_bootstrap_peer_id() -> PeerId {
        "12D3KooWQd1K1k8XA9xVEzSAu7HUCodC7LJB6uW5Kw4VwkRdstPE"
            .parse().unwrap()
    }

    pub fn get_bootstrap_address() -> Multiaddr {
        "/ip4/130.245.173.222/tcp/61000/p2p/12D3KooWQd1K1k8XA9xVEzSAu7HUCodC7LJB6uW5Kw4VwkRdstPE"
            .parse().unwrap()
    }

    pub fn get_relay_peer_id() -> PeerId {
        "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            .parse().unwrap()
    }

    pub fn get_relay_address() -> Multiaddr {
        "/ip4/130.245.173.221/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            .parse().unwrap()
    }

    pub fn get_config_file_path() -> PathBuf {
        let home_dir = dirs::home_dir().unwrap();
        home_dir.join(Self::CONFIG_FILE_REL_PATH)
    }

    pub fn get_from_config(config_key: ConfigKey) -> Option<Value> {
        let json_str = std::fs::read_to_string(Self::get_config_file_path())
            .expect(format!("Config file to be present at $HOME/{}", &Self::CONFIG_FILE_REL_PATH).as_str());
        let json: Value = serde_json::from_str(&json_str).unwrap();

        return json.get(config_key.to_string().as_str()).cloned();
    }

    pub fn get_str_from_config(config_key: ConfigKey) -> String {
        Self::get_from_config(config_key.clone())
            .expect(format!("{} to be present in config", config_key.to_string()).as_str())
            .as_str()
            .unwrap()
            .to_string()
    }

    /// Update the config JSON file
    pub fn modify_config(key: &str, value: &str) -> Result<(), Box<dyn Error>> {
        let json_str = std::fs::read_to_string(Self::get_config_file_path())
            .expect(format!("Config file to be present at $HOME/{}", &Self::CONFIG_FILE_REL_PATH).as_str());
        let mut json: Value = serde_json::from_str(&json_str).unwrap();

        if let Some(obj) = json.as_object_mut() {
            obj.insert(key.to_string(), json!(value));
        }

        let updated_json = serde_json::to_string_pretty(&json)?;
        std::fs::write(Self::get_config_file_path(), updated_json)?;

        Ok(())
    }

    pub fn get_fee_rate() -> f64 {
        let amt_str = Self::get_str_from_config(ConfigKey::FeeRatePerKB);

        amt_str.parse()
            .expect("Amount to be valid floating point value in BTC")
    }

    pub fn get_network_type() -> BTCNetwork {
        OrcaNetConfig::get_str_from_config(ConfigKey::NetworkType)
            .as_str().parse()
            .expect("Expect network to be a valid value in config")
    }

    pub fn should_start_http_server() -> bool {
        Self::get_from_config(ConfigKey::RunHTTPServer)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

#[derive(Debug)]
pub enum OrcaNetCommand {
    StartListening {
        addr: Multiaddr,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    Dial {
        peer_id: PeerId,
        peer_addr: Multiaddr,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    StartProviding {
        file_id: String,
        sender: oneshot::Sender<()>,
    },
    StopProviding {
        file_id: String,
    },
    GetProviders {
        file_id: String,
        sender: oneshot::Sender<HashSet<PeerId>>,
    },
    PutKV {
        key: String,
        value: Vec<u8>,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    GetValue {
        key: String,
        sender: oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>,
    },
    Request {
        request: OrcaNetRequest,
        peer: PeerId,
        sender: oneshot::Sender<Result<OrcaNetResponse, Box<dyn Error + Send>>>,
    },
    Respond {
        response: OrcaNetResponse,
        channel: ResponseChannel<OrcaNetResponse>,
    },
    SendInStream {
        peer_id: PeerId,
        stream_req: StreamReq,
        sender: Option<oneshot::Sender<Result<OrcaNetResponse, Box<dyn Error + Send>>>>,
    },
}

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

    pub fn get_file_metadata(file_id: String, db_client: &DBClient) -> Option<(FileInfo, FileMetadata)> {
        match db_client.get_provided_file_info(file_id.as_str()) {
            Ok(file_info) => {
                let file_name = file_info.file_name.clone();
                Some((file_info, FileMetadata {
                    file_id,
                    file_name,
                    fee_rate_per_kb: OrcaNetConfig::get_fee_rate(),
                    recipient_address: OrcaNetConfig::get_str_from_config(ConfigKey::BTCAddress),
                }))
            }
            Err(_) => None
        }
    }

    //TODO: Move to a better struct
    //TODO: Return saved file path?
    /// Expects the response to be a file content response. Saves the file in dest_path
    pub fn handle_file_content_response(resp: OrcaNetResponse, dest_path: Option<String>) {
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

                // Store the file
                match std::fs::write(&path, &content) {
                    Ok(_) => println!("Wrote file {} to {:?}", metadata.file_name, path),
                    Err(e) => eprintln!("Error writing file {:?}", e)
                }

                let size_kb = (content.len() as f64) / 1000f64;
                println!("Received file with size {} KB", size_kb);

                // Send payment after computing size
                let btc_wrapper = RPCWrapper::new(BTCNetwork::RegTest);
                let btc_addr = OrcaNetConfig::get_str_from_config(ConfigKey::BTCAddress);
                let cost_btc = metadata.fee_rate_per_kb * size_kb;
                let comment = format!("Payment for {}", metadata.file_id);
                println!("Initiating transfer of {:?} BTC to {}", cost_btc, btc_addr);

                match btc_wrapper.send_to_address(metadata.recipient_address.as_str(), cost_btc, Some(comment.as_str())) {
                    Ok(tx_id) => {
                        println!("sendtoaddress created transaction: {}", tx_id);
                        // TODO: Handle error case ?
                        let _ = btc_wrapper.generate_to_address(btc_addr.as_str());
                    }
                    Err(e) => {
                        eprintln!("Failed to send btc: {:?}", e);
                    }
                }
            }
            OrcaNetResponse::Error { message } => {
                eprintln!("Failed to fetch file: {message}");
            }
            e => {
                eprintln!("Expected file content response but got {:?}", e)
            }
        }
    }
}

#[derive(Debug)]
pub enum OrcaNetEvent {
    Request {
        request: OrcaNetRequest,
        channel: ResponseChannel<OrcaNetResponse>,
    },
    StreamRequest {
        request: OrcaNetRequest,
        sender: oneshot::Sender<OrcaNetResponse>,
    },
    ProvideFile {
        file_id: String,
        file_path: String,
    },
    StopProvidingFile {
        file_id: String
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamReq {
    pub request_id: String,
    pub stream_data: StreamData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamData {
    Request(OrcaNetRequest),
    Response(OrcaNetResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrcaNetRequest {
    FileMetadataRequest {
        file_id: String,
    },
    FileContentRequest {
        file_id: String,
    },
    HTTPProxyRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HTTPProxyResponse {
    pub address: String,
    pub port: u16,
    pub fee_rate: f64,
    pub recipient_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_id: String,
    pub file_name: String,
    pub fee_rate_per_kb: f64,
    pub recipient_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrcaNetResponse {
    FileMetadataResponse(FileMetadata),
    FileContentResponse {
        metadata: FileMetadata,
        content: Vec<u8>,
    },
    HTTPProxyResponse(Option<HTTPProxyResponse>),
    Error {
        message: String
    },
}
