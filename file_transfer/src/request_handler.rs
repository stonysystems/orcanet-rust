use std::path::Path;

use futures::{SinkExt, StreamExt};
use futures::channel::mpsc;
use serde_json::json;
use tokio::select;
use tracing_subscriber::filter::FilterExt;

use crate::common::{ConfigKey, FileMetadata, OrcaNetConfig, OrcaNetError, OrcaNetEvent, OrcaNetRequest, OrcaNetResponse, ProxyMode};
use crate::db::{ProvidedFileInfo, ProvidedFilesTable};
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
                self.network_client
                    .respond(response, channel)
                    .await;
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
                    let resp = provided_files_table
                        .insert_provided_file(ProvidedFileInfo {
                            file_id: file_id.clone(),
                            file_name,
                            file_path,
                            downloads_count: 0,
                            status: 1,
                            provide_start_timestamp: Some(Utils::get_unix_timestamp()),
                        });

                    match resp {
                        Ok(_) => {
                            self.network_client
                                .start_providing(file_id)
                                .await
                        }
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
                    let remove_resp = provided_files_table
                        .remove_provided_file(file_id.as_str());

                    if let Err(e) = remove_resp {
                        tracing::error!("Deletion failed for {}. Error: {:?}", file_id.as_str(), e)
                    }
                } else {
                    let mut provided_files_table = ProvidedFilesTable::new(None);
                    let status_change_resp = provided_files_table
                        .set_provided_file_status(file_id.as_str(), false, None);

                    if let Err(e) = status_change_resp {
                        tracing::error!("Error changing status for {}. Error: {:?}", file_id.as_str(), e)
                    }
                }

                self.network_client
                    .stop_providing(file_id)
                    .await;
            }
            OrcaNetEvent::ChangeProxyClient(client_config) => {
                // We restart to change proxy configuration
                // Technically, we only need to change the address and port in the running client
                // But that requires mutating it, and we need to start using Mutex locks for handler
                // Since changing is expected to be rare, I don't think it's worth introducing locks
                // TODO: Change later if required

                // About why Box::pin is needed: https://rust-lang.github.io/async-book/07_workarounds/04_recursion.html
                Box::pin(self.handle_event(OrcaNetEvent::StopProxy))
                    .await;
                Box::pin(self.handle_event(
                    OrcaNetEvent::StartProxy(ProxyMode::ProxyClient(client_config))
                )).await;
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
                ).expect("Proxy configuration to be updated");
            }
            OrcaNetEvent::StopProxy => {
                let proxy_mode = OrcaNetConfig::get_proxy_config()
                    .expect("Proxy mode to be present in config");

                Self::send_event_to_proxy_server(
                    OrcaNetEvent::StopProxy,
                    self.proxy_event_sender.take().as_mut(),
                ).await;

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
                ).expect("Proxy configuration to be updated");
            }
        }
    }

    async fn send_event_to_proxy_server(event: OrcaNetEvent, sender: Option<&mut mpsc::Sender<OrcaNetEvent>>) {
        // TODO: Return responses instead of just logging
        match sender {
            Some(proxy_event_sender) => {
                if let Err(e) = proxy_event_sender
                    .send(event)
                    .await {
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
            OrcaNetRequest::FileMetadataRequest { file_id } => {
                tracing::info!("Received metadata request for file_id: {}", file_id);
                let mut provided_files_table = ProvidedFilesTable::new(None);

                match provided_files_table.get_provided_file_info(file_id.as_str()) {
                    Ok(file_info) => {
                        OrcaNetResponse::FileMetadataResponse(FileMetadata {
                            file_id,
                            file_name: file_info.file_name,
                            fee_rate_per_kb: OrcaNetConfig::get_fee_rate(),
                            recipient_address: OrcaNetConfig::get_str_from_config(ConfigKey::BTCAddress),
                        })
                    }
                    Err(_) => {
                        tracing::error!("Requested file not found in DB");
                        OrcaNetResponse::Error(
                            OrcaNetError::NotAProvider("Not a provider of requested file".parse().unwrap())
                        )
                    }
                }
            }
            OrcaNetRequest::FileContentRequest { file_id } => {
                tracing::info!("Received content request for file_id: {}", file_id);

                let mut provided_files_table = ProvidedFilesTable::new(None);
                let file_info = provided_files_table
                    .get_provided_file_info(file_id.as_str());
                tracing::info!("File info for request {} {:?}", file_id, file_info);

                let resp = match file_info {
                    Ok(file_info) => {
                        match std::fs::read(&file_info.file_path) {
                            Ok(content) => {
                                let _ = provided_files_table
                                    .increment_download_count(file_id.as_str());

                                OrcaNetResponse::FileContentResponse {
                                    metadata: FileMetadata {
                                        file_id,
                                        file_name: file_info.file_name,
                                        fee_rate_per_kb: OrcaNetConfig::get_fee_rate(),
                                        recipient_address: OrcaNetConfig::get_str_from_config(ConfigKey::BTCAddress),
                                    },
                                    content,
                                }
                            }
                            Err(e) => {
                                tracing::error!("Error reading file: {:?}", e);
                                OrcaNetResponse::Error(
                                    OrcaNetError::FileProvideError("Error while reading file".parse().unwrap())
                                )
                            }
                        }
                    }
                    Err(_) => OrcaNetResponse::Error(
                        OrcaNetError::FileProvideError("File can't be provided".parse().unwrap())
                    )
                };

                resp
            }
            OrcaNetRequest::HTTPProxyMetadataRequest => {
                // Not providing
                OrcaNetResponse::Error(
                    OrcaNetError::NotAProvider("Not a proxy provider".parse().unwrap())
                )
            }
            OrcaNetRequest::HTTPProxyProvideRequest => {
                // Not providing
                OrcaNetResponse::Error(
                    OrcaNetError::NotAProvider("Not a proxy provider".parse().unwrap())
                )
            }
        }
    }
}