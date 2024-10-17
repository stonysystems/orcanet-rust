use bitcoincore_rpc::RpcApi;
use rocket::{get, routes, State};

use crate::btc_rpc::RPCWrapper;
use crate::common::{ConfigKey, OrcaNetConfig, Utils};
use crate::network_client::NetworkClient;

pub struct AppState {
    pub network_client: NetworkClient,
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
    let dial_resp = state.network_client
        .clone()
        .dial(peer_id.parse().unwrap(), addr)
        .await;

    match dial_resp {
        Ok(_) => "Dialled successfully".to_string(),
        Err(e) => format!("Dialing failed: {:?}", e)
    }
}

// File sharing
#[get("/provide-file?<file_path>")]
fn provide_file(state: &State<AppState>, file_path: String) -> String {
    // Compute hash
    // Start providing
    "".parse().unwrap()
}


pub async fn start_http_server(network_client: NetworkClient) {
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
            network_client
        })
        .launch()
        .await
        .expect("HTTP server should start");
}