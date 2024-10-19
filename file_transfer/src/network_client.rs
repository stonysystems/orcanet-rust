use std::collections::HashSet;
use std::error::Error;

use futures::channel::{mpsc, oneshot};
use futures::SinkExt;
use libp2p::{Multiaddr, PeerId};
use libp2p::request_response::ResponseChannel;

use crate::common::{OrcaNetCommand, OrcaNetRequest, OrcaNetResponse, Utils};
use crate::db_client::DBClient;

#[derive(Clone)]
pub struct NetworkClient {
    pub sender: mpsc::Sender<OrcaNetCommand>,
}

impl NetworkClient {
    /// Listen for incoming connections on the given address.
    pub async fn start_listening(
        &mut self,
        addr: Multiaddr,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(OrcaNetCommand::StartListening { addr, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Dial the given peer at the given address.
    pub async fn dial(
        &mut self,
        peer_id: PeerId,
        peer_addr: Multiaddr,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(OrcaNetCommand::Dial {
                peer_id,
                peer_addr,
                sender,
            })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Advertise the local node as the provider of the given file on the DHT.
    pub async fn start_providing(&mut self, file_id: String) {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(OrcaNetCommand::StartProviding { file_id, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.");
    }

    /// Stop providing: Stop re-publishing provider record for given file_id
    pub async fn stop_providing(&mut self, file_id: String) {
        self.sender
            .send(OrcaNetCommand::StopProviding { file_id })
            .await
            .expect("Command receiver not to be dropped.");
    }

    /// Find the providers for the given file on the DHT.
    pub async fn get_providers(&mut self, file_id: String) -> HashSet<PeerId> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(OrcaNetCommand::GetProviders { file_id, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Put the given KV pair to the DHT
    pub async fn put_kv_pair(
        &mut self,
        key: String,
        value: Vec<u8>,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();

        self.sender
            .send(OrcaNetCommand::PutKV {
                key,
                value,
                sender,
            })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not be dropped.")
    }

    /// Get the value for the given key from the DHT
    pub async fn get_value(
        &mut self,
        key: String,
    ) -> Result<Vec<u8>, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();

        self.sender
            .send(OrcaNetCommand::GetValue {
                key,
                sender,
            })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not be dropped.")
    }

    /// Download the given file from one of the providers (if any)
    pub async fn download_file(
        &mut self,
        file_id: String,
    ) -> Result<(), Box<dyn Error>> {
        let file_id_with_ns = Utils::get_key_with_ns(file_id.as_str());
        let providers = self.get_providers(file_id_with_ns).await;

        println!("Got providers: {:?}", providers);

        if providers.is_empty() {
            return Err(format!("No peer provides {file_id}").into());
        }

        for peer in providers {
            let addr = Utils::get_address_through_relay(&peer, None);
            if self.dial(peer.clone(), addr).await.is_err() {
                // Peer not available, skip it
                continue;
            }

            if let Ok(resp) = self.send_request(
                peer.clone(),
                OrcaNetRequest::FileRequest { file_id: file_id.clone() },
            ).await {
                Utils::handle_file_response(resp);
                return Ok(());
            }
        }

        // // Request the content of the file from each node.
        // // TODO: Convert this to series of requests based on dial success because we can't ask everyone
        // // Then we'll have to pay everyone
        // let requests = providers.into_iter().map(|peer| {
        //     let mut network_client = self.clone();
        //     let name = file_id.clone();
        //     async move { network_client.send_request(peer, name).await }.boxed()
        // });
        //
        // // Await the requests, ignore the remaining once a single one succeeds.
        // let file_response: OrcaNetResponse = futures::future::select_ok(requests)
        //     .await
        //     .map_err(|_| "None of the providers returned file.")?
        //     .0;

        // Utils::handle_file_response(file_response);
        // Ok(())

        Err(format!("Could not get file from any provider for {file_id}").into())
    }

    /// Send request to the given peer.
    pub async fn send_request(
        &mut self,
        peer: PeerId,
        request: OrcaNetRequest,
    ) -> Result<OrcaNetResponse, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(OrcaNetCommand::Request {
                request,
                peer,
                sender,
            })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not be dropped.")
    }

    /// Send response through the given channel
    pub async fn respond(
        &mut self,
        response: OrcaNetResponse,
        channel: ResponseChannel<OrcaNetResponse>,
    ) {
        self.sender
            .send(OrcaNetCommand::Respond { response, channel })
            .await
            .expect("Command receiver not to be dropped.");
    }

    /// Advertise all provided files to the network
    pub async fn advertise_provided_files(&mut self) {
        let db_client = DBClient::new(None);

        match db_client.get_provided_files() {
            Ok(provided_files) => {
                for file_info in provided_files {
                    let key = Utils::get_key_with_ns(file_info.file_id.as_str());
                    self.start_providing(key).await;
                }
            }
            Err(e) => {
                eprintln!("Error getting provided files {:?}", e);
            }
        }
    }
}