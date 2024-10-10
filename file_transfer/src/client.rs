use std::collections::HashSet;
use std::error::Error;
use std::io::Write;

use futures::{FutureExt, SinkExt};
use futures::channel::{mpsc, oneshot};
use libp2p::{Multiaddr, PeerId};
use libp2p::request_response::ResponseChannel;

use crate::common::{FileResponse, OrcaNetCommand};

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

    /// Find the providers for the given file on the DHT.
    pub async fn get_providers(&mut self, file_id: String) -> HashSet<PeerId> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(OrcaNetCommand::GetProviders { file_id, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Request the content of the given file from the given peer.
    pub async fn request_file(
        &mut self,
        peer: PeerId,
        file_id: String,
    ) -> Result<FileResponse, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(OrcaNetCommand::RequestFile {
                file_id,
                peer,
                sender,
            })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not be dropped.")
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

    // pub async fn get_file(
    //     &mut self,
    //     file_id: String,
    // ) -> Result<(), dyn Error> {
    //     let providers = self.get_providers(file_id.clone()).await;
    //     if providers.is_empty() {
    //         return Err(format!("Could not find provider for file {name}.").into());
    //     }
    //
    //     // Request the content of the file from each node.
    //     let requests = providers.into_iter().map(|p| {
    //         let mut network_client = self.clone();
    //         let name = file_id.clone();
    //         async move { network_client.request_file(p, name).await }.boxed()
    //     });
    //
    //     // Await the requests, ignore the remaining once a single one succeeds.
    //     let file_content = futures::future::select_ok(requests)
    //         .await
    //         .map_err(|_| "None of the providers returned file.")?
    //         .0;
    //
    //     std::io::stdout().write_all(&file_content)?;
    //     Ok(())
    // }

    /// Respond with the provided file content to the given request.
    pub async fn respond_file(
        &mut self,
        file_resp: FileResponse,
        channel: ResponseChannel<FileResponse>,
    ) {
        self.sender
            .send( OrcaNetCommand::RespondFile { file_resp, channel })
            .await
            .expect("Command receiver not to be dropped.");
    }
}