use crate::common::{OrcaNetRequest, OrcaNetResponse};
use crate::http::endpoints::{AppResponse, AppState};
use crate::utils::Utils;
use rocket::serde::json::Json;
use rocket::{Route, State};
use serde_json::json;

pub fn get_proxy_endpoints() -> Vec<Route> {
    routes![get_proxy_providers]
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
