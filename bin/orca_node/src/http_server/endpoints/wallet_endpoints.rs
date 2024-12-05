use crate::common::btc_rpc::RPCWrapper;
use crate::common::config::OrcaNetConfig;
use crate::common::Utils;
use crate::http_server::endpoints::AppResponse;
use bitcoin::Txid;
use bitcoincore_rpc::RpcApi;
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use rocket::{get, post, routes, Route, State};
use serde_json::json;

pub fn get_wallet_endpoints() -> Vec<Route> {
    routes![
        get_block_count,
        get_balance,
        load_wallet,
        send_to_address,
        generate_block,
        list_transactions,
        get_transaction_info
    ]
}

#[derive(Serialize, Deserialize, Debug)]
struct SendToAddressRequest {
    address: String,
    amount: f64,
    comment: Option<String>,
}

#[get("/blocks-count")]
fn get_block_count() -> Json<AppResponse> {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    match rpc_wrapper.get_client().get_block_count() {
        Ok(count) => AppResponse::success(json!(count)),
        Err(e) => AppResponse::error(format!("Error getting block count {:?}", e)),
    }
}

#[get("/balance")]
fn get_balance() -> Json<AppResponse> {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    match rpc_wrapper.get_client().get_balance(None, None) {
        Ok(balance) => AppResponse::success(json!(balance.to_string())),
        Err(e) => AppResponse::error(format!("Error getting balance {:?}", e)),
    }
}

#[get("/load/<wallet_name>")]
fn load_wallet(wallet_name: String) -> Json<AppResponse> {
    tracing::info!("Load wallet request for {}", wallet_name);
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    match rpc_wrapper.load_wallet(wallet_name.as_str()) {
        Ok(_) => AppResponse::success(json!("Wallet loaded")),
        Err(e) => AppResponse::error(format!("Error loading wallet {:?}", e)),
    }
}

#[post("/send-to-address", format = "application/json", data = "<request>")]
fn send_to_address(request: Json<SendToAddressRequest>) -> Json<AppResponse> {
    tracing::info!("Send to address request: {:?}", request);
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());
    let comment = request.comment.as_deref();

    match rpc_wrapper.send_to_address(request.address.as_str(), request.amount, comment) {
        Ok(tx_id) => AppResponse::success(json!({
            "tx_id": tx_id
        })),
        Err(e) => AppResponse::error(format!("Failed to send: {:?}", e)),
    }
}

#[post("/generate-block")]
async fn generate_block() -> Json<AppResponse> {
    Utils::start_async_block_gen().await;

    AppResponse::success(json!("Started block generation"))

    // match rpc_wrapper.generate_to_address(address.as_str()) {
    //     Ok(blockhash) => {
    //         format!("Created transaction {:?}", blockhash)
    //     }
    //     Err(e) => {
    //         format!("Failed to generate: {:?}", e)
    //     }
    // }
}

#[get("/list-transactions?<start_offset>&<end_offset>")]
fn list_transactions(start_offset: usize, end_offset: usize) -> Json<AppResponse> {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    match rpc_wrapper.get_client().list_transactions(
        None,
        Some(end_offset - start_offset),
        Some(start_offset - 1),
        None,
    ) {
        Ok(mut transactions) => {
            transactions.sort_by(|a, b| b.info.time.cmp(&a.info.time));
            AppResponse::success(json!(transactions))
        }
        Err(e) => AppResponse::error(format!("Error fetching transactions {:?}", e)),
    }
}

#[get("/get-transaction-info/<tx_id>")]
fn get_transaction_info(tx_id: String) -> Json<AppResponse> {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());
    let tx_id_val: Txid = match tx_id.parse() {
        Ok(tx_id) => tx_id,
        Err(e) => {
            return AppResponse::error(format!("Error parsing tx_id: {:?}", e));
        }
    };

    match rpc_wrapper.get_client().get_transaction(&tx_id_val, None) {
        Ok(transaction) => AppResponse::success(json!(transaction)),
        Err(e) => AppResponse::error(format!("Error fetching transactions {:?}", e)),
    }
}
