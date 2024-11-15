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

pub fn get_proxy_endpoints() -> Vec<Route> {
    routes![get_providers]
}

#[derive(Serialize, Deserialize, Debug)]
struct ConnectToProxyRequest {
    peer_id: PeerId,
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
        Some(ProxyMode::ProxyProvider) => {
            todo!()
        }
        Some(ProxyMode::ProxyClient { session_id }) => {
            // Settle up remaining balances
            // Inform the server about session end
            todo!()
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
        return AppResponse::error("Proxy already running. Stop it first.".to_string());
    }

    // Send connect request to peer
    let mut network_client = state.network_client.clone();

    let proxy_session = match network_client
        .send_request(
            request.peer_id.clone(),
            OrcaNetRequest::HTTPProxyProvideRequest,
        )
        .await
    {
        Ok(OrcaNetResponse::HTTPProxyProvideResponse {
            metadata,
            auth_token,
            client_id,
        }) => {
            // Persist data in DB
            let mut proxy_sessions_table = ProxySessionsTable::new(None);
            let session_id = Utils::new_uuid();
            let proxy_session_info = ProxySessionInfo {
                session_id,
                client_id,
                auth_token,
                start_timestamp: Utils::get_unix_timestamp(),
                proxy_address: metadata.proxy_address,
                fee_rate_per_kb: metadata.fee_rate_per_kb,
                provider_peer_id: request.peer_id.to_string(),
                recipient_address: metadata.recipient_address,
                ..Default::default()
            };

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
            session_id: proxy_session.session_id,
        }))
        .await;

    todo!()
}
