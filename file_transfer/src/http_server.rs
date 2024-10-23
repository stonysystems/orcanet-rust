use bitcoincore_rpc::RpcApi;
use futures::channel::mpsc;
use futures::SinkExt;
use ring::digest::digest;
use rocket::{get, routes, State};
use rocket::serde::{json::Json, Serialize};
use rocket::time::format_description::parse;
use serde::Deserialize;
use serde_json::json;
use tracing_subscriber::fmt::format;
use std::path::Path;
use libp2p_swarm::derive_prelude::PeerId;

use crate::btc_rpc::RPCWrapper;
use crate::common::{ConfigKey, OrcaNetConfig, OrcaNetEvent, Utils};
use crate::db_client::DBClient;
use crate::network_client::NetworkClient;

pub struct AppState {
    pub network_client: NetworkClient,
    pub event_sender: mpsc::Sender<OrcaNetEvent>,
}

// Request structs
#[derive(Serialize, Deserialize)]
struct SendToAddressRequest {
    address: String,
    amount: f64,
}

#[derive(Serialize, Deserialize)]
struct ProvideFileRequest {
    file_path: String,
}

#[derive(Serialize, Deserialize)]
struct DownloadFileRequest {
    file_id: String,
    peer_id: String,
    dest_path: String,
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

// TODO: All HTTP endpoints are currently GET. Some of them should be POST/PUT. Change later

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

#[post("/send-to-address", format = "application/json", data = "<request>")]
fn send_to_address(request: Json<SendToAddressRequest>) -> Json<Response> {
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    match rpc_wrapper.send_to_address(request.address.as_str(), request.amount) {
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

#[post("/generate-block")]
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

#[post("/dial/<peer_id_str>")]
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
#[get("/get-provided-files")]
async fn get_provided_files() -> Json<Response> {
    let db_client = DBClient::new(None);
    match db_client.get_provided_files() {
        Ok(files) => Response::success(json!(files)),
        Err(e) => Response::error(format!("Error getting files: {:?}", e))
    }
}

#[get("/get-file-info/<file_id>")]
async fn get_file_info(file_id: String) -> Json<Response> {
    let db_client = DBClient::new(None);
    match db_client.get_provided_file_info(file_id.as_str()) {
        Ok(file) => Response::success(json!(file)),
        Err(e) => Response::error(format!("Error getting file: {:?}", e))
    }
}

#[post("/provide-file", format = "application/json", data = "<request>")]
async fn provide_file(state: &State<AppState>, request: Json<ProvideFileRequest>) -> Json<Response> {
    let file_path = request.file_path.clone();

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

#[post("/stop-providing/<file_id>")]
async fn stop_providing(state: &State<AppState>, file_id: String) -> Json<Response> {
    let _ = state.event_sender.clone()
        .send(OrcaNetEvent::StopProvidingFile { file_id })
        .await;

    Response::success(json!("Stopped providing file"))
}

#[post("/download-file", format = "application/json", data = "<request>")]
async fn download_file(state: &State<AppState>, request: Json<DownloadFileRequest>) -> Json<Response> {
    // TODO: Add a check to make sure it's not already downloaded or provided
    let path = Path::new(&request.dest_path);

    if path.exists() {
        return Response::error("Path already exists".to_string());
    }

    let peer_id = match request.peer_id.parse() {
        Ok(id) => id,
        Err(e) => {
            return Response::error(format!("Error parsing peer_id: {:?}", e));
        }
    };

    match state.network_client.clone()
        .download_file_from_peer(request.file_id.clone(), peer_id, Some(request.dest_path.clone()))
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
            // Wallet
            get_block_count,
            get_balance,
            load_wallet,
            send_to_address,
            generate_block,
            dial,
            // File sharing
            get_provided_files,
            get_file_info,
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