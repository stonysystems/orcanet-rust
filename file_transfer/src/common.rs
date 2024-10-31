use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::str::FromStr;

use futures::channel::oneshot;
use libp2p::{Multiaddr, PeerId};
use libp2p::request_response::ResponseChannel;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::btc_rpc::BTCNetwork;
use crate::impl_str_serde;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ConfigKey {
    DBPath,
    AppDataPath,
    BTCAddress,
    FeeRatePerKB,
    NetworkType,
    RunHTTPServer,
    ProxyConfig,
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
    pub const PROXY_PORT: u16 = 3000;

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

    fn get_config_json() -> Value {
        let json_str = std::fs::read_to_string(Self::get_config_file_path())
            .expect(format!("Config file to be present at $HOME/{}", &Self::CONFIG_FILE_REL_PATH).as_str());
        serde_json::from_str(&json_str)
            .unwrap()
    }

    pub fn get_from_config(config_key: ConfigKey) -> Option<Value> {
        return Self::get_config_json()
            .get(config_key.to_string().as_str())
            .cloned();
    }

    pub fn get_str_from_config(config_key: ConfigKey) -> String {
        Self::get_from_config(config_key.clone())
            .expect(format!("{:?} to be present in config", config_key).as_str())
            .as_str()
            .expect(format!("{:?} to be a string", config_key).as_str())
            .to_string()
    }

    /// Update the config JSON file
    pub fn modify_config(key: &str, value: &str) -> Result<(), Box<dyn Error>> {
        let mut json: Value = Self::get_config_json();

        if let Some(obj) = json.as_object_mut() {
            obj.insert(key.to_string(), json!(value));
        }

        let updated_json = serde_json::to_string_pretty(&json)?;
        std::fs::write(Self::get_config_file_path(), updated_json)?;

        Ok(())
    }

    pub fn get_fee_rate() -> f64 {
        Self::get_str_from_config(ConfigKey::FeeRatePerKB)
            .parse()
            .expect("Amount to be valid floating point value in BTC")
    }

    pub fn get_network_type() -> BTCNetwork {
        Self::get_str_from_config(ConfigKey::NetworkType)
            .as_str()
            .parse()
            .expect("Expect network to be a valid value in config")
    }

    pub fn should_start_http_server() -> bool {
        Self::get_from_config(ConfigKey::RunHTTPServer)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

#[derive(Debug)]
pub enum NetworkCommand {
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
        file_id: String,
        permanent: bool, // Permanently stop providing
    },
    StartProxyServer,
    StopProxyServer,
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
    HTTPProxyMetadataRequest,
    HTTPProxyProvideRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HTTPProxyMetadata {
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
    HTTPProxyMetadataResponse(HTTPProxyMetadata),
    HTTPProxyProvideResponse {
        metadata: HTTPProxyMetadata,
        client_id: String,
        auth_token: String,
    },
    Error(OrcaNetError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "message")]
pub enum OrcaNetError {
    AuthorizationFailed(String),
    FileProvideError(String),
    NotAProvider(String),
    InternalServerError(String),
}

#[derive(Debug, Clone)]
pub struct ProxyClientConfig {
    pub proxy_address: String,
    pub client_id: String,
    pub auth_token: String,
}

#[derive(Clone)]
pub enum ProxyMode {
    ProxyProvider,
    ProxyClient(ProxyClientConfig),
}