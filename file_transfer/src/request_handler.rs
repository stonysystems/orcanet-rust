use std::path::Path;

use futures::{SinkExt, StreamExt};
use futures::channel::mpsc;
use tokio::select;

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
                let file_name = String::from(Path::new(file_path.as_str()).file_name()
                    .unwrap().to_str()
                    .unwrap());

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

                self.network_client.stop_providing(file_id).await;
            }
            OrcaNetEvent::StartProxyServer => {
                let (mut proxy_event_sender, mut proxy_event_receiver) = mpsc::channel::<OrcaNetEvent>(0);
                tokio::task::spawn(start_http_proxy(ProxyMode::ProxyProvider, proxy_event_receiver));
                self.proxy_event_sender = Some(proxy_event_sender);
            }
            OrcaNetEvent::StopProxyServer => {
                match self.proxy_event_sender.take() {
                    Some(mut proxy_event_sender) => {
                        if let Err(e) = proxy_event_sender
                            .send(OrcaNetEvent::StopProxyServer)
                            .await {
                            println!("Error sending stop command to proxy: {:?}", e);
                        }
                    }
                    None => {
                        println!("Proxy server not running");
                    }
                }
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