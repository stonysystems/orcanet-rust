use bitcoincore_rpc::RpcApi;
use futures::channel::mpsc;
use futures::SinkExt;
use ring::digest::digest;
use rocket::{get, routes, State};
use rocket::time::format_description::parse;

use crate::btc_rpc::RPCWrapper;
use crate::common::{ConfigKey, OrcaNetConfig, OrcaNetEvent, Utils};
use crate::network_client::NetworkClient;

pub struct AppState {
    pub network_client: NetworkClient,
    pub event_sender: mpsc::Sender<OrcaNetEvent>,
}

// Wallet
#[get("/blocks-count")]
fn get_block_count() -> String {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    rpc_wrapper.get_client()
        .get_block_count()
        .expect("to get block count")
        .to_string()
}

#[get("/balance")]
fn get_balance() -> String {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    rpc_wrapper.get_client()
        .get_balance(None, None)
        .expect("to get balance count")
        .to_string()
}

#[get("/load-wallet/<wallet_name>")]
fn load_wallet(wallet_name: String) -> String {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    match rpc_wrapper.get_client().load_wallet(wallet_name.as_str()) {
        Ok(_) => {
            "Wallet loaded".parse().unwrap()
        }
        Err(e) => {
            format!("Error loading wallet {:?}", e)
        }
    }
}

#[get("/send-to-address/<address>?<amt>")]
fn send_to_address(address: String, amt: f64) -> String {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    // rpc_wrapper.send_to_address(address.as_str(), amt)
    //     .map(|tx_id| format!("Created transaction {:?}", tx_id))
    //     .unwrap_or_else(|e| format!("Failed to send: {:?}", e))

    match rpc_wrapper.send_to_address(address.as_str(), amt) {
        Ok(tx_id) => {
            format!("Created transaction {:?}", tx_id)
        }
        Err(e) => {
            format!("Failed to send: {:?}", e)
        }
    }
}

#[get("/generate-block")]
async fn generate_block() -> String {
    Utils::start_async_block_gen().await;
    "Started generation".to_string()

    // match rpc_wrapper.generate_to_address(address.as_str()) {
    //     Ok(blockhash) => {
    //         format!("Created transaction {:?}", blockhash)
    //     }
    //     Err(e) => {
    //         format!("Failed to generate: {:?}", e)
    //     }
    // }
}

#[get("/dial?<peer_id>")]
async fn dial(state: &State<AppState>, peer_id: String) -> String {
    let addr = Utils::get_address_through_relay(&peer_id.parse().unwrap(), None);
    let dial_resp = state.network_client.clone()
        .dial(peer_id.parse().unwrap(), addr)
        .await;

    match dial_resp {
        Ok(_) => "Dialled successfully".to_string(),
        Err(e) => format!("Dialing failed: {:?}", e)
    }
}

// File sharing
#[get("/provide-file?<file_path>")]
async fn provide_file(state: &State<AppState>, file_path: String) -> String {
    // Validate path and size
    let path = std::path::Path::new(file_path.as_str());
    if !path.exists() {
        return "Path does not exist".parse().unwrap();
    }

    if path.metadata().unwrap().len() > OrcaNetConfig::MAX_FILE_SIZE_BYTES {
        return format!("File size exceeds threshold of {} bytes", OrcaNetConfig::MAX_FILE_SIZE_BYTES);
    }

    // Compute hash
    let file_id = match Utils::sha256_digest(&path) {
        Ok(digest) => digest,
        _ => {
            return "Error computing digest".parse().unwrap();
        }
    };

    // Start providing
    let _ = state.event_sender.clone()
        .send(OrcaNetEvent::ProvideFile { file_id, file_path })
        .await;

    "Started providing".parse().unwrap()
}

#[get("/stop-providing?<file_id>")]
async fn stop_providing(state: &State<AppState>, file_id: String) -> String {
    let _ = state.event_sender.clone()
        .send(OrcaNetEvent::StopProvidingFile { file_id })
        .await;

    "Stopped".parse().unwrap()
}

#[get("/download-file?<file_id>")]
fn download_file(state: &State<AppState>, file_id: String) -> String {
    "".parse().unwrap()
}


pub async fn start_http_server(
    network_client: NetworkClient,
    event_sender: mpsc::Sender<OrcaNetEvent>,
) {
    rocket::build()
        .mount("/", routes![
            get_block_count,
            get_balance,
            load_wallet,
            send_to_address,
            generate_block,
            dial
        ])
        .manage(AppState {
            network_client,
            event_sender,
        })
        .launch()
        .await
        .expect("HTTP server should start");
}