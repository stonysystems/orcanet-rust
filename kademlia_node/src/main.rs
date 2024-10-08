mod request_handlers;

use std::{error::Error, time::Duration};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use clap::Parser;
use futures::{AsyncReadExt, executor::block_on, future::FutureExt, stream::StreamExt};
use libp2p::{core::multiaddr::{Multiaddr, Protocol}, identify, identity, kad, noise, PeerId, ping, relay, swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux};
use libp2p::kad::store::{MemoryStore, MemoryStoreConfig};
use libp2p::StreamProtocol;
use tokio::{io, select};
use tokio::io::AsyncBufReadExt;
use tracing_subscriber::EnvFilter;

struct Config;
impl Config {
    pub const NAMESPACE: &'static str = "/orcanet";
    pub const STREAM_PROTOCOL: &'static str = "/orcanet/p2p";
    pub const SECRET_KEY_SEED: u64 = 4;

    pub fn get_bootstrap_peer_id() -> PeerId {
        PeerId::from_str("12D3KooWQd1K1k8XA9xVEzSAu7HUCodC7LJB6uW5Kw4VwkRdstPE").unwrap()
    }

    pub fn get_relay_address() -> Multiaddr {
        "/ip4/130.245.173.221/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN".parse().unwrap()
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    stream: libp2p_stream::Behaviour,
}

fn get_address_through_relay(relay_address: &Multiaddr, peer_id: &PeerId) -> Multiaddr {
    relay_address.clone()
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(peer_id.clone()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let relay_address = Config::get_relay_address();
    let bootstrap_peer_id = Config::get_bootstrap_peer_id();
    let boostrap_addr = get_address_through_relay(&relay_address, &bootstrap_peer_id);

    let mut swarm =
        libp2p::SwarmBuilder::with_existing_identity(generate_ed25519(Config::SECRET_KEY_SEED))
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
                    MemoryStore::with_config(keypair.public().to_peer_id(), MemoryStoreConfig {
                        max_records: 2 * 1000 * 1000, // 2M
                        max_provided_keys: 2 * 1000 * 1000, // 2M
                        max_providers_per_key: 500,
                        max_value_bytes: 1 * 1024 * 1024, // 1 MB
                    }),
                ),
                stream: libp2p_stream::Behaviour::new(),
            })?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

    swarm
        .listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap())
        .unwrap();
    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();

    // Wait to listen on all interfaces.
    block_on(async {
        let mut delay = futures_timer::Delay::new(std::time::Duration::from_secs(1)).fuse();
        loop {
            futures::select! {
                event = swarm.next() => {
                    match event.unwrap() {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            tracing::info!(%address, "Listening on address");
                        }
                        event => panic!("{event:?}"),
                    }
                }
                _ = delay => {
                    // Likely listening on all interfaces now, thus continuing by breaking the loop.
                    break;
                }
            }
        }
    });

    // Make a reservation with relay
    swarm.listen_on(relay_address.clone().with(Protocol::P2pCircuit)).unwrap();

    // Set up kademlia props
    swarm.behaviour_mut().kademlia.set_mode(Some(kad::Mode::Client));
    swarm.behaviour_mut().kademlia.add_address(&bootstrap_peer_id, boostrap_addr.clone());

    // Dial the bootstrap node
    swarm.dial(boostrap_addr.clone()).unwrap();

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines();
    let mut control = swarm.behaviour().stream.new_control();
    let mut incoming = control.accept(StreamProtocol::new(Config::STREAM_PROTOCOL)).unwrap();

    block_on(async {
        loop {
            select! {
                Ok(Some(line)) = stdin.next_line() => {
                    handle_input_line(&mut swarm.behaviour_mut().kademlia, line);
                }

                stream_event = incoming.next() => {
                    if let Some((peer_id, mut stream)) = stream_event {
                        println!("Received stream from Peer {:?}", peer_id);

                        let mut buffer = Vec::new();
                        stream.read_to_end(&mut buffer).await?;

                        match String::from_utf8(buffer) {
                            Ok(str) => {
                                let json: serde_json::Value = serde_json::from_str(str.as_str())?;
                                println!("Got json {:?}", json);
                                if let Some(known_peers) = json.get("known_peers") {
                                    for v in known_peers.as_array().unwrap() {
                                        let peer_id_str = v.get("peer_id").unwrap().as_str().unwrap();
                                        let known_peer_id = PeerId::from_str(peer_id_str).unwrap();
                                        let peer_addr = get_address_through_relay(
                                                &relay_address,
                                                &known_peer_id);

                                        // TODO: Check if this is fine
                                        if let Ok(_) = swarm.dial(peer_addr.clone()) {
                                            println!("Adding {:?} to Kademlia", known_peer_id);
                                            swarm.behaviour_mut().kademlia.add_address(&known_peer_id, peer_addr.clone());
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                println!("Error while parsing stream data into UTF8 {:?}", e);
                            }
                        }
                    }
                }

                event = swarm.select_next_some() => match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        tracing::info!(%address, "Listening on address");
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
                        // TODO: Add condition check to ignore relay node connection events
                        tracing::info!(peer=%peer_id, ?endpoint, "Established new connection");
                        let peer_relay_addr = get_address_through_relay(&relay_address, &peer_id);
                        swarm.behaviour_mut().kademlia.add_address(&peer_id, peer_relay_addr);
                    }
                    SwarmEvent::Behaviour(BehaviourEvent::Kademlia(kad::Event::OutboundQueryProgressed { result, .. })) => {
                        match result {
                            kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders { key, providers, .. })) => {
                                for peer in providers {
                                    println!(
                                        "Peer {peer:?} provides key {:?}",
                                        std::str::from_utf8(key.as_ref()).unwrap()
                                    );
                                }
                            }
                            kad::QueryResult::GetProviders(Err(err)) => {
                                eprintln!("Failed to get providers: {err:?}");
                            }
                            kad::QueryResult::GetRecord(Ok(
                                                            kad::GetRecordOk::FoundRecord(kad::PeerRecord {
                                                                                              record: kad::Record { key, value, .. },
                                                                                              ..
                                                                                          })
                                                        )) => {
                                println!(
                                    "Got record {:?} {:?}",
                                    std::str::from_utf8(key.as_ref()).unwrap(),
                                    std::str::from_utf8(&value).unwrap(),
                                );
                            }
                            kad::QueryResult::GetRecord(Ok(_)) => {}
                            kad::QueryResult::GetRecord(Err(err)) => {
                                eprintln!("Failed to get record: {err:?}");
                            }
                            kad::QueryResult::PutRecord(Ok(kad::PutRecordOk { key })) => {
                                println!(
                                    "Successfully put record {:?}",
                                    std::str::from_utf8(key.as_ref()).unwrap()
                                );
                            }
                            kad::QueryResult::PutRecord(Err(err)) => {
                                eprintln!("Failed to put record: {err:?}");
                            }
                            kad::QueryResult::StartProviding(Ok(kad::AddProviderOk { key })) => {
                                println!(
                                    "Successfully put provider record {:?}",
                                    std::str::from_utf8(key.as_ref()).unwrap()
                                );
                            }
                            kad::QueryResult::StartProviding(Err(err)) => {
                                eprintln!("Failed to put provider record: {err:?}");
                            }
                            _ => {}
                        }
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        tracing::info!(peer=?peer_id, "Outgoing connection failed: {error}");
                    }
                    _ => {}
                }
            }
        }
    })
}

fn get_key_with_ns(key: &str) -> String {
    format!("{}/{}", Config::NAMESPACE, key)
}

fn handle_input_line(kademlia: &mut kad::Behaviour<MemoryStore>, line: String) {
    let mut args = line.split(' ');
    let command = args.next();

    if command.is_none() {
        return;
    }

    let key = match args.next() {
        Some(key) => {
            let key_with_ns = get_key_with_ns(key);
            kad::RecordKey::new(&key_with_ns.as_str())
        }
        None => {
            eprintln!("Expected key");
            return;
        }
    };

    match command {
        Some("get") => {
            kademlia.get_record(key);
        }
        Some("get_providers") => {
            kademlia.get_providers(key);
        }
        Some("put") => {
            let value = match args.next() {
                Some(value) => value.as_bytes().to_vec(),
                None => {
                    eprintln!("Expected value");
                    return;
                }
            };
            let record = kad::Record {
                key,
                value,
                publisher: None,
                expires: None,
            };
            kademlia
                .put_record(record, kad::Quorum::One)
                .expect("Failed to store record locally.");
        }
        Some("put_provider") => {
            kademlia
                .start_providing(key)
                .expect("Failed to start providing key");
        }
        _ => {
            eprintln!("expected get, get_providers, put or put_provider");
        }
    }
}

fn generate_ed25519(secret_key_seed: u64) -> identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0..8].copy_from_slice(&secret_key_seed.to_le_bytes());

    identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
}
