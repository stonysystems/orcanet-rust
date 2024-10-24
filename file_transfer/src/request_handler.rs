use std::path::Path;

use futures::channel::mpsc;
use futures::StreamExt;
use tokio::select;

use crate::common::{ConfigKey, FileMetadata, OrcaNetConfig, OrcaNetEvent, OrcaNetRequest, OrcaNetResponse, Utils};
use crate::db_client::{DBClient, ProvidedFileInfo};
use crate::network_client::NetworkClient;

pub struct RequestHandlerLoop {
    network_client: NetworkClient,
    event_receiver: mpsc::Receiver<OrcaNetEvent>,
}

impl RequestHandlerLoop {
    pub fn new(
        network_client: NetworkClient,
        event_receiver: mpsc::Receiver<OrcaNetEvent>,
    ) -> Self {
        RequestHandlerLoop {
            network_client,
            event_receiver,
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
        let mut db_client = DBClient::new(None);

        match event {
            OrcaNetEvent::Request { request, channel } => {
                let response = self.handle_request(request, &mut db_client);
                self.network_client
                    .respond(response, channel)
                    .await;
            }
            OrcaNetEvent::StreamRequest { request, sender } => {
                let response = self.handle_request(request, &mut db_client);
                let _ = sender.send(response);
            }
            OrcaNetEvent::ProvideFile { file_id, file_path } => {
                let path = Path::new(&file_path);
                let file_name = String::from(Path::new(file_path.as_str()).file_name()
                    .unwrap().to_str()
                    .unwrap());

                if path.exists() {
                    let resp = db_client.insert_provided_file(ProvidedFileInfo {
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
                    let remove_resp = db_client.remove_provided_file(file_id.as_str());
                    if let Err(e) = remove_resp {
                        tracing::error!("Deletion failed for {}. Error: {:?}", file_id.as_str(), e)
                    }
                } else {
                    let status_change_resp = db_client.set_provided_file_status(
                        file_id.as_str(), false, None,
                    );
                    if let Err(e) = status_change_resp {
                        tracing::error!("Error changing status for {}. Error: {:?}", file_id.as_str(), e)
                    }
                }

                self.network_client.stop_providing(file_id).await;
            }
        }
    }

    fn handle_request(&mut self, request: OrcaNetRequest, db_client: &mut DBClient) -> OrcaNetResponse {
        match request {
            OrcaNetRequest::FileMetadataRequest { file_id } => {
                tracing::info!("Received metadata request for file_id: {}", file_id);

                match db_client.get_provided_file_info(file_id.as_str()) {
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
                        OrcaNetResponse::Error {
                            message: "File can't be provided".parse().unwrap()
                        }
                    }
                }
            }
            OrcaNetRequest::FileContentRequest { file_id } => {
                tracing::info!("Received content request for file_id: {}", file_id);

                let file_info = db_client.get_provided_file_info(file_id.as_str());
                tracing::info!("File info for request {} {:?}", file_id, file_info);

                let resp = match file_info {
                    Ok(file_info) => {
                        match std::fs::read(&file_info.file_path) {
                            Ok(content) => {
                                let _ = db_client.increment_download_count(file_id.as_str());

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
                                OrcaNetResponse::Error {
                                    message: "Error while reading file".parse().unwrap()
                                }
                            }
                        }
                    }
                    Err(_) => OrcaNetResponse::Error {
                        message: "File can't be provided".parse().unwrap()
                    }
                };

                resp
            }
            OrcaNetRequest::HTTPProxyRequest => {
                // Not providing
                OrcaNetResponse::HTTPProxyResponse(None)
            }
        }
    }
}