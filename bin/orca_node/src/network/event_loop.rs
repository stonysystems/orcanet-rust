use std::collections::{hash_map, HashMap, HashSet};
use std::default::Default;
use std::error::Error;
use std::time::Duration;

use bincode;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use futures::StreamExt;
use libp2p::bytes::Bytes;
use libp2p::kad::store::{MemoryStore, MemoryStoreConfig};
use libp2p::request_response::ProtocolSupport;
use libp2p::{
    identify, kad,
    multiaddr::Protocol,
    noise, ping, relay,
    request_response::{self, OutboundRequestId, ResponseChannel},
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    tcp, yamux, PeerId, StreamProtocol,
};
use libp2p_swarm::Stream;
use serde::{Deserialize, Serialize};

use crate::common::config::{ConfigKey, OrcaNetConfig};
use crate::common::types::{
    NetworkCommand, OrcaNetEvent, OrcaNetRequest, OrcaNetResponse, StreamData, StreamReq,
};
use crate::common::Utils;
use crate::network::NetworkClient;

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    stream: libp2p_stream::Behaviour,
    request_response: request_response::cbor::Behaviour<OrcaNetRequest, OrcaNetResponse>,
}

pub async fn setup_network_event_loop(
    secret_key_seed: u64,
    event_sender: mpsc::Sender<OrcaNetEvent>,
) -> Result<(NetworkClient, NetworkEventLoop), Box<dyn Error>> {
    // Create the swarm to manage the network
    let keypair = Utils::generate_ed25519(secret_key_seed);
    let relay_address = OrcaNetConfig::get_relay_address();
    let bootstrap_peer_id = OrcaNetConfig::get_bootstrap_peer_id();
    let boostrap_addr = OrcaNetConfig::get_bootstrap_address();

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
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .expect("Listening for TCP failed");
    swarm
        .listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap())
        .expect("Listening for QuickV1 failed");

    // Make a reservation with relay
    swarm
        .listen_on(relay_address.clone().with(Protocol::P2pCircuit))
        .expect("Reservation attempt failed");

    // Set up kademlia props
    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(Some(kad::Mode::Server));

    // Connect to the bootstrap node
    swarm
        .behaviour_mut()
        .kademlia
        .add_address(&bootstrap_peer_id, boostrap_addr.clone());
    swarm
        .dial(boostrap_addr.clone())
        .expect("Failed to connect to bootstrap node");

    let (command_sender, command_receiver) = mpsc::channel(0);

    Ok((
        NetworkClient {
            sender: command_sender,
        },
        NetworkEventLoop::new(swarm, command_receiver, event_sender),
    ))
}

pub struct NetworkEventLoop {
    swarm: Swarm<Behaviour>,
    command_receiver: mpsc::Receiver<NetworkCommand>,
    event_sender: mpsc::Sender<OrcaNetEvent>,
    pending_dial: HashMap<PeerId, oneshot::Sender<Result<(), Box<dyn Error + Send>>>>,
    pending_start_providing: HashMap<kad::QueryId, oneshot::Sender<()>>,
    pending_get_providers: HashMap<kad::QueryId, oneshot::Sender<HashSet<PeerId>>>,
    pending_request:
        HashMap<OutboundRequestId, oneshot::Sender<Result<OrcaNetResponse, Box<dyn Error + Send>>>>,
    pending_put_kv: HashMap<kad::QueryId, oneshot::Sender<Result<(), Box<dyn Error + Send>>>>,
    pending_get_value:
        HashMap<kad::QueryId, oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>>,
    pending_stream_requests:
        HashMap<String, oneshot::Sender<Result<OrcaNetResponse, Box<dyn Error + Send>>>>,
}

impl NetworkEventLoop {
    fn new(
        swarm: Swarm<Behaviour>,
        command_receiver: mpsc::Receiver<NetworkCommand>,
        event_sender: mpsc::Sender<OrcaNetEvent>,
    ) -> Self {
        Self {
            swarm,
            command_receiver,
            event_sender,
            pending_dial: Default::default(),
            pending_start_providing: Default::default(),
            pending_get_providers: Default::default(),
            pending_request: Default::default(),
            pending_put_kv: Default::default(),
            pending_get_value: Default::default(),
            pending_stream_requests: Default::default(),
        }
    }

    pub async fn run(mut self) {
        let stream_protocol = StreamProtocol::new(OrcaNetConfig::STREAM_PROTOCOL);
        let mut control = self.swarm.behaviour().stream.new_control();
        let mut incoming = control.accept(stream_protocol.clone()).unwrap();

        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => self.handle_event(event).await,

                command = self.command_receiver.next() => match command {
                    Some(c) => self.handle_command(c).await,
                    // Command channel closed, thus shutting down the network event loop.
                    None=>  return,
                },

                stream_event = incoming.next() => match stream_event {
                    Some((peer_id, mut stream)) => {
                        tracing::info!("Received stream from {:?}", peer_id);

                        self.handle_stream_req(peer_id, &mut stream).await;
                    }
                    None => {}
                }
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
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed { id, result, .. },
            )) => {
                self.handle_kademlia_events(id, result);
            }
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(event)) => {
                tracing::info!(?event, "Received kademlia event");
            }
            SwarmEvent::Behaviour(BehaviourEvent::RequestResponse(
                request_response::Event::Message { message, peer },
            )) => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    tracing::info!(?request, "Received request");

                    self.event_sender
                        .send(OrcaNetEvent::Request {
                            request,
                            from_peer: peer,
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
                        .pending_request
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
                    .pending_request
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
        }
    }

    fn handle_kademlia_events(&mut self, query_id: kad::QueryId, result: kad::QueryResult) {
        match result {
            kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders {
                key,
                providers,
                ..
            })) => {
                if let Some(sender) = self.pending_get_providers.remove(&query_id) {
                    sender.send(providers).expect("Receiver not to be dropped");
                }
            }
            kad::QueryResult::GetProviders(Err(err)) => {
                tracing::error!("Failed to get providers: {err:?}");
                if let Some(sender) = self.pending_get_providers.remove(&query_id) {
                    sender
                        .send(HashSet::new())
                        .expect("Receiver not to be dropped");
                }
            }
            kad::QueryResult::GetRecord(Ok(kad::GetRecordOk::FoundRecord(kad::PeerRecord {
                record: kad::Record { key, value, .. },
                ..
            }))) => {
                if let Some(sender) = self.pending_get_value.remove(&query_id) {
                    sender.send(Ok(value)).expect("Receiver not to be dropped");
                }
            }
            kad::QueryResult::GetRecord(Err(err)) => {
                if let Some(sender) = self.pending_get_value.remove(&query_id) {
                    sender
                        .send(Err(Box::new(err.clone())))
                        .expect("Receiver not to be dropped");
                }
                tracing::error!("Failed to get record: {err:?}");
            }
            kad::QueryResult::PutRecord(Ok(kad::PutRecordOk { key })) => {
                tracing::info!(
                    "Successfully put record {:?}",
                    std::str::from_utf8(key.as_ref()).unwrap()
                );
                if let Some(sender) = self.pending_put_kv.remove(&query_id) {
                    sender.send(Ok(())).expect("Receiver not to be dropped");
                }
            }
            kad::QueryResult::PutRecord(Err(err)) => {
                tracing::error!("Failed to put record: {err:?}");
                if let Some(sender) = self.pending_put_kv.remove(&query_id) {
                    sender
                        .send(Err(Box::new(err)))
                        .expect("Receiver not to be dropped");
                }
            }
            kad::QueryResult::StartProviding(Ok(kad::AddProviderOk { key })) => {
                tracing::info!(
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
                tracing::error!("Failed to put provider record: {err:?}");
                // TODO: May be add handling later ?
            }
            _ => {}
        }
    }

    async fn handle_command(&mut self, command: NetworkCommand) {
        match command {
            NetworkCommand::StartListening { addr, sender } => {
                let _ = match self.swarm.listen_on(addr) {
                    Ok(_) => sender.send(Ok(())),
                    Err(e) => sender.send(Err(Box::new(e))),
                };
            }
            NetworkCommand::Dial {
                peer_id,
                peer_addr,
                sender,
            } => {
                // self.swarm
                //     .behaviour_mut()
                //     .kademlia
                //     .add_address(&peer_id, peer_addr.clone());

                if let hash_map::Entry::Vacant(e) = self.pending_dial.entry(peer_id) {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, peer_addr.clone());

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
            NetworkCommand::StartProviding { key, sender } => {
                let key_with_ns = Utils::get_key_with_ns(key.as_str());
                let query_id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .start_providing(key_with_ns.into_bytes().into())
                    .expect("No store error.");

                self.pending_start_providing.insert(query_id, sender);
            }
            NetworkCommand::StopProviding { key } => {
                let key_with_ns = Utils::get_key_with_ns(key.as_str());
                let record_key = kad::RecordKey::new(&key_with_ns.into_bytes());

                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .stop_providing(&record_key);
            }
            NetworkCommand::GetProviders { key, sender } => {
                let key_with_ns = Utils::get_key_with_ns(key.as_str());
                let query_id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .get_providers(key_with_ns.into_bytes().into());

                self.pending_get_providers.insert(query_id, sender);
            }
            NetworkCommand::Request {
                request,
                peer,
                sender,
            } => {
                let request_id = self
                    .swarm
                    .behaviour_mut()
                    .request_response
                    .send_request(&peer, request);

                self.pending_request.insert(request_id, sender);

                tracing::info!("Sent request to {:?}", peer);
            }
            NetworkCommand::Respond { response, channel } => {
                self.swarm
                    .behaviour_mut()
                    .request_response
                    .send_response(channel, response)
                    .expect("Connection to peer to be still open.");

                tracing::info!("Sent response");
            }
            NetworkCommand::PutKV { key, value, sender } => {
                let key_with_ns = Utils::get_key_with_ns(key.as_str());
                let record = kad::Record {
                    key: kad::RecordKey::new(&key_with_ns.as_str()),
                    value: value.into(),
                    publisher: None,
                    expires: None,
                };
                let request_id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .put_record(record, kad::Quorum::One)
                    .expect("Failed to initiate put request");

                self.pending_put_kv.insert(request_id, sender);
            }
            NetworkCommand::GetValue { key, sender } => {
                let key_with_ns = Utils::get_key_with_ns(key.as_str());
                let request_id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .get_record(kad::RecordKey::new(&key_with_ns.as_str()));

                self.pending_get_value.insert(request_id, sender);
            }
            NetworkCommand::SendInStream {
                peer_id,
                stream_req,
                sender,
            } => {
                let mut control = self.swarm.behaviour_mut().stream.new_control();
                let content_bytes = bincode::serialize(&stream_req).unwrap();
                tracing::info!("Sending {} bytes", content_bytes.len());

                let open_stream_resp = control
                    .open_stream(
                        peer_id.clone(),
                        StreamProtocol::new(OrcaNetConfig::STREAM_PROTOCOL),
                    )
                    .await;

                match open_stream_resp {
                    Ok(mut stream) => {
                        tracing::info!("Opened stream");
                        match stream.write_all(content_bytes.as_slice()).await {
                            Ok(_) => {
                                tracing::info!("Wrote successfully");

                                if let Some(sender) = sender {
                                    self.pending_stream_requests
                                        .insert(stream_req.request_id, sender);
                                }

                                let _ = stream.close().await;
                            }
                            Err(e) => tracing::error!("Failed to write to stream: {:?}", e),
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to open stream: {:?}", e)
                    }
                }
            }
        }
    }

    async fn handle_stream_req(&mut self, peer_id: PeerId, stream: &mut Stream) {
        let mut buffer = Vec::new();
        if let Err(e) = stream.read_to_end(&mut buffer).await {
            tracing::error!("Failed to read from stream: {:?}", e);
            return;
        }

        let stream_req: StreamReq = match bincode::deserialize(buffer.as_slice()) {
            Ok(content) => content,
            Err(e) => {
                tracing::error!("Error deserializing stream request: {:?}", e);
                return;
            }
        };

        match stream_req.stream_data {
            StreamData::Request(request) => {
                tracing::info!("Received request: {:?}", request);
                let (sender, receiver) = oneshot::channel();

                self.event_sender
                    .send(OrcaNetEvent::StreamRequest {
                        from_peer: peer_id,
                        request,
                        sender,
                    })
                    .await
                    .expect("Command receiver not to be dropped");

                let response = receiver.await.expect("Sender not to be dropped");

                self.handle_command(NetworkCommand::SendInStream {
                    peer_id,
                    stream_req: StreamReq {
                        request_id: stream_req.request_id.clone(),
                        stream_data: StreamData::Response(response),
                    },
                    sender: None,
                })
                .await;
            }
            StreamData::Response(response) => {
                tracing::info!("Received stream response");
                // Utils::handle_file_response(response_content);

                if let Some(sender) = self.pending_stream_requests.remove(&stream_req.request_id) {
                    let _ = sender.send(Ok(response)).expect("Send to work");
                }
            }
        }
    }
}
