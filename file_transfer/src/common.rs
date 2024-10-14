use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display};
use std::path::PathBuf;
use std::str::FromStr;

use futures::channel::oneshot;
use libp2p::{identity, Multiaddr, PeerId};
use libp2p::multiaddr::Protocol;
use libp2p::request_response::ResponseChannel;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::impl_str_serde;

#[derive(Serialize, Deserialize, Clone)]
pub enum ConfigKey {
    DBPath,
    AppDataPath,
    BTCAddress,
    FeeRatePerKB,
    NetworkType,
}

impl_str_serde!(ConfigKey);

pub struct OrcaNetConfig;

impl OrcaNetConfig {
    pub const NAMESPACE: &'static str = "/orcanet";
    pub const STREAM_PROTOCOL: &'static str = "/orcanet/p2p";
    pub const SECRET_KEY_SEED: u64 = 4;
    pub const CONFIG_FILE_REL_PATH: &'static str = ".orcanet/config.json";
    pub const FILE_SAVE_DIR: &'static str = "file_store_dest";

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
        format!("{}/{}", OrcaNetConfig::NAMESPACE, key)
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
}

#[derive(Debug)]
pub enum OrcaNetEvent {
    Request {
        request: OrcaNetRequest,
        channel: ResponseChannel<OrcaNetResponse>,
    },
    ProvideFile {
        file_id: String,
        file_path: String,
    },
    StopProvidingFile {
        file_id: String
    },
    // GetProvidedFiles,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrcaNetRequest {
    FileRequest { file_id: String }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OrcaNetResponse {
    FileResponse {
        file_name: String,
        fee_rate_per_kb: f64,
        recipient_address: String,
        content: Vec<u8>,
    },
    Error {
        message: String
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum BTCNetwork {
    MainNet,
    TestNet,
    RegTest,
}

impl_str_serde!(BTCNetwork);

impl BTCNetwork {
    pub fn get_rpc_url(self) -> String {
        match self {
            BTCNetwork::MainNet => String::from("http://127.0.0.1:8332"),
            BTCNetwork::TestNet => String::from("http://127.0.0.1:18334"),
            BTCNetwork::RegTest => String::from("http://127.0.0.1:18444")
        }
    }
}
