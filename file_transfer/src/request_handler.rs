use std::path::Path;

use futures::channel::mpsc;
use futures::StreamExt;
use tokio::select;

use crate::common::{OrcaNetConfig, OrcaNetEvent, OrcaNetResponse};
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
            OrcaNetEvent::FileRequest { file_id, channel } => {
                println!("Received request for file_id: {}", file_id);
                // TODO: Add proper error handling
                let file_info = db_client.get_provided_file_info(file_id.as_str());
                println!("File info for request {} {:?}", file_id, file_info);

                let file_resp = match file_info {
                    Ok(file_info) => {
                        let path = Path::new(file_info.file_path.as_str());
                        match std::fs::read(path) {
                            Ok(content) => {
                                let _ = db_client.increment_download_count(file_id.as_str());

                                OrcaNetResponse::FileResponse {
                                    file_name: file_info.file_name,
                                    fee_rate_per_kb: OrcaNetConfig::get_fee_rate(),
                                    recipient_address: OrcaNetConfig::get_receiver_btc_address(),
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

                self.network_client
                    .respond(file_resp, channel)
                    .await;
            }
            OrcaNetEvent::ProvideFile { file_id, file_path } => {
                let path = Path::new(&file_path);
                let file_name = String::from(Path::new(file_path.as_str()).file_name()
                    .unwrap().to_str()
                    .unwrap());
                if path.exists() {
                    let resp = db_client.insert_provided_file(FileInfo {
                        file_id,
                        file_name,
                        file_path,
                        downloads_count: 0,
                    });

                    if resp.is_err() {
                        println!("Failed to insert into DB");
                    }
                }
            }
            OrcaNetEvent::StopProvidingFile { file_id } => {
                // TODO: Remove from table
            }
        }
    }
}