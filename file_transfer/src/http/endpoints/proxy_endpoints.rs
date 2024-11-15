use crate::common::{
    OrcaNetConfig, OrcaNetEvent, OrcaNetRequest, OrcaNetResponse, ProxyClientConfig, ProxyMode,
};
use crate::db::{ProxySessionInfo, ProxySessionsTable};
use crate::http::endpoints::{AppResponse, AppState};
use crate::utils::Utils;

use futures::SinkExt;
use libp2p::PeerId;
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use rocket::{Route, State};
use serde_json::json;
use tracing::metadata;

pub fn get_proxy_endpoints() -> Vec<Route> {
    routes![
        get_providers,
        stop_proxy,
        connect,
        start_client,
        start_providing
    ]
}

#[derive(Serialize, Deserialize, Debug)]
struct ConnectToProxyRequest {
    peer_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct StartLocalProxyRequest {
    session_id: String,
}

#[get("/get-providers")]
async fn get_providers(state: &State<AppState>) -> Json<AppResponse> {
    let mut network_client = state.network_client.clone();

    // Get proxy providers
    let proxy_providers = network_client
        .get_providers(OrcaNetConfig::PROXY_PROVIDER_KEY_DHT.to_string())
        .await;

    if proxy_providers.is_empty() {
        return AppResponse::success(json!([]));
    }

    // Ask them for metadata
    let responses = Utils::request_from_peers(
        OrcaNetRequest::HTTPProxyMetadataRequest,
        network_client,
        proxy_providers.into_iter(),
    )
    .await;

    let provider_metadata_list = responses
        .iter()
        .filter_map(|(peer_id, resp)| {
            if let OrcaNetResponse::HTTPProxyMetadataResponse(metadata) = resp {
                Some(json!({
                    "peer_id": peer_id.to_string(),
                    "proxy_metadata": metadata
                }))
            } else {
                // Ignore if not metadata response
                None
            }
        })
        .collect::<Vec<_>>();

    AppResponse::success(json!(provider_metadata_list))
}

#[post("/stop")]
async fn stop_proxy(state: &State<AppState>) -> Json<AppResponse> {
    match OrcaNetConfig::get_proxy_config() {
        Some(_) => {
            state
                .event_sender
                .clone()
                .send(OrcaNetEvent::StopProxy)
                .await
                .expect("Sender not to be dropped");

            AppResponse::success(json!("proxy stopped"))
        }
        None => AppResponse::error("Proxy not running".to_string()),
    }
}

#[post("/connect", format = "application/json", data = "<request>")]
async fn connect(
    state: &State<AppState>,
    request: Json<ConnectToProxyRequest>,
) -> Json<AppResponse> {
    // Check if proxy is already active
    if OrcaNetConfig::get_proxy_config().is_some() {
        // Current proxy session should be stopped before connecting to new proxy
        // Let's not do that automatically in here. Connect is only to initiate a new session.
        return AppResponse::error(
            "Proxy already running. Stop it before starting a new client session.".to_string(),
        );
    }

    // Send connect request to peer
    let peer_id: PeerId = match request.peer_id.parse() {
        Ok(peer_id) => peer_id,
        Err(e) => {
            return AppResponse::error("Invalid peer_id".to_string());
        }
    };
    let mut network_client = state.network_client.clone();

    let provide_resp = network_client
        .send_request(peer_id.clone(), OrcaNetRequest::HTTPProxyProvideRequest)
        .await;

    // Persist session info in DB
    let proxy_session = match provide_resp {
        Ok(OrcaNetResponse::HTTPProxyProvideResponse {
            metadata,
            auth_token,
            client_id,
        }) => {
            let mut proxy_sessions_table = ProxySessionsTable::new(None);
            let proxy_session_info = ProxySessionInfo::from_proxy_connect_response(
                request.peer_id.clone(),
                client_id,
                auth_token,
                metadata,
            );

            proxy_sessions_table
                .insert_session_info(&proxy_session_info)
                .expect("Session info to be added to DB");

            proxy_session_info
        }
        Ok(_) => {
            tracing::error!("Received wrong response for proxy provide request");
            return AppResponse::error("Couldn't connect to proxy".to_string());
        }
        Err(e) => {
            tracing::error!("Error sending proxy provide request to proxy: {:?}", e);
            return AppResponse::error("Couldn't connect to proxy".to_string());
        }
    };

    // Start local proxy server
    let mut event_sender = state.event_sender.clone();
    let _ = event_sender
        .send(OrcaNetEvent::StartProxy(ProxyMode::ProxyClient {
            session_id: proxy_session.session_id.clone(),
        }))
        .await;

    AppResponse::success(json!({
        "session_id": proxy_session.session_id
    }))
}

// For testing only
#[post("/start-client", format = "application/json", data = "<request>")]
async fn start_client(
    state: &State<AppState>,
    request: Json<StartLocalProxyRequest>,
) -> Json<AppResponse> {
    // Check if proxy is already active
    if OrcaNetConfig::get_proxy_config().is_some() {
        return AppResponse::error("Proxy already running".to_string());
    }

    let mut event_sender = state.event_sender.clone();
    let _ = event_sender
        .send(OrcaNetEvent::StartProxy(ProxyMode::ProxyClient {
            session_id: request.session_id.clone(),
        }))
        .await;

    AppResponse::success(json!("Started local proxy client"))
}

#[post("/start-providing")]
async fn start_providing(state: &State<AppState>) -> Json<AppResponse> {
    // Check if proxy is already active
    if OrcaNetConfig::get_proxy_config().is_some() {
        return AppResponse::error("Proxy already running".to_string());
    }

    let mut event_sender = state.event_sender.clone();
    let _ = event_sender
        .send(OrcaNetEvent::StartProxy(ProxyMode::ProxyProvider))
        .await;

    AppResponse::success(json!("Started providing proxy"))
}
