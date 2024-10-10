use std::collections::HashMap;
use std::path::Path;

use futures::channel::mpsc;
use futures::StreamExt;
use tokio::select;

use crate::client::NetworkClient;
use crate::common::OrcaNetEvent;

pub struct RequestHandlerLoop {
    network_client: NetworkClient,
    event_receiver: mpsc::Receiver<OrcaNetEvent>,
    provided_files: HashMap<String, String>,
}

impl RequestHandlerLoop {
    pub fn new(network_client: NetworkClient, event_receiver: mpsc::Receiver<OrcaNetEvent>) -> Self {
        RequestHandlerLoop {
            network_client,
            event_receiver,
            provided_files: Default::default(),
        }
    }

    pub async fn run(mut self) {
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
                        std::fs::read(file_path).unwrap_or_else(|e| {
                            eprintln!("Couldn't read file: {:?}", e);
                            "Can't read it".as_bytes().into()
                        })
                    }
                    None => "Don't have it".as_bytes().into()
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
                }
            }
        }
    }
}