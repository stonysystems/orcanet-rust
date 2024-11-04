use std::collections::HashSet;
use std::error::Error;

use futures::channel::{mpsc, oneshot};
use futures::SinkExt;
use libp2p::request_response::ResponseChannel;
use libp2p::{Multiaddr, PeerId};

use crate::common::{NetworkCommand, OrcaNetRequest, OrcaNetResponse, StreamData, StreamReq};
use crate::db::ProvidedFilesTable;
use crate::utils::Utils;

#[derive(Clone)]
pub struct NetworkClient {
    pub sender: mpsc::Sender<NetworkCommand>,
}

impl NetworkClient {
    /// Listen for incoming connections on the given address.
    pub async fn start_listening(&mut self, addr: Multiaddr) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(NetworkCommand::StartListening { addr, sender })
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
            .send(NetworkCommand::Dial {
                peer_id,
                peer_addr,
                sender,
            })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Advertise the local node as the provider of the given key on the DHT.
    pub async fn start_providing(&mut self, key: String) {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(NetworkCommand::StartProviding { key, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.");
    }

    /// Stop providing: Stop re-publishing provider record for given key
    pub async fn stop_providing(&mut self, key: String) {
        self.sender
            .send(NetworkCommand::StopProviding { key })
            .await
            .expect("Command receiver not to be dropped.");
    }

    /// Find the providers for the given key on the DHT.
    pub async fn get_providers(&mut self, key: String) -> HashSet<PeerId> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(NetworkCommand::GetProviders { key, sender })
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
            .send(NetworkCommand::PutKV { key, value, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not be dropped.")
    }

    /// Get the value for the given key from the DHT
    pub async fn get_value(&mut self, key: String) -> Result<Vec<u8>, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();

        self.sender
            .send(NetworkCommand::GetValue { key, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not be dropped.")
    }

    /// Download the given file from one of the providers (if any)
    pub async fn download_file(
        &mut self,
        file_id: String,
        dest_path: Option<String>,
    ) -> Result<(), Box<dyn Error>> {
        let providers = self.get_providers(file_id.clone()).await;
        tracing::info!("Got providers: {:?}", providers);

        if providers.is_empty() {
            return Err(format!("No peer provides {file_id}").into());
        }

        for peer_id in providers {
            let resp = self
                .download_file_from_peer(file_id.clone(), peer_id.clone(), dest_path.clone())
                .await;

            if resp.is_ok() {
                return Ok(());
            }
        }

        Err(format!("Could not get file from any provider for {file_id}").into())
    }

    pub async fn download_file_from_peer(
        &mut self,
        file_id: String,
        peer_id: PeerId,
        dest_path: Option<String>,
    ) -> Result<(), Box<dyn Error>> {
        let response = self
            .send_stream_request(
                peer_id.clone(),
                OrcaNetRequest::FileContentRequest { file_id },
            )
            .await;

        if let Ok(file_response) = response {
            tracing::info!("Got file from peer {:?}", peer_id);
            Utils::handle_file_content_response(peer_id, file_response, dest_path);

            return Ok(());
        }

        Err(format!("Could not get file from peer {peer_id}").into())
    }

    /// Send a request using streams and get response
    pub async fn send_stream_request(
        &mut self,
        peer_id: PeerId,
        orca_net_request: OrcaNetRequest,
    ) -> Result<OrcaNetResponse, Box<dyn Error>> {
        let addr = Utils::get_address_through_relay(&peer_id, None);
        let request_id = Utils::new_uuid();
        let stream_req = StreamReq {
            request_id: request_id.clone(),
            stream_data: StreamData::Request(orca_net_request),
        };

        let resp = self
            .send_in_stream(peer_id.clone(), addr.clone(), stream_req, true)
            .await;

        match resp {
            Some(response) => response.map_err(|e| e as Box<dyn Error>),
            None => Err(format!("No valid response from peer {peer_id}").into()),
        }
    }

    /// Send request to the given peer.
    pub async fn send_request(
        &mut self,
        peer: PeerId,
        request: OrcaNetRequest,
    ) -> Result<OrcaNetResponse, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(NetworkCommand::Request {
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
            .send(NetworkCommand::Respond { response, channel })
            .await
            .expect("Command receiver not to be dropped.");
    }

    /// Advertise all provided files to the network
    pub async fn advertise_provided_files(&mut self) {
        let mut provided_files_table = ProvidedFilesTable::new(None);

        match provided_files_table.get_provided_files() {
            Ok(provided_files) => {
                for file_info in provided_files {
                    self.start_providing(file_info.file_id).await;
                }
            }
            Err(e) => {
                tracing::error!("Error getting provided files {:?}", e);
            }
        }
    }

    /// Send stream req command
    pub async fn send_in_stream(
        &mut self,
        peer_id: PeerId,
        peer_addr: Multiaddr,
        stream_req: StreamReq,
        expect_response: bool,
    ) -> Option<Result<OrcaNetResponse, Box<dyn Error + Send>>> {
        // Dial to make sure that peer is reachable
        self.dial(peer_id.clone(), peer_addr).await.ok()?;

        if !expect_response {
            self.sender
                .send(NetworkCommand::SendInStream {
                    peer_id,
                    stream_req,
                    sender: None,
                })
                .await
                .expect("Command receiver not to be dropped.");

            None
        } else {
            let (sender, receiver) = oneshot::channel();

            self.sender
                .send(NetworkCommand::SendInStream {
                    peer_id,
                    stream_req,
                    sender: Some(sender),
                })
                .await
                .expect("Command receiver not to be dropped.");

            Some(receiver.await.expect("Sender not to be dropped"))
        }
    }
}
