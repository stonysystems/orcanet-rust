use std::collections::HashMap;
use std::path::Path;

use futures::channel::mpsc;
use futures::StreamExt;
use tokio::select;

use crate::network_client::NetworkClient;
use crate::common::{FileResponse, OrcaNetEvent, Utils};

pub struct RequestHandlerLoop {
    network_client: NetworkClient,
    event_receiver: mpsc::Receiver<OrcaNetEvent>,
    provided_files: HashMap<String, String>,
    app_data_path: String,
}

impl RequestHandlerLoop {
    pub fn new(
        network_client: NetworkClient,
        event_receiver: mpsc::Receiver<OrcaNetEvent>,
        app_data_path: String,
    ) -> Self {
        RequestHandlerLoop {
            network_client,
            event_receiver,
            provided_files: Utils::load_provided_files(&app_data_path),
            app_data_path,
        }
    }

    pub async fn run(mut self) {
        // Start providing all configured files
        for file_id in self.provided_files.keys() {
            self.network_client.start_providing(file_id.clone()).await;
        }

        loop {
            select! {
                event = self.event_receiver.next() => match event {
                    Some(ev) => self.handle_event(ev).await,
                    e => {todo!("Not implemented")}
                }
            }
        }
    }

    async fn handle_event(&mut self, event: OrcaNetEvent) {
        match event {
            OrcaNetEvent::FileRequest { file_id, channel } => {
                println!("Received request for file_id: {}", file_id);
                // TODO: Add proper error handling

                let file_resp = match self.provided_files.get(&file_id) {
                    Some(file_path) => {
                        let path = Path::new(&file_path);
                        let file_name = String::from(path.file_name().unwrap().to_str().unwrap());
                        let content = std::fs::read(path).unwrap_or_else(|e| {
                            eprintln!("Couldn't read file: {:?}", e);
                            "Can't read it".as_bytes().into()
                        });

                        FileResponse {
                            file_name,
                            content,
                        }
                    }
                    None => FileResponse {
                        file_name: "no_name".to_string(),
                        content: "Can't find it".as_bytes().into(),
                    }
                };

                self.network_client
                    .respond_file(file_resp, channel)
                    .await;
            }
            OrcaNetEvent::ProvideFile { file_id, file_path } => {
                let path = Path::new(&file_path);
                if path.exists() {
                    println!("Added file {} to provided files list", file_id);
                    self.provided_files.insert(file_id, file_path);
                    Utils::dump_provided_files(&self.app_data_path, &self.provided_files);
                }
            }
        }
    }
}