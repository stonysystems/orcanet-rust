use std::error::Error;
use std::path::Path;
use std::str::FromStr;

use bitcoin::Txid;
use bitcoincore_rpc::json::ListTransactionResult;
use bitcoincore_rpc::RpcApi;
use futures::channel::mpsc;
use futures::SinkExt;
use libp2p_swarm::derive_prelude::PeerId;
use ring::digest::digest;
use rocket::serde::{json::Json, Serialize};
use rocket::time::format_description::parse;
use rocket::{get, routes, State};
use serde::Deserialize;
use serde_json::json;
use tracing_subscriber::fmt::format;

use crate::btc_rpc::RPCWrapper;
use crate::common::{ConfigKey, OrcaNetConfig, OrcaNetEvent, OrcaNetRequest, OrcaNetResponse};
use crate::db::{DownloadedFileInfo, DownloadedFilesTable, ProvidedFilesTable};
use crate::network_client::NetworkClient;
use crate::utils::Utils;

pub struct AppState {
    pub network_client: NetworkClient,
    pub event_sender: mpsc::Sender<OrcaNetEvent>,
}

// Request structs
#[derive(Serialize, Deserialize, Debug)]
struct SendToAddressRequest {
    address: String,
    amount: f64,
    comment: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ProvideFileRequest {
    file_path: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct DownloadFileRequest {
    file_id: String,
    peer_id: String,
    dest_path: String,
}

#[derive(Serialize)]
struct AppResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

impl AppResponse {
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

// TODO: Change to use axum/actix-web instead of rocket later if there is time

// Wallet
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

#[get("/load-wallet/<wallet_name>")]
fn load_wallet(wallet_name: String) -> Json<AppResponse> {
    tracing::info!("Load wallet request for {}", wallet_name);
    let rpc_wrapper = RPCWrapper::new(OrcaNetConfig::get_network_type());

    match rpc_wrapper.get_client().load_wallet(wallet_name.as_str()) {
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

// File sharing
#[post("/dial/<peer_id_str>")]
async fn dial(state: &State<AppState>, peer_id_str: String) -> Json<AppResponse> {
    tracing::info!("Dial peer request: {}", peer_id_str);
    let peer_id = match peer_id_str.parse() {
        Ok(id) => id,
        Err(e) => {
            return AppResponse::error(format!("Error parsing peer_id: {:?}", e));
        }
    };

    let addr = Utils::get_address_through_relay(&peer_id, None);
    let dial_resp = state.network_client.clone().dial(peer_id, addr).await;

    match dial_resp {
        Ok(_) => AppResponse::success(json!("Dialled successfully")),
        Err(e) => AppResponse::error(format!("Dialing failed: {:?}", e)),
    }
}

#[get("/get-provided-files")]
async fn get_provided_files() -> Json<AppResponse> {
    let mut provided_files_table = ProvidedFilesTable::new(None);

    match provided_files_table.get_provided_files() {
        Ok(files) => AppResponse::success(json!(files)),
        Err(e) => AppResponse::error(format!("Error getting files: {:?}", e)),
    }
}

#[get("/get-downloaded-files")]
async fn get_downloaded_files() -> Json<AppResponse> {
    let mut downloaded_files_table = DownloadedFilesTable::new(None);

    match downloaded_files_table.get_downloaded_files() {
        Ok(files) => AppResponse::success(json!(files)),
        Err(e) => AppResponse::error(format!("Error getting files: {:?}", e)),
    }
}

#[get("/get-file-info/<file_id>")]
async fn get_file_info(file_id: String) -> Json<AppResponse> {
    tracing::info!("Get file info for : {}", file_id);
    let mut provided_files_table = ProvidedFilesTable::new(None);

    match provided_files_table.get_provided_file_info(file_id.as_str()) {
        Ok(file) => AppResponse::success(json!(file)),
        Err(e) => AppResponse::error(format!("Error getting file: {:?}", e)),
    }
}

#[post("/provide-file", format = "application/json", data = "<request>")]
async fn provide_file(
    state: &State<AppState>,
    request: Json<ProvideFileRequest>,
) -> Json<AppResponse> {
    tracing::info!("Provide file request: {:?}", request);
    let file_path = request.file_path.clone();

    // Validate path and size
    let path = Path::new(file_path.as_str());
    if !path.exists() {
        return AppResponse::error("Path does not exist".to_string());
    }

    if path.metadata().unwrap().len() > OrcaNetConfig::MAX_FILE_SIZE_BYTES {
        return AppResponse::error(format!(
            "File size exceeds threshold of {} bytes",
            OrcaNetConfig::MAX_FILE_SIZE_BYTES
        ));
    }

    // Compute hash
    let file_id = match Utils::sha256_digest(&path) {
        Ok(digest) => digest,
        _ => {
            return AppResponse::error("Error computing digest".to_string());
        }
    };

    // Start providing
    let _ = state
        .event_sender
        .clone()
        .send(OrcaNetEvent::ProvideFile {
            file_id: file_id.clone(),
            file_path,
        })
        .await;

    AppResponse::success(json!({
        "file_id": file_id
    }))
}

/// Stop providing a file to the network. Set permanent to true to remove from DB. Otherwise, it's set as inactive.
#[post("/stop-providing/<file_id>?<permanent>")]
async fn stop_providing(
    state: &State<AppState>,
    file_id: String,
    permanent: bool,
) -> Json<AppResponse> {
    tracing::info!("Stop providing request for: {}", file_id);
    let _ = state
        .event_sender
        .clone()
        .send(OrcaNetEvent::StopProvidingFile { file_id, permanent })
        .await;

    AppResponse::success(json!("Stopped providing file"))
}

#[post("/download-file", format = "application/json", data = "<request>")]
async fn download_file(
    state: &State<AppState>,
    request: Json<DownloadFileRequest>,
) -> Json<AppResponse> {
    // TODO: Add a check to make sure it's not already downloaded or provided
    tracing::info!("Download file request: {:?}", request);
    let path = Path::new(&request.dest_path);

    if path.exists() {
        return AppResponse::error("A file with the same name already exists in the given path. Provide a different name or path.".to_string());
    }

    // Validate that parent exists
    if path.parent().map_or(true, |v| !v.exists()) {
        return AppResponse::error(
            "Invalid path. Either it's not a file path or the parent directory does not exist"
                .to_string(),
        );
    }

    let peer_id = match request.peer_id.parse() {
        Ok(id) => id,
        Err(e) => {
            return AppResponse::error(format!("Error parsing peer_id: {:?}", e));
        }
    };

    match state
        .network_client
        .clone()
        .download_file_from_peer(
            request.file_id.clone(),
            peer_id,
            Some(request.dest_path.clone()),
        )
        .await
    {
        Ok(_) => AppResponse::success(json!("Downloaded file")),
        Err(e) => AppResponse::error(format!("Error downloading file: {:?}", e)),
    }
}

#[get("/get-providers/<file_id>")]
async fn get_providers(state: &State<AppState>, file_id: String) -> Json<AppResponse> {
    tracing::info!("Get providers request for: {}", file_id);
    let mut network_client = state.network_client.clone();
    let providers = network_client.get_providers(file_id.clone()).await;

    if providers.is_empty() {
        AppResponse::success(json!([]));
    }

    let responses = Utils::request_from_peers(
        OrcaNetRequest::FileMetadataRequest {
            file_id: file_id.clone(),
        },
        state.network_client.clone(),
        providers.iter(),
    )
    .await;

    let file_metadata_list = responses
        .iter()
        .filter_map(|item| {
            if let OrcaNetResponse::FileMetadataResponse(metadata) = item {
                Some(metadata)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    AppResponse::success(json!(file_metadata_list))
}

#[get("/get-proxy-providers")]
async fn get_proxy_providers(state: &State<AppState>) -> Json<AppResponse> {
    let mut network_client = state.network_client.clone();
    let proxy_providers = network_client
        .get_providers("proxy_providers".to_string())
        .await;

    if proxy_providers.is_empty() {
        return AppResponse::success(json!([]));
    }

    let responses = Utils::request_from_peers(
        OrcaNetRequest::HTTPProxyMetadataRequest,
        network_client,
        proxy_providers.iter(),
    )
    .await;

    let provider_metadata_list = responses
        .iter()
        .filter_map(|item| {
            if let OrcaNetResponse::HTTPProxyMetadataResponse(metadata) = item {
                Some(metadata)
            } else {
                // Ignore if not metadata response
                None
            }
        })
        .collect::<Vec<_>>();

    AppResponse::success(json!(provider_metadata_list))
}

pub async fn start_http_server(
    network_client: NetworkClient,
    event_sender: mpsc::Sender<OrcaNetEvent>,
) {
    rocket::build()
        .mount(
            "/api",
            routes![
                // Wallet
                get_block_count,
                get_balance,
                load_wallet,
                send_to_address,
                generate_block,
                list_transactions,
                get_transaction_info,
                // File sharing
                dial,
                get_provided_files,
                get_downloaded_files,
                get_file_info,
                provide_file,
                stop_providing,
                download_file,
                get_providers
            ],
        )
        .manage(AppState {
            network_client,
            event_sender,
        })
        .launch()
        .await
        .expect("HTTP server should start");
}
