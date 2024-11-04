use std::path::Path;

use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};
use serde_json::json;
use tokio::select;
use tracing_subscriber::filter::FilterExt;

use crate::common::{
    ConfigKey, FileMetadata, HTTPProxyMetadata, OrcaNetConfig, OrcaNetError, OrcaNetEvent,
    OrcaNetRequest, OrcaNetResponse, ProxyMode,
};
use crate::db::{ProvidedFileInfo, ProvidedFilesTable, ProxyClientInfo, ProxyClientsTable};
use crate::http::start_http_proxy;
use crate::network_client::NetworkClient;
use crate::utils::Utils;

pub struct RequestHandlerLoop {
    network_client: NetworkClient,
    event_receiver: mpsc::Receiver<OrcaNetEvent>,
    proxy_event_sender: Option<mpsc::Sender<OrcaNetEvent>>,
}

impl RequestHandlerLoop {
    pub fn new(
        network_client: NetworkClient,
        event_receiver: mpsc::Receiver<OrcaNetEvent>,
    ) -> Self {
        RequestHandlerLoop {
            network_client,
            event_receiver,
            proxy_event_sender: None,
        }
    }

    pub async fn run(mut self) {
        // Start providing all configured files
        // TODO

        loop {
            select! {
                event = self.event_receiver.next() => match event {
                    Some(ev) => self.handle_event(ev).await,
                    _ => {todo!("Not implemented")}
                }
            }
        }
    }

    async fn handle_event(&mut self, event: OrcaNetEvent) {
        match event {
            OrcaNetEvent::Request { request, channel } => {
                let response = self.handle_request(request);
                self.network_client.respond(response, channel).await;
            }
            OrcaNetEvent::StreamRequest { request, sender } => {
                let response = self.handle_request(request);
                let _ = sender.send(response);
            }
            OrcaNetEvent::ProvideFile { file_id, file_path } => {
                let path = Path::new(&file_path);
                let file_name: String = Utils::get_file_name_from_path(&path);

                if path.exists() {
                    let mut provided_files_table = ProvidedFilesTable::new(None);
                    let file_info =
                        ProvidedFileInfo::with_defaults(file_id.clone(), file_path, file_name);
                    let resp = provided_files_table.insert_provided_file(&file_info);

                    match resp {
                        Ok(_) => self.network_client.start_providing(file_id).await,
                        Err(e) => {
                            tracing::error!("Failed to insert into DB: {:?}", e);
                        }
                    }
                }
            }
            OrcaNetEvent::StopProvidingFile { file_id, permanent } => {
                if permanent {
                    // Permanently stop providing - Remove from DB
                    let mut provided_files_table = ProvidedFilesTable::new(None);
                    let remove_resp = provided_files_table.remove_provided_file(file_id.as_str());

                    if let Err(e) = remove_resp {
                        tracing::error!("Deletion failed for {}. Error: {:?}", file_id.as_str(), e)
                    }
                } else {
                    let mut provided_files_table = ProvidedFilesTable::new(None);
                    let status_change_resp = provided_files_table.set_provided_file_status(
                        file_id.as_str(),
                        false,
                        None,
                    );

                    if let Err(e) = status_change_resp {
                        tracing::error!(
                            "Error changing status for {}. Error: {:?}",
                            file_id.as_str(),
                            e
                        )
                    }
                }

                self.network_client.stop_providing(file_id).await;
            }
            OrcaNetEvent::ChangeProxyClient(client_config) => {
                // We restart to change proxy configuration
                // Technically, we only need to change the address and port in the running client
                // But that requires mutating it, and we need to start using Mutex locks for handler
                // Since changing is expected to be rare, I don't think it's worth introducing locks
                // TODO: Change later if required

                // About why Box::pin is needed: https://rust-lang.github.io/async-book/07_workarounds/04_recursion.html
                Box::pin(self.handle_event(OrcaNetEvent::StopProxy)).await;
                Box::pin(
                    self.handle_event(OrcaNetEvent::StartProxy(ProxyMode::ProxyClient(
                        client_config,
                    ))),
                )
                .await;
            }
            OrcaNetEvent::StartProxy(proxy_mode) => {
                if self.proxy_event_sender.is_some() {
                    println!("Proxy server already running");
                    return;
                }

                let (proxy_event_sender, proxy_event_receiver) = mpsc::channel::<OrcaNetEvent>(0);
                tokio::task::spawn(start_http_proxy(proxy_mode.clone(), proxy_event_receiver));
                self.proxy_event_sender = Some(proxy_event_sender);

                match &proxy_mode {
                    ProxyMode::ProxyProvider => {
                        tracing::info!("Started proxy provider. Putting provider record");
                        self.network_client
                            .start_providing(OrcaNetConfig::PROXY_PROVIDER_KEY_DHT.to_string())
                            .await;
                    }
                    ProxyMode::ProxyClient(config) => {
                        tracing::info!("Started proxy client with config: {:?}", config);
                    }
                }

                OrcaNetConfig::modify_config(
                    ConfigKey::ProxyConfig.to_string().as_str(),
                    json!(Some(proxy_mode)),
                )
                .expect("Proxy configuration to be updated");
            }
            OrcaNetEvent::StopProxy => {
                let proxy_mode =
                    OrcaNetConfig::get_proxy_config().expect("Proxy mode to be present in config");

                Self::send_event_to_proxy_server(
                    OrcaNetEvent::StopProxy,
                    self.proxy_event_sender.take().as_mut(),
                )
                .await;

                match proxy_mode {
                    ProxyMode::ProxyProvider => {
                        self.network_client
                            .stop_providing(OrcaNetConfig::PROXY_PROVIDER_KEY_DHT.to_string())
                            .await;
                    }
                    ProxyMode::ProxyClient(config) => {
                        // TODO: Pay remaining amount owed
                    }
                }

                OrcaNetConfig::modify_config(
                    ConfigKey::ProxyConfig.to_string().as_str(),
                    json!(None::<ProxyMode>),
                )
                .expect("Proxy configuration to be updated");
            }
        }
    }

    async fn send_event_to_proxy_server(
        event: OrcaNetEvent,
        sender: Option<&mut mpsc::Sender<OrcaNetEvent>>,
    ) {
        // TODO: Return responses instead of just logging
        match sender {
            Some(proxy_event_sender) => {
                if let Err(e) = proxy_event_sender.send(event).await {
                    tracing::error!("Error sending command to proxy: {:?}", e);
                }
            }
            None => {
                tracing::error!("Send event to proxy: Proxy server not running");
            }
        }
    }

    fn handle_request(&mut self, request: OrcaNetRequest) -> OrcaNetResponse {
        match request {
            OrcaNetRequest::FileMetadataRequest { .. }
            | OrcaNetRequest::FileContentRequest { .. } => Self::handle_file_request(request),
            OrcaNetRequest::HTTPProxyMetadataRequest | OrcaNetRequest::HTTPProxyProvideRequest => {
                Self::handle_http_proxy_request(request)
            }
        }
    }

    fn handle_http_proxy_request(request: OrcaNetRequest) -> OrcaNetResponse {
        match OrcaNetConfig::get_proxy_config() {
            Some(ProxyMode::ProxyProvider) => {
                let metadata = HTTPProxyMetadata {
                    proxy_address: "".to_string(),
                    fee_rate_per_kb: OrcaNetConfig::get_proxy_fee_rate(),
                    recipient_address: OrcaNetConfig::get_str_from_config(ConfigKey::BTCAddress),
                };

                match request {
                    OrcaNetRequest::HTTPProxyMetadataRequest => {
                        OrcaNetResponse::HTTPProxyMetadataResponse(metadata)
                    }
                    OrcaNetRequest::HTTPProxyProvideRequest => {
                        // Create new client
                        let client_id = Utils::new_uuid();
                        let auth_token = Utils::new_uuid();

                        let mut proxy_clients_table = ProxyClientsTable::new(None);
                        let proxy_client_info =
                            ProxyClientInfo::with_defaults(client_id.clone(), auth_token.clone());

                        match proxy_clients_table.add_client(&proxy_client_info) {
                            Ok(_) => {
                                tracing::info!("Created new client: {:?}", proxy_client_info);
                                OrcaNetResponse::HTTPProxyProvideResponse {
                                    metadata,
                                    client_id,
                                    auth_token,
                                }
                            }
                            Err(_) => OrcaNetResponse::Error(OrcaNetError::InternalServerError(
                                "Error creating client".to_string(),
                            )),
                        }
                    }
                    _ => panic!("Expected only proxy requests in handle_proxy_requests"),
                }
            }
            _ => {
                // Not providing
                OrcaNetResponse::Error(OrcaNetError::NotAProvider(
                    "Not a proxy provider".to_string(),
                ))
            }
        }
    }

    fn handle_file_request(request: OrcaNetRequest) -> OrcaNetResponse {
        let file_id = match &request {
            OrcaNetRequest::FileMetadataRequest { file_id } => file_id,
            OrcaNetRequest::FileContentRequest { file_id } => file_id,
            _ => panic!("Expected file request"),
        };

        let mut provided_files_table = ProvidedFilesTable::new(None);
        let file_info_resp = provided_files_table.get_provided_file_info(file_id.as_str());

        if file_info_resp.is_err() {
            // Most likely not present in DB
            tracing::error!("Requested file not found in DB");
            return OrcaNetResponse::Error(OrcaNetError::NotAProvider(
                "Not a provider of requested file".to_string(),
            ));
        }

        let file_info = file_info_resp.unwrap();
        tracing::info!("File info for request {} {:?}", file_id, file_info);

        let file_metadata = FileMetadata {
            file_id: file_id.clone(),
            file_name: file_info.file_name,
            fee_rate_per_kb: OrcaNetConfig::get_file_fee_rate(),
            recipient_address: OrcaNetConfig::get_str_from_config(ConfigKey::BTCAddress),
        };

        match request {
            OrcaNetRequest::FileMetadataRequest { .. } => {
                tracing::info!("Received metadata request for file_id: {}", file_id);

                OrcaNetResponse::FileMetadataResponse(file_metadata)
            }
            OrcaNetRequest::FileContentRequest { .. } => {
                tracing::info!("Received content request for file_id: {}", file_id);

                match std::fs::read(&file_info.file_path) {
                    Ok(content) => {
                        let _ = provided_files_table.increment_download_count(file_id.as_str());

                        OrcaNetResponse::FileContentResponse {
                            metadata: file_metadata,
                            content,
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error reading file: {:?}", e);
                        OrcaNetResponse::Error(OrcaNetError::FileProvideError(
                            "Error while reading file".to_string(),
                        ))
                    }
                }
            }
            _ => panic!("Expected file request"),
        }
    }
}
