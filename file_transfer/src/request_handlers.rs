use futures::channel::mpsc;
use futures::StreamExt;
use tokio::select;

use crate::client::NetworkClient;
use crate::common::OrcaNetEvent;

pub struct RequestHandlerLoop {
    network_client: NetworkClient,
    event_receiver: mpsc::Receiver<OrcaNetEvent>,
}

impl RequestHandlerLoop {
    pub fn new(network_client: NetworkClient, event_receiver: mpsc::Receiver<OrcaNetEvent>) -> Self {
        RequestHandlerLoop {
            network_client,
            event_receiver,
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
                self.network_client
                    .respond_file("Wow bro".as_bytes().into(), channel)
                    .await;
            }
        }
    }
}