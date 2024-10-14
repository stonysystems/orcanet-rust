use bitcoincore_rpc::RpcApi;
use rocket::{get, routes};

use crate::btc_rpc::RPCWrapper;
use crate::common::{ConfigKey, OrcaNetConfig, Utils};

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
async fn send_to_address(address: String, amt: f64) -> String {
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

pub async fn start_http_server() {
    rocket::build()
        .mount("/", routes![
            get_block_count,
            get_balance,
            load_wallet,
            send_to_address,
            generate_block
        ])
        .launch()
        .await
        .expect("HTTP server should start");
}