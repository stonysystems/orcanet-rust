use crate::common::btc_rpc::BTCNetwork;
use crate::common::types::ProxyMode;
use crate::impl_str_serde;
use libp2p::{Multiaddr, PeerId};
use rocket::serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::error::Error;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ConfigKey {
    DBPath,
    AppDataPath,
    BTCWalletName,
    BTCAddress,
    FileFeeRatePerKB,
    ProxyFeeRatePerKB,
    NetworkType,
    RunHTTPServer,
    ProxyConfig,
    SecretKeySeed,
    ProxyProviderServerAddress, // For provider
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
    pub const PROXY_PROVIDER_KEY_DHT: &'static str = "http_proxy_providers";
    pub const DEFAULT_SECRET_KEY_SEED: u64 = 4;
    pub const PROXY_PAYMENT_INTERVAL_SECS: u64 = 10;
    pub const BTC_PRECISION: u32 = 8; // Bitcoin max precision is 8 decimals

    // TODO: Having high values for testing. Decide these later.
    pub const FEE_OWED_PD_ALLOWED: f64 = 300.0; // Percent diff allowed
    pub const DATA_TRANSFER_PD_ALLOWED: f64 = 300.0; // Percent diff allowed
    pub const PROXY_TERMINATION_PD_THRESHOLD: f64 = 300.0; // Beyond this, terminate the connection

    pub fn get_bootstrap_peer_id() -> PeerId {
        "12D3KooWQd1K1k8XA9xVEzSAu7HUCodC7LJB6uW5Kw4VwkRdstPE"
            .parse()
            .unwrap()
    }

    pub fn get_bootstrap_address() -> Multiaddr {
        "/ip4/130.245.173.222/tcp/61000/p2p/12D3KooWQd1K1k8XA9xVEzSAu7HUCodC7LJB6uW5Kw4VwkRdstPE"
            .parse()
            .unwrap()
    }

    pub fn get_relay_peer_id() -> PeerId {
        "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            .parse()
            .unwrap()
    }

    pub fn get_relay_address() -> Multiaddr {
        "/ip4/130.245.173.221/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            .parse()
            .unwrap()
    }

    pub fn get_config_file_path() -> PathBuf {
        let home_dir = dirs::home_dir().unwrap();
        home_dir.join(Self::CONFIG_FILE_REL_PATH)
    }

    fn get_config_json() -> Value {
        let json_str = std::fs::read_to_string(Self::get_config_file_path()).expect(
            format!(
                "Config file to be present at $HOME/{}",
                &Self::CONFIG_FILE_REL_PATH
            )
            .as_str(),
        );
        serde_json::from_str(&json_str).unwrap()
    }

    pub fn get_from_config(config_key: ConfigKey) -> Option<Value> {
        Self::get_config_json()
            .get(config_key.to_string().as_str())
            .cloned()
    }

    pub fn get_str_from_config(config_key: ConfigKey) -> String {
        Self::get_from_config(config_key.clone())
            .expect(format!("{:?} to be present in config", config_key).as_str())
            .as_str()
            .expect(format!("{:?} to be valid as a string", config_key).as_str())
            .to_string()
    }

    /// Update the config JSON file
    pub fn modify_config(key: &str, value: Value) -> Result<(), Box<dyn Error>> {
        Self::modify_config_with_kv_pair(HashMap::from([(key.to_string(), value)]))
    }

    /// Update the config JSON file
    pub fn modify_config_with_kv_pair(
        kv_pair: HashMap<String, Value>,
    ) -> Result<(), Box<dyn Error>> {
        let mut json: Value = Self::get_config_json();
        let object_mut = json
            .as_object_mut()
            .expect("Config to be a valid json object");

        for (k, v) in kv_pair {
            object_mut.insert(k, v);
        }

        let updated_json = serde_json::to_string_pretty(&json)?;
        std::fs::write(Self::get_config_file_path(), updated_json)?;

        Ok(())
    }

    pub fn get_file_fee_rate() -> f64 {
        Self::get_from_config(ConfigKey::FileFeeRatePerKB)
            .and_then(|v| v.as_f64())
            .expect("Amount to be valid floating point value in BTC")
    }

    pub fn get_proxy_fee_rate() -> f64 {
        Self::get_from_config(ConfigKey::ProxyFeeRatePerKB)
            .and_then(|v| v.as_f64())
            .expect("Amount to be valid floating point value in BTC")
    }

    pub fn get_network_type() -> BTCNetwork {
        Self::get_str_from_config(ConfigKey::NetworkType)
            .as_str()
            .parse()
            .expect("Expect network to be a valid value in config")
    }

    pub fn get_secret_key_seed() -> u64 {
        Self::get_from_config(ConfigKey::SecretKeySeed)
            .and_then(|v| v.as_u64())
            .unwrap_or(Self::DEFAULT_SECRET_KEY_SEED)
    }

    pub fn get_proxy_config() -> Option<ProxyMode> {
        Self::get_from_config(ConfigKey::ProxyConfig)
            .and_then(|v| serde_json::from_value::<ProxyMode>(v).ok())
    }

    pub fn should_start_http_server() -> bool {
        Self::get_from_config(ConfigKey::RunHTTPServer)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn get_btc_address() -> String {
        Self::get_str_from_config(ConfigKey::BTCAddress)
    }
}
