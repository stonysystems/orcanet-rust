use crate::common::config::OrcaNetConfig;
use crate::common::types::{OrcaNetEvent, OrcaNetRequest, OrcaNetResponse};
use crate::common::Utils;
use crate::db::{DownloadedFilesTable, ProvidedFilesTable};
use crate::http_server::endpoints::{AppResponse, AppState};
use futures::SinkExt;
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use rocket::{Route, State};
use serde_json::json;
use std::path::Path;

pub fn get_file_endpoints() -> Vec<Route> {
    routes![
        dial,
        get_provided_files,
        get_downloaded_files,
        get_file_info,
        provide_file,
        stop_providing,
        resume_providing,
        download_file,
        get_providers
    ]
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

/// Resume providing the given file. It must already be in the provided files table.
#[post("/resume-providing/<file_id>")]
async fn resume_providing(state: &State<AppState>, file_id: String) -> Json<AppResponse> {
    let mut provided_files_table = ProvidedFilesTable::new(None);

    match provided_files_table.set_provided_file_status(file_id.as_str(), true, None) {
        Ok(rows) => {
            if rows == 0 {
                return AppResponse::error(format!("File id {} is not provided", file_id));
            }
            state.network_client.clone().start_providing(file_id).await;
            AppResponse::success(json!("Resumed providing file"))
        }
        Err(e) => {
            // File might not be provided already
            AppResponse::error(format!("Failed to resume providing file: {:?}", e))
        }
    }
}

#[post("/download-file", format = "application/json", data = "<request>")]
async fn download_file(
    state: &State<AppState>,
    request: Json<DownloadFileRequest>,
) -> Json<AppResponse> {
    // TODO: Add a check to make sure it's not already downloaded or provided
    // TODO: Add a check to make sure we have enough funds to get the file
    // TODO: Start providing after download
    tracing::info!("Download file request: {:?}", request);
    let path = Path::new(&request.dest_path);

    if path.exists() {
        return AppResponse::error("A file with the same name already exists in the given path. Provide a different name or path.".to_string());
    }

    // Validate the path
    if path.is_dir() || path.parent().is_some_and(|v| !v.exists()) {
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

    // Download
    let download_resp = state
        .network_client
        .clone()
        .download_file_from_peer(
            request.file_id.clone(),
            peer_id,
            Some(request.dest_path.clone()),
        )
        .await;

    match download_resp {
        Ok(_) => AppResponse::success(json!("Downloaded file")),
        Err(e) => AppResponse::error(format!("Error downloading file: {:?}", e)),
    }
}

#[get("/get-providers/<file_id>")]
async fn get_providers(state: &State<AppState>, file_id: String) -> Json<AppResponse> {
    tracing::info!("Get providers request for: {}", file_id);
    let mut network_client = state.network_client.clone();

    // Get providers for the file
    let providers = network_client.get_providers(file_id.clone()).await;

    if providers.is_empty() {
        AppResponse::success(json!([]));
    }

    // Ask them for metadata
    let responses = Utils::request_from_peers(
        OrcaNetRequest::FileMetadataRequest {
            file_id: file_id.clone(),
        },
        state.network_client.clone(),
        providers.into_iter(),
    )
    .await;

    let file_metadata_list = responses
        .iter()
        .filter_map(|(peer_id, resp)| {
            if let OrcaNetResponse::FileMetadataResponse(metadata) = resp {
                Some(json!({
                    "peer_id": peer_id.to_string(),
                    "file_metadata": metadata
                }))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    AppResponse::success(json!(file_metadata_list))
}
