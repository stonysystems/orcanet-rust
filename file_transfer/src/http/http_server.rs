use futures::channel::mpsc;

use crate::common::OrcaNetEvent;
use crate::http::endpoints::{
    get_file_endpoints, get_proxy_endpoints, get_wallet_endpoints, AppState,
};
use crate::network_client::NetworkClient;

// TODO: Change to use axum/actix-web instead of rocket later if there is time

pub async fn start_http_server(
    network_client: NetworkClient,
    event_sender: mpsc::Sender<OrcaNetEvent>,
) {
    rocket::build()
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
