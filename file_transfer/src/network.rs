use std::collections::{hash_map, HashMap, HashSet};
use std::default::Default;
use std::error::Error;
use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use futures::StreamExt;
use libp2p::{identify, kad, multiaddr::Protocol, noise, PeerId, ping, relay, request_response::{self, OutboundRequestId, ResponseChannel}, StreamProtocol, swarm::{NetworkBehaviour, Swarm, SwarmEvent}, tcp, yamux};
use libp2p::kad::store::{MemoryStore, MemoryStoreConfig};
use libp2p::request_response::ProtocolSupport;
use serde::{Deserialize, Serialize};

use crate::client::NetworkClient;
use crate::common::{FileRequest, FileResponse, OrcaNetCommand, OrcaNetConfig, OrcaNetEvent, Utils};

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    stream: libp2p_stream::Behaviour,
    request_response: request_response::cbor::Behaviour<FileRequest, FileResponse>,
}

/// Creates the network components, namely:
///
/// - The network client to interact with the network layer from anywhere
///   within your application.
///
/// - The network event stream, e.g. for incoming requests.
///
/// - The network task driving the network itself.
pub async fn new(
    secret_key_seed: u64,
    event_sender: mpsc::Sender<OrcaNetEvent>
) -> Result<(NetworkClient, EventLoop), Box<dyn Error>> {
    let keypair = Utils::generate_ed25519(secret_key_seed);
    let relay_address = OrcaNetConfig::get_relay_address();
    let bootstrap_peer_id = OrcaNetConfig::get_bootstrap_peer_id();
    let boostrap_addr = OrcaNetConfig::get_bootstrap_address();

    let mut swarm =
        libp2p::SwarmBuilder::with_existing_identity(keypair)
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
                request_response: request_response::cbor::Behaviour::new(
                    [(
                        StreamProtocol::new("/file-exchange/1"),
                        ProtocolSupport::Full,
                    )],
                    request_response::Config::default(),
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
    swarm.listen_on(relay_address.clone().with(Protocol::P2pCircuit)).unwrap();

    // Set up kademlia props
    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(Some(kad::Mode::Server));
    // swarm
    //     .behaviour_mut()
    //     .kademlia
    //     .add_address(&bootstrap_peer_id, boostrap_addr.clone());

    // Dial the bootstrap node
    // swarm.dial(boostrap_addr.clone()).unwrap();

    let (command_sender, command_receiver) = mpsc::channel(0);

    Ok((
        NetworkClient {
            sender: command_sender,
        },
        EventLoop::new(swarm, command_receiver, event_sender),
    ))
}

pub struct EventLoop {
    swarm: Swarm<Behaviour>,
    command_receiver: mpsc::Receiver<OrcaNetCommand>,
    event_sender: mpsc::Sender<OrcaNetEvent>,
    pending_dial: HashMap<PeerId, oneshot::Sender<Result<(), Box<dyn Error + Send>>>>,
    pending_start_providing: HashMap<kad::QueryId, oneshot::Sender<()>>,
    pending_get_providers: HashMap<kad::QueryId, oneshot::Sender<HashSet<PeerId>>>,
    pending_request_file: HashMap<OutboundRequestId, oneshot::Sender<Result<FileResponse, Box<dyn Error + Send>>>>,
    pending_put_kv: HashMap<kad::QueryId, oneshot::Sender<Result<(), Box<dyn Error + Send>>>>,
    pending_get_value: HashMap<kad::QueryId, oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>>,
}

impl EventLoop {
    fn new(
        swarm: Swarm<Behaviour>,
        command_receiver: mpsc::Receiver<OrcaNetCommand>,
        event_sender: mpsc::Sender<OrcaNetEvent>,
    ) -> Self {
        Self {
            swarm,
            command_receiver,
            event_sender,
            pending_dial: Default::default(),
            pending_start_providing: Default::default(),
            pending_get_providers: Default::default(),
            pending_request_file: Default::default(),
            pending_put_kv: Default::default(),
            pending_get_value: Default::default(),
        }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => self.handle_event(event).await,

                command = self.command_receiver.next() => match command {
                    Some(c) => self.handle_command(c).await,
                    // Command channel closed, thus shutting down the network event loop.
                    None=>  return,
                },
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
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
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(kad::Event::OutboundQueryProgressed { id, result, .. })) => {
                self.handle_kademlia_events(id, result);
            }
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(event)) => {
                tracing::info!(?event, "Received kademlia event");
            }
            SwarmEvent::Behaviour(BehaviourEvent::RequestResponse(
                                      request_response::Event::Message { message, .. },
                                  )) => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    tracing::info!(?request, "Received request");

                    self.event_sender
                        .send(OrcaNetEvent::FileRequest {
                            file_id: request.0,
                            channel,
                        })
                        .await
                        .expect("Event receiver not to be dropped.");
                }
                request_response::Message::Response {
                    request_id,
                    response,
                } => {
                    tracing::debug!(?response, "Received file");

                    let _ = self
                        .pending_request_file
                        .remove(&request_id)
                        .expect("Request to still be pending.")
                        .send(Ok(response));
                }
            },
            SwarmEvent::Behaviour(BehaviourEvent::RequestResponse(
                                      request_response::Event::OutboundFailure {
                                          request_id, error, ..
                                      },
                                  )) => {
                tracing::info!(?error, "Request response outbound failure:");

                let _ = self
                    .pending_request_file
                    .remove(&request_id)
                    .expect("Request to still be pending.")
                    .send(Err(Box::new(error)));
            }
            SwarmEvent::Behaviour(BehaviourEvent::RequestResponse(
                                      request_response::Event::ResponseSent { .. },
                                  )) => {}
            SwarmEvent::IncomingConnection { .. } => {}
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                tracing::info!(peer=%peer_id, ?endpoint, "Established new connection");

                if endpoint.is_dialer() {
                    if let Some(sender) = self.pending_dial.remove(&peer_id) {
                        let _ = sender.send(Ok(()));
                    }
                }
            }
            SwarmEvent::ConnectionClosed { .. } => {}
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                tracing::error!(peer=?peer_id, "Outgoing connection failed: {error}");
                if let Some(peer_id) = peer_id {
                    if let Some(sender) = self.pending_dial.remove(&peer_id) {
                        let _ = sender.send(Err(Box::new(error)));
                    }
                }
            }
            SwarmEvent::IncomingConnectionError { .. } => {}
            SwarmEvent::Dialing {
                peer_id: Some(peer_id),
                ..
            } => tracing::info!("Dialing {peer_id}"),
            _ => {}
            // e => panic!("{e:?}"),
        }
    }

    fn handle_kademlia_events(&mut self, query_id: kad::QueryId, result: kad::QueryResult) {
        match result {
            kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders { key, providers, .. })) => {
                for peer in &providers {
                    println!(
                        "Peer {peer:?} provides key {:?}",
                        std::str::from_utf8(key.as_ref()).unwrap()
                    );
                }
                if let Some(sender) = self.pending_get_providers.remove(&query_id) {
                    sender.send(providers).expect("Receiver not to be dropped");

                    // Finish the query. We are only interested in the first result.
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .query_mut(&query_id)
                        .unwrap()
                        .finish();
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
                // println!(
                //     "Got record {:?} {:?}",
                //     std::str::from_utf8(key.as_ref()).unwrap(),
                //     std::str::from_utf8(&value).unwrap(),
                // );
                if let Some(sender) = self.pending_get_value.remove(&query_id) {
                    sender.send(Ok(value)).expect("Receiver not to be dropped");
                }
            }
            kad::QueryResult::GetRecord(Err(err)) => {
                if let Some(sender) = self.pending_get_value.remove(&query_id) {
                    sender.send(Err(Box::new(err.clone()))).expect("Receiver not to be dropped");
                }
                eprintln!("Failed to get record: {err:?}");
            }
            kad::QueryResult::PutRecord(Ok(kad::PutRecordOk { key })) => {
                println!(
                    "Successfully put record {:?}",
                    std::str::from_utf8(key.as_ref()).unwrap()
                );
                if let Some(sender) = self.pending_put_kv.remove(&query_id) {
                    sender.send(Ok(())).expect("Receiver not to be dropped");
                }
            }
            kad::QueryResult::PutRecord(Err(err)) => {
                eprintln!("Failed to put record: {err:?}");
                if let Some(sender) = self.pending_put_kv.remove(&query_id) {
                    sender.send(Err(Box::new(err))).expect("Receiver not to be dropped");
                }
            }
            kad::QueryResult::StartProviding(Ok(kad::AddProviderOk { key })) => {
                println!(
                    "Successfully put provider record {:?}",
                    std::str::from_utf8(key.as_ref()).unwrap()
                );
                let sender: oneshot::Sender<()> = self
                    .pending_start_providing
                    .remove(&query_id)
                    .expect("Completed query to be previously pending.");
                let _ = sender.send(());
            }
            kad::QueryResult::StartProviding(Err(err)) => {
                eprintln!("Failed to put provider record: {err:?}");
            }
            _ => {}
        }
    }

    async fn handle_command(&mut self, command: OrcaNetCommand) {
        match command {
            OrcaNetCommand::StartListening { addr, sender } => {
                let _ = match self.swarm.listen_on(addr) {
                    Ok(_) => sender.send(Ok(())),
                    Err(e) => sender.send(Err(Box::new(e))),
                };
            }
            OrcaNetCommand::Dial {
                peer_id,
                peer_addr,
                sender,
            } => {
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, peer_addr.clone());

                if let hash_map::Entry::Vacant(e) = self.pending_dial.entry(peer_id) {
                    // self.swarm
                    //     .behaviour_mut()
                    //     .kademlia
                    //     .add_address(&peer_id, peer_addr.clone());

                    match self.swarm.dial(peer_addr.clone()) {
                        Ok(()) => {
                            e.insert(sender);
                        }
                        Err(e) => {
                            let _ = sender.send(Err(Box::new(e)));
                        }
                    }
                } else {
                    todo!("Already dialing peer.");
                }
            }
            OrcaNetCommand::StartProviding { file_id, sender } => {
                let query_id = self.swarm
                    .behaviour_mut()
                    .kademlia
                    .start_providing(file_id.into_bytes().into())
                    .expect("No store error.");

                self.pending_start_providing.insert(query_id, sender);
            }
            OrcaNetCommand::GetProviders { file_id, sender } => {
                let query_id = self.swarm
                    .behaviour_mut()
                    .kademlia
                    .get_providers(file_id.into_bytes().into());
                self.pending_get_providers.insert(query_id, sender);
            }
            OrcaNetCommand::RequestFile {
                file_id,
                peer,
                sender,
            } => {
                let request_id = self.swarm
                    .behaviour_mut()
                    .request_response
                    .send_request(&peer, FileRequest(file_id));
                println!("Sent file request");
                self.pending_request_file.insert(request_id, sender);
            }
            OrcaNetCommand::RespondFile { file_resp, channel } => {
                self.swarm
                    .behaviour_mut()
                    .request_response
                    .send_response(channel, file_resp)
                    .expect("Connection to peer to be still open.");
            }
            OrcaNetCommand::PutKV { key, value, sender } => {
                let record = kad::Record {
                    key: kad::RecordKey::new(&key.as_str()),
                    value: value.into(),
                    publisher: None,
                    expires: None,
                };
                let request_id = self.swarm
                    .behaviour_mut()
                    .kademlia
                    .put_record(record, kad::Quorum::One)
                    .expect("Failed to initiate put request");
                self.pending_put_kv.insert(request_id, sender);
            }
            OrcaNetCommand::GetValue { key, sender } => {
                let request_id = self.swarm
                    .behaviour_mut()
                    .kademlia
                    .get_record(kad::RecordKey::new(&key.as_str()));
                self.pending_get_value.insert(request_id, sender);
            }
        }
    }
}
