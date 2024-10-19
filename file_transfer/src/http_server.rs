use bitcoincore_rpc::RpcApi;
use futures::channel::mpsc;
use futures::SinkExt;
use ring::digest::digest;
use rocket::{get, routes, State};
use rocket::serde::{json::Json, Serialize};
use rocket::time::format_description::parse;
use serde_json::json;
use tracing_subscriber::fmt::format;

use crate::btc_rpc::RPCWrapper;
use crate::common::{ConfigKey, OrcaNetConfig, OrcaNetEvent, Utils};
use crate::network_client::NetworkClient;

pub struct AppState {
    pub network_client: NetworkClient,
    pub event_sender: mpsc::Sender<OrcaNetEvent>,
}

#[derive(Serialize)]
struct Response {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

impl Response {
    pub fn success(data: serde_json::Value) -> Json<Self> {
        Json(Self {
            success: true,
            error: None,
            data: Some(data),
        })
    }

    pub fn error(error: String) -> Json<Self> {
        Json(Self {
            success: false,
            error: Some(error),
            data: None,
        })
    }
}

// Wallet
#[get("/blocks-count")]
fn get_block_count() -> Json<Response> {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    match rpc_wrapper.get_client()
        .get_block_count() {
        Ok(count) => Response::success(json!(count)),
        Err(e) => {
            Response::error(format!("Error getting block count {:?}", e))
        }
    }
}

#[get("/balance")]
fn get_balance() -> Json<Response> {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    match rpc_wrapper.get_client()
        .get_balance(None, None) {
        Ok(balance) => Response::success(json!(balance.to_string())),
        Err(e) => {
            Response::error(format!("Error getting balance {:?}", e))
        }
    }
}

#[get("/load-wallet/<wallet_name>")]
fn load_wallet(wallet_name: String) -> Json<Response> {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    match rpc_wrapper.get_client().load_wallet(wallet_name.as_str()) {
        Ok(_) => Response::success(json!("Wallet loaded")),
        Err(e) => {
            Response::error(format!("Error loading wallet {:?}", e))
        }
    }
}

#[get("/send-to-address/<address>?<amt>")]
fn send_to_address(address: String, amt: f64) -> Json<Response> {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    match rpc_wrapper.send_to_address(address.as_str(), amt) {
        Ok(tx_id) => {
            Response::success(json!({
                "tx_id": tx_id
            }))
        }
        Err(e) => {
            Response::error(format!("Failed to send: {:?}", e))
        }
    }
}

#[get("/generate-block")]
async fn generate_block() -> Json<Response> {
    Utils::start_async_block_gen().await;

    Response::success(json!("Started block generation"))

    // match rpc_wrapper.generate_to_address(address.as_str()) {
    //     Ok(blockhash) => {
    //         format!("Created transaction {:?}", blockhash)
    //     }
    //     Err(e) => {
    //         format!("Failed to generate: {:?}", e)
    //     }
    // }
}

#[get("/dial/<peer_id_str>")]
async fn dial(state: &State<AppState>, peer_id_str: String) -> Json<Response> {
    let peer_id = match peer_id_str.parse() {
        Ok(id) => id,
        Err(e) => {
            return Response::error(format!("Error parsing peer_id: {:?}", e));
        }
    };

    let addr = Utils::get_address_through_relay(&peer_id, None);
    let dial_resp = state.network_client.clone()
        .dial(peer_id, addr)
        .await;

    match dial_resp {
        Ok(_) => Response::success(json!("Dialled successfully")),
        Err(e) => Response::error(format!("Dialing failed: {:?}", e))
    }
}

// File sharing
#[get("/provide-file?<file_path>")]
async fn provide_file(state: &State<AppState>, file_path: String) -> Json<Response> {
    // Validate path and size
    let path = std::path::Path::new(file_path.as_str());
    if !path.exists() {
        return Response::error("Path does not exist".to_string());
    }

    if path.metadata().unwrap().len() > OrcaNetConfig::MAX_FILE_SIZE_BYTES {
        return Response::error(format!("File size exceeds threshold of {} bytes", OrcaNetConfig::MAX_FILE_SIZE_BYTES));
    }

    // Compute hash
    let file_id = match Utils::sha256_digest(&path) {
        Ok(digest) => digest,
        _ => {
            return Response::error("Error computing digest".to_string());
        }
    };

    // Start providing
    let _ = state.event_sender.clone()
        .send(OrcaNetEvent::ProvideFile { file_id, file_path })
        .await;

    Response::success(json!("Started providing file"))
}

#[get("/stop-providing/<file_id>")]
async fn stop_providing(state: &State<AppState>, file_id: String) -> Json<Response> {
    let _ = state.event_sender.clone()
        .send(OrcaNetEvent::StopProvidingFile { file_id })
        .await;

    Response::success(json!("Stopped providing file"))
}

#[get("/download-file/<file_id>")]
async fn download_file(state: &State<AppState>, file_id: String) -> Json<Response> {
    match state.network_client.clone()
        .download_file(file_id)
        .await {
        Ok(_) => Response::success(json!("Downloaded file")),
        Err(e) => Response::error(format!("Error downloading file: {:?}", e))
    }
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
            dial,
            provide_file,
            stop_providing,
            download_file
        ])
        .manage(AppState {
            network_client,
            event_sender,
        })
        .launch()
        .await
        .expect("HTTP server should start");
}