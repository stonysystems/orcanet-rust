use std::collections::HashSet;
use std::error::Error;

use futures::channel::{mpsc, oneshot};
use futures::SinkExt;
use libp2p::{Multiaddr, PeerId};
use crate::common::OrcaNetCommand;


struct FileResponse(Vec<u8>);

#[derive(Clone)]
pub(crate) struct NetworkClient {
    pub(crate) sender: mpsc::Sender<OrcaNetCommand>,
}

impl NetworkClient {
    /// Listen for incoming connections on the given address.
    pub(crate) async fn start_listening(
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
    pub(crate) async fn dial(
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
    pub(crate) async fn start_providing(&mut self, file_id: String) {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(OrcaNetCommand::StartProviding { file_id, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.");
    }

    /// Find the providers for the given file on the DHT.
    pub(crate) async fn get_providers(&mut self, file_id: String) -> HashSet<PeerId> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(OrcaNetCommand::GetProviders { file_id, sender })
            .await
            .expect("Command receiver not to be dropped.");
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Request the content of the given file from the given peer.
    pub(crate) async fn request_file(
        &mut self,
        peer: PeerId,
        file_id: String,
    ) -> Result<Vec<u8>, Box<dyn Error + Send>> {
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

    /// Request the content of the given file from the given peer.
    pub(crate) async fn put_kv_pair(
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

    /// Request the content of the given file from the given peer.
    pub(crate) async fn get_value(
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

    // /// Respond with the provided file content to the given request.
    // pub(crate) async fn respond_file(
    //     &mut self,
    //     file: Vec<u8>,
    //     channel: ResponseChannel<FileResponse>,
    // ) {
    //     self.sender
    //         .send(Command::RespondFile { file, channel })
    //         .await
    //         .expect("Command receiver not to be dropped.");
    // }
}