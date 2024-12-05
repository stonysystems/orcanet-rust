use std::error::Error;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;
use std::usize;

use clap::Parser;
use futures::channel::mpsc;
use futures::executor::block_on;
use futures::stream::StreamExt;
use libp2p::kad::store::{MemoryStore, MemoryStoreConfig};
use libp2p::request_response::ProtocolSupport;
use libp2p::{
    core::multiaddr::Protocol,
    core::Multiaddr,
    gossipsub, identify, identity, kad, noise, ping, relay, request_response,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, StreamProtocol,
};
use tokio::{io, io::AsyncBufReadExt, select};
use tracing_subscriber::EnvFilter;

const RELAY_ADDRESS: &str =
    "/ip4/130.245.173.221/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // Create the swarm to manage the network
    let opts = Opts::parse();
    let keypair = generate_ed25519(opts.secret_key_seed);
    let relay_address: Multiaddr = RELAY_ADDRESS.parse().unwrap();

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_dns()?
        .with_relay_client(noise::Config::new, yamux::Config::default)?
        .with_behaviour(|keypair, relay_behaviour| Behaviour {
            relay_client: relay_behaviour,
            ping: ping::Behaviour::new(ping::Config::new()),
            identify: identify::Behaviour::new(identify::Config::new(
                "/TODO/0.0.1".to_string(),
                keypair.public(),
            )),
            kademlia: kad::Behaviour::new(
                keypair.public().to_peer_id(),
                MemoryStore::with_config(
                    keypair.public().to_peer_id(),
                    MemoryStoreConfig {
                        max_records: 2 * 1000 * 1000,       // 2M
                        max_provided_keys: 2 * 1000 * 1000, // 2M
                        max_providers_per_key: 500,
                        max_value_bytes: 1 * 1024 * 1024, // 1 MB
                    },
                ),
            ),
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    // Set up listening on all interfaces
    swarm
        .listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap())
        .unwrap();
    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();

    // Make a reservation with relay
    swarm
        .listen_on(relay_address.clone().with(Protocol::P2pCircuit))
        .unwrap();

    // Set up kademlia props
    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(Some(kad::Mode::Server));

    block_on(async {
        loop {
            match swarm.next().await.expect("Infinite Stream.") {
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
                    info: identify::Info { observed_addr, .. },
                    ..
                })) => {
                    swarm.add_external_address(observed_addr.clone());
                }
                SwarmEvent::Behaviour(BehaviourEvent::RelayClient(
                    relay::client::Event::ReservationReqAccepted { .. },
                )) => {
                    tracing::info!("Relay accepted our reservation request");
                }
                SwarmEvent::Behaviour(BehaviourEvent::RelayClient(event)) => {
                    tracing::info!(?event)
                }
                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    tracing::info!(peer=%peer_id, ?endpoint, "Established new connection");
                }
                SwarmEvent::NewListenAddr { address, .. } => {
                    tracing::info!("Listening on {address:?}");
                }
                _ => {}
            }
        }
    })
}

fn generate_ed25519(secret_key_seed: u64) -> identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0..8].copy_from_slice(&secret_key_seed.to_le_bytes());

    identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
}

#[derive(Debug, Parser)]
#[clap(name = "Bootstrap node")]
struct Opts {
    /// Fixed value to generate deterministic peer id
    #[clap(long)]
    secret_key_seed: u64,
}
