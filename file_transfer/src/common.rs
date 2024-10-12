use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use futures::channel::oneshot;
use libp2p::{identity, Multiaddr, PeerId};
use libp2p::multiaddr::Protocol;
use libp2p::request_response::ResponseChannel;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct OrcaNetConfig;

impl OrcaNetConfig {
    pub const NAMESPACE: &'static str = "/orcanet";
    pub const STREAM_PROTOCOL: &'static str = "/orcanet/p2p";
    pub const SECRET_KEY_SEED: u64 = 4;
    // pub const CONFIG_FILE_PATH: PathBuf = dirs::home_dir().unwrap().join("/.orcanet/config.json");
    pub const CONFIG_FILE_REL_PATH: &'static str = ".orcanet/config.json";
    pub const FILE_SAVE_DIR: &'static str = "orcanet/file_store_dest";
    pub const FILES_LISTING: &'static str = "orcanet/provided_files.json";

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

    pub fn get_from_config(key: &str) -> Option<Value> {
        let json_str = std::fs::read_to_string(Self::get_config_file_path())
            .expect(format!("Config file to be present at $HOME/{}", &Self::CONFIG_FILE_REL_PATH).as_str());
        let json: Value = serde_json::from_str(&json_str).unwrap();

        return json.get(key).cloned();
    }

    pub fn get_db_path() -> String {
        return Self::get_from_config("db_path")
            .expect("db_path to be present in config")
            .as_str()
            .unwrap()
            .to_string();
    }

    pub fn get_app_data_path() -> String {
        return Self::get_from_config("app_data_path")
            .expect("app_data_path to be present in config")
            .as_str()
            .unwrap()
            .to_string();
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

    pub fn dump_provided_files(app_data_path: &String, files_map: &HashMap<String, String>) {
        let path = Path::new(app_data_path).join(OrcaNetConfig::FILES_LISTING);
        if !path.exists() {
            return;
        }

        let json = serde_json::to_string(files_map)
            .expect("files_map must be serializable");
        let _ = std::fs::write(path, json);
    }

    pub fn load_provided_files(app_data_path: &String) -> HashMap<String, String> {
        let path = Path::new(app_data_path).join(OrcaNetConfig::FILES_LISTING);
        if !path.exists() {
            return Default::default();
        }

        let json = std::fs::read_to_string(path).unwrap();
        let map: HashMap<String, String> = serde_json::from_str(&json)
            .unwrap_or(Default::default());

        return map;
    }
}

#[derive(Debug)]
pub enum OrcaNetEvent {
    FileRequest {
        file_id: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrcaNetResponse {
    FileResponse {
        file_name: String,
        content: Vec<u8>,
    }
}


struct DBClient {}
