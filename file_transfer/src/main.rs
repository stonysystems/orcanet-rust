mod request_handlers;

use std::{error::Error, time::Duration};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use clap::Parser;
use futures::{AsyncReadExt, AsyncWriteExt, executor::block_on, future::FutureExt, stream::StreamExt};
use libp2p::{core::multiaddr::{Multiaddr, Protocol}, identify, identity, kad, noise, PeerId, ping, relay, swarm::{NetworkBehaviour, SwarmEvent}, Swarm, tcp, yamux};
use libp2p::identity::ParseError;
use libp2p::kad::store::{MemoryStore, MemoryStoreConfig};
use libp2p::StreamProtocol;
use libp2p_stream::Control;
use tokio::{io, select, time};
use tokio::io::AsyncBufReadExt;
use tracing_subscriber::EnvFilter;
use request_handlers::{FileRequest, FileResponse, RequestHandler};

struct Config;

impl Config {
    pub const NAMESPACE: &'static str = "/orcanet";
    pub const STREAM_PROTOCOL: &'static str = "/orcanet/p2p";
    pub const SECRET_KEY_SEED: u64 = 4;

    pub fn get_bootstrap_peer_id() -> PeerId {
        PeerId::from_str("12D3KooWQd1K1k8XA9xVEzSAu7HUCodC7LJB6uW5Kw4VwkRdstPE").unwrap()
    }

    pub fn get_relay_peer_id() -> PeerId {
        PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap()
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
    stream: libp2p_stream::Behaviour,
}

fn get_address_through_relay(relay_address: &Multiaddr, peer_id: &PeerId) -> Multiaddr {
    relay_address.clone()
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(peer_id.clone()))
}

async fn send_get_file_request(control: &mut Control, peer_id: PeerId) {
    tracing::info!("Send get file request");
    let mut stream = control
        .open_stream(peer_id, StreamProtocol::new(Config::STREAM_PROTOCOL))
        .await.unwrap();
    let file_request = FileRequest {
        file_hash: String::from("abcd"),
        requester_id: String::from("idv"),
    };

    tracing::info!("Opened stream");

    match stream.write(serde_json::to_string(&file_request).unwrap().as_bytes()).await {
        Ok(_) => {
            tracing::info!("Write succeeded");
        }
        Err(err) => {
            tracing::info!(?err, "Write failed with error:");
        }
    }

    stream.close().await.unwrap();
}

#[derive(Parser)]
struct Opts {
    #[arg(long, default_value_t = 4)]
    seed: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let relay_address = Config::get_relay_address();
    let opts = Opts::parse();
    let identity = generate_ed25519(opts.seed);

    let mut swarm =
        libp2p::SwarmBuilder::with_existing_identity(identity)
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

    // Make a reservation with relay
    swarm.listen_on(relay_address.clone().with(Protocol::P2pCircuit)).unwrap();

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines();
    let stream_protocol = StreamProtocol::new(Config::STREAM_PROTOCOL);
    let mut control = swarm.behaviour().stream.new_control();
    let mut incoming = control.accept(stream_protocol.clone()).unwrap();

    block_on(async {
        loop {
            select! {
                Ok(Some(line)) = stdin.next_line() => {
                    // handle_input_line(&mut swarm.behaviour_mut().kademlia, line);
                    handle_input_line_file_check(&mut control, &mut swarm, line).await;
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

                        if peer_id != Config::get_relay_peer_id() {
                            let peer_relay_addr = get_address_through_relay(&relay_address, &peer_id);
                            send_get_file_request(&mut control, peer_id).await;
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

async fn handle_input_line_file_check(control: &mut Control, swarm: &mut Swarm<Behaviour>, line: String) {
    let mut args = line.split(' ');
    let command = args.next();

    println!("Got command {:?}", command);

    if command.is_none() {
        return;
    }

    let peer_id = match args.next()
        .map(|v| PeerId::from_str(v)) {
        Some(res) => {
            match res {
                Ok(peer_id) => peer_id,
                Err(_) => {
                    eprintln!("Invalid peer id");
                    return;
                }
            }
        }
        None => {
            eprintln!("Expected peer id");
            return;
        }
    };

    match command {
        Some("send_req") => {
            let address = get_address_through_relay(&Config::get_relay_address(), &peer_id);
            match swarm.dial(address) {
                Ok(_) => {
                    println!("Dialled successfully")
                }
                Err(e) => {
                    eprintln!("Dial failed {:?}", e);
                    return;
                }
            }
        }
        _ => {
            eprintln!("expected send_req");
        }
    }
}

fn generate_ed25519(secret_key_seed: u64) -> identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0..8].copy_from_slice(&secret_key_seed.to_le_bytes());

    identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
}
