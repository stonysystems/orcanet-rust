use crate::common::types::OrcaNetEvent;
use crate::http_server::endpoints::{
    get_file_endpoints, get_proxy_endpoints, get_wallet_endpoints, AppState,
};
use crate::network::NetworkClient;
use diesel::sql_types::Json;
use futures::channel::mpsc;
use rocket::serde::json::json;
use serde_json::Value;

// TODO: Change to use axum/actix-web instead of rocket later if there is time

#[get("/health")]
fn health_check() -> Value {
    json!({
        "success": true
    })
}

pub async fn start_http_server(
    network_client: NetworkClient,
    event_sender: mpsc::Sender<OrcaNetEvent>,
) {
    rocket::build()
        .mount("/api/", routes![health_check])
        .mount("/api/wallet", get_wallet_endpoints())
        .mount("/api/file", get_file_endpoints())
        .mount("/api/proxy", get_proxy_endpoints())
        .manage(AppState {
            network_client,
            event_sender,
        })
        .launch()
        .await
        .expect("HTTP server should start");
}
