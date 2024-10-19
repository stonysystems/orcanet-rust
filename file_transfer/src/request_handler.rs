use std::path::Path;

use futures::channel::mpsc;
use futures::StreamExt;
use tokio::select;

use crate::common::{ConfigKey, OrcaNetConfig, OrcaNetEvent, OrcaNetRequest, OrcaNetResponse};
use crate::db_client::{DBClient, FileInfo};
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
        let db_client = DBClient::new(None);

        match event {
            OrcaNetEvent::Request { request, channel } => {
                let response = self.handle_request(request, &db_client);
                self.network_client
                    .respond(response, channel)
                    .await;
            }
            OrcaNetEvent::ProvideFile { file_id, file_path } => {
                let path = Path::new(&file_path);
                let file_name = String::from(Path::new(file_path.as_str()).file_name()
                    .unwrap().to_str()
                    .unwrap());

                if path.exists() {
                    let resp = db_client.insert_provided_file(FileInfo {
                        file_id: file_id.clone(),
                        file_name,
                        file_path,
                        downloads_count: 0,
                    });

                    if resp.is_err() {
                        eprintln!("Failed to insert into DB");
                    }
                }

                self.network_client.start_providing(file_id).await;
            }
            OrcaNetEvent::StopProvidingFile { file_id } => {
                db_client.remove_provided_file(file_id.as_str()).unwrap_or_else(
                    |_| eprintln!("Deletion failed for {}", file_id.as_str())
                );
                self.network_client.stop_providing(file_id).await;
            }
        }
    }

    fn handle_request(&mut self, request: OrcaNetRequest, db_client: &DBClient) -> OrcaNetResponse {
        match request {
            OrcaNetRequest::FileRequest { file_id } => {
                println!("Received request for file_id: {}", file_id);
                // TODO: Add proper error handling
                let file_info = db_client.get_provided_file_info(file_id.as_str());
                println!("File info for request {} {:?}", file_id, file_info);

                let resp = match file_info {
                    Ok(file_info) => {
                        let path = Path::new(file_info.file_path.as_str());

                        match std::fs::read(path) {
                            Ok(content) => {
                                let _ = db_client.increment_download_count(file_id.as_str());
                                let recipient_address = OrcaNetConfig::get_str_from_config(ConfigKey::BTCAddress);

                                OrcaNetResponse::FileResponse {
                                    file_id,
                                    file_name: file_info.file_name,
                                    fee_rate_per_kb: OrcaNetConfig::get_fee_rate(),
                                    recipient_address,
                                    content,
                                }
                            }
                            Err(e) => {
                                eprintln!("Error reading file: {:?}", e);
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
        }
    }
}