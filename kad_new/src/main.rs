use std::{error::Error, time::Duration};
use std::str::FromStr;

use clap::Parser;
use futures::{AsyncReadExt, executor::block_on, future::FutureExt, stream::StreamExt};
use libp2p::{core::multiaddr::{Multiaddr, Protocol}, dcutr, identify, identity, kad, noise, PeerId, ping, relay, swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux};
use libp2p::kad::store::MemoryStore;
use libp2p::StreamProtocol;
use tokio::{io, select};
use tokio::io::AsyncBufReadExt;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[clap(name = "libp2p Kademlia client")]
struct Opts {
    /// The mode (client-listen, client-dial).
    #[clap(long)]
    mode: Mode,

    /// Fixed value to generate deterministic peer id.
    #[clap(long)]
    secret_key_seed: u8,

    /// The listening address
    #[clap(long)]
    relay_address: Multiaddr,

    /// Peer ID of the remote peer to hole punch to.
    #[clap(long)]
    remote_peer_id: Option<PeerId>,
}

#[derive(Clone, Debug, PartialEq, Parser)]
enum Mode {
    Dial,
    Listen,
}

impl FromStr for Mode {
    type Err = String;
    fn from_str(mode: &str) -> Result<Self, Self::Err> {
        match mode {
            "dial" => Ok(Mode::Dial),
            "listen" => Ok(Mode::Listen),
            _ => Err("Expected either 'dial' or 'listen'".to_string()),
        }
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    ping: ping::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    identify: identify::Behaviour,
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

    let opts = Opts::parse();

    let mut swarm =
        libp2p::SwarmBuilder::with_existing_identity(generate_ed25519(opts.secret_key_seed))
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
                kademlia: kad::Behaviour::new(
                    keypair.public().to_peer_id(),
                    MemoryStore::new(keypair.public().to_peer_id()),
                ),
                relay_client: relay_behaviour,
                ping: ping::Behaviour::new(ping::Config::new()),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/TODO/0.0.1".to_string(),
                    keypair.public(),
                )),
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

    // Connect to the relay server. Not for the reservation or relayed connection, but to (a) learn
    // our local public address and (b) enable a freshly started relay to learn its public address.
    swarm.dial(opts.relay_address.clone()).unwrap();
    block_on(async {
        let mut learned_observed_addr = false;
        let mut told_relay_observed_addr = false;

        loop {
            match swarm.next().await.unwrap() {
                SwarmEvent::NewListenAddr { .. } => {}
                SwarmEvent::Dialing { .. } => {}
                SwarmEvent::ConnectionEstablished { .. } => {}
                SwarmEvent::Behaviour(BehaviourEvent::Ping(_)) => {}
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Sent {
                                                                   ..
                                                               })) => {
                    tracing::info!("Told relay its public address");
                    told_relay_observed_addr = true;
                }
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
                                                                   info: identify::Info { observed_addr, .. },
                                                                   ..
                                                               })) => {
                    tracing::info!(address=%observed_addr, "Relay told us our observed address");
                    learned_observed_addr = true;
                }
                event => panic!("{event:?}"),
            }

            if learned_observed_addr && told_relay_observed_addr {
                break;
            }
        }
    });

    swarm.behaviour_mut().kademlia.set_mode(Some(libp2p::kad::Mode::Server));

    match opts.mode {
        Mode::Dial => {
            swarm
                .dial(
                    get_address_through_relay(&opts.relay_address, opts.remote_peer_id.as_ref().unwrap())
                )
                .unwrap();
        }
        Mode::Listen => {
            swarm
                .listen_on(opts.relay_address.clone().with(Protocol::P2pCircuit))
                .unwrap();
        }
    }

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines();
    let mut control = swarm.behaviour().stream.new_control();
    let mut incoming = control.accept(StreamProtocol::new("/peer-exchange/1.0.0")).unwrap();

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

                        if let Ok(str) = String::from_utf8(buffer) {
                            let json: serde_json::Value = serde_json::from_str(str.as_str())?;

                            if let Some(known_peers) = json.get("known_peers") {
                                for v in known_peers.as_array().unwrap() {
                                    let peer_id_str = v.as_str().unwrap();
                                    let known_peer_id = PeerId::from_str(peer_id_str).unwrap();
                                    let peer_addr = get_address_through_relay(
                                            &opts.relay_address,
                                            &known_peer_id);
                                    println!("Adding {:?} to Kademlia", known_peer_id);
                                    swarm.behaviour_mut().kademlia.add_address(&known_peer_id, peer_addr);
                                }
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
                        assert!(opts.mode == Mode::Listen);
                        tracing::info!("Relay accepted our reservation request");
                    }
                    SwarmEvent::Behaviour(BehaviourEvent::RelayClient(event)) => {
                        tracing::info!(?event)
                    }
                    SwarmEvent::Behaviour(BehaviourEvent::Identify(event)) => {
                        tracing::info!(?event)
                    }
                    SwarmEvent::Behaviour(BehaviourEvent::Ping(_)) => {}
                    SwarmEvent::ConnectionEstablished {
                        peer_id, endpoint, ..
                    } => {
                        // TODO: Add condition check to ignore relay node connection events
                        tracing::info!(peer=%peer_id, ?endpoint, "Established new connection");
                        let peer_relay_addr = get_address_through_relay(&opts.relay_address, &peer_id);
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

fn handle_input_line(kademlia: &mut kad::Behaviour<MemoryStore>, line: String) {
    let mut args = line.split(' ');

    match args.next() {
        Some("GET") => {
            let key = {
                match args.next() {
                    Some(key) => kad::RecordKey::new(&key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };
            kademlia.get_record(key);
        }
        Some("GET_PROVIDERS") => {
            let key = {
                match args.next() {
                    Some(key) => kad::RecordKey::new(&key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };
            kademlia.get_providers(key);
        }
        Some("PUT") => {
            let key = {
                match args.next() {
                    Some(key) => kad::RecordKey::new(&key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };
            let value = {
                match args.next() {
                    Some(value) => value.as_bytes().to_vec(),
                    None => {
                        eprintln!("Expected value");
                        return;
                    }
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
        Some("PUT_PROVIDER") => {
            let key = {
                match args.next() {
                    Some(key) => kad::RecordKey::new(&key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };

            kademlia
                .start_providing(key)
                .expect("Failed to start providing key");
        }
        _ => {
            eprintln!("expected GET, GET_PROVIDERS, PUT or PUT_PROVIDER");
        }
    }
}

fn generate_ed25519(secret_key_seed: u8) -> identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0] = secret_key_seed;

    identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
}
