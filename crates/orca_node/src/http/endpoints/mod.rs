use crate::common::OrcaNetEvent;
use crate::network_client::NetworkClient;
use async_std::stream::Extend;
pub use file_endpoints::get_file_endpoints;
use futures::channel::mpsc;
pub use proxy_endpoints::get_proxy_endpoints;
use rocket::serde::json::Json;
use rocket::serde::Serialize;
pub use wallet_endpoints::get_wallet_endpoints;

mod file_endpoints;
mod proxy_endpoints;
mod wallet_endpoints;

#[derive(Serialize)]
pub struct AppResponse {
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

pub struct AppState {
    pub network_client: NetworkClient,
    pub event_sender: mpsc::Sender<OrcaNetEvent>,
}
