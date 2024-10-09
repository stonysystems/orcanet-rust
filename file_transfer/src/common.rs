use std::collections::HashSet;
use std::error::Error;
use futures::channel::oneshot;
use libp2p::{identity, Multiaddr, PeerId};
use libp2p::multiaddr::Protocol;

pub(crate) struct OrcaNetConfig;

impl OrcaNetConfig {
    pub const NAMESPACE: &'static str = "/orcanet";
    pub const STREAM_PROTOCOL: &'static str = "/orcanet/p2p";
    pub const SECRET_KEY_SEED: u64 = 4;

    pub fn get_bootstrap_peer_id() -> PeerId {
        "12D3KooWQd1K1k8XA9xVEzSAu7HUCodC7LJB6uW5Kw4VwkRdstPE"
            .parse().unwrap()
    }

    pub fn get_bootstrap_address() -> Multiaddr {
        "/ip4/130.245.173.222/tcp/61000/p2p/12D3KooWQd1K1k8XA9xVEzSAu7HUCodC7LJB6uW5Kw4VwkRdstPE"
            .parse().unwrap()
    }

    pub fn get_relay_peer_id() -> PeerId {
        "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            .parse().unwrap()
    }

    pub fn get_relay_address() -> Multiaddr {
        "/ip4/130.245.173.221/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            .parse().unwrap()
    }
}

#[derive(Debug)]
pub(crate) enum OrcaNetCommand {
    StartListening {
        addr: Multiaddr,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    Dial {
        peer_id: PeerId,
        peer_addr: Multiaddr,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    StartProviding {
        file_id: String,
        sender: oneshot::Sender<()>,
    },
    GetProviders {
        file_id: String,
        sender: oneshot::Sender<HashSet<PeerId>>,
    },
    PutKV {
        key: String,
        value: Vec<u8>,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    GetValue {
        key: String,
        sender: oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>,
    },
    RequestFile {
        file_id: String,
        peer: PeerId,
        sender: oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>,
    },
}

pub(crate) struct Utils;

impl Utils {
    pub fn get_address_through_relay(peer_id: &PeerId, relay_address_override: Option<Multiaddr>) -> Multiaddr {
        let relay_address = relay_address_override.unwrap_or(OrcaNetConfig::get_relay_address());
        relay_address.clone()
            .with(Protocol::P2pCircuit)
            .with(Protocol::P2p(peer_id.clone()))
    }

    pub fn generate_ed25519(secret_key_seed: u64) -> identity::Keypair {
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&secret_key_seed.to_le_bytes());

        identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
    }

    pub fn get_key_with_ns(key: &str) -> String {
        format!("{}/{}", OrcaNetConfig::NAMESPACE, key)
    }
}

