// Copyright 2021 Protocol Labs.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.

use futures::channel::oneshot::channel;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use futures::StreamExt;

use libp2p::{
    core::Multiaddr,
    identity, kad,
    multiaddr::Protocol,
    noise,
    request_response::{self, OutboundRequestId, ProtocolSupport, ResponseChannel},
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    tcp, yamux, PeerId,
    identify::{Config as IdentifyConfig, Behaviour as IdentifyBehavior, Event as IdentifyEvent}
};

use libp2p::StreamProtocol;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map, HashMap, HashSet};
use std::error::Error;
use std::time::Duration;

use tokio::time::timeout;
use tokio::sync::Mutex;
use std::sync::Arc;
/// Creates the network components, namely:
///
/// - The network client to interact with the network layer from anywhere
///   within your application.
///
/// - The network event stream, e.g. for incoming requests.
///
/// - The network task driving the network itself.
/// 
/// - Peer ID for the node.
pub(crate) async fn new(
    secret_key_seed: Option<u8>,
) -> Result<(NetworkClient, impl Stream<Item = Event>, EventLoop, PeerId), Box<dyn Error>> {
    // Create a public/private key pair, either random or based on a seed.
    let id_keys = match secret_key_seed {
        Some(seed) => {
            let mut bytes = [0u8; 32];
            bytes[0] = seed;
            identity::Keypair::ed25519_from_bytes(bytes).unwrap()
        }
        None => identity::Keypair::generate_ed25519(),
    };
    let peer_id = id_keys.public().to_peer_id();

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(id_keys)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| Behaviour {
            kademlia: kad::Behaviour::new(
                peer_id,
                kad::store::MemoryStore::new(key.public().to_peer_id()),
            ),
            request_response: request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::new("/function-request/1"),
                    ProtocolSupport::Full,
                )],
                request_response::Config::default(),
            ),
            identify: IdentifyBehavior::new(
                IdentifyConfig::new(
                "/agent/connection/1.0.0".to_string(), 
                key.clone().public()
                )
            )
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(Some(kad::Mode::Server));

    let (command_sender, command_receiver) = mpsc::channel(0);
    let (event_sender, event_receiver) = mpsc::channel(0);

    Ok((
        NetworkClient {
            sender: Arc::new(Mutex::new(command_sender)),
        },
        event_receiver,
        EventLoop::new(swarm, command_receiver, event_sender),
        peer_id
    ))
}

#[derive(Clone)]
pub(crate) struct NetworkClient {
    sender: Arc<Mutex<mpsc::Sender<Command>>>,
}

impl NetworkClient {
    /// Listen for incoming connections on the given address.
    pub(crate) async fn start_listening(
        &self,
        addr: Multiaddr,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        let sender_clone = Arc::clone(&self.sender);
        {
        let mut locked_sender = sender_clone.lock().await;
        locked_sender.send(Command::StartListening { addr, sender })
            .await
            .expect("Command receiver not to be dropped.");
        }
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Dial the given peer at the given address.
    pub(crate) async fn dial(
        &self,
        peer_id: PeerId,
        peer_addr: Multiaddr,
    ) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        let sender_clone = Arc::clone(&self.sender);
        {
        let mut locked_sender = sender_clone.lock().await;
        locked_sender.send(Command::Dial {
                peer_id,
                peer_addr,
                sender,
            })
            .await
            .expect("Command receiver not to be dropped.");
        }
        receiver.await.expect("Sender not to be dropped.")
    }

    /// Advertise the local node as the provider of the given function on the DHT.
    pub(crate) async fn start_providing(&self, function_name: String) {
        let providers = self.get_providers(function_name.clone()).await;
        println!("Providers before start providing: {:?}", providers);
        let (sender, receiver) = oneshot::channel();
        let sender_clone = Arc::clone(&self.sender);
        {
        let mut locked_sender = sender_clone.lock().await;
        locked_sender
            .send(Command::StartProviding { function_name: function_name.clone(), sender })
            .await
            .expect("Command receiver not to be dropped.");
        }
        receiver.await.expect("Sender not to be dropped.");
        let providers = self.get_providers(function_name.clone()).await;
        println!("Providers after start providing: {:?}", providers);
    }

    /// Find the providers for the given function on the DHT.
    pub(crate) async fn get_providers(&self, function_name: String) -> HashSet<PeerId> {
        let (sender, receiver) = oneshot::channel();
        let sender_clone = Arc::clone(&self.sender);
        {
        let mut locked_sender = sender_clone.lock().await;
        locked_sender.send(Command::GetProviders { function_name, sender })
            .await
            .expect("Command receiver not to be dropped.");
        }
        
        // Add 5 sec timeout to avoid infinite waiting
        match timeout(Duration::from_secs(5), receiver).await {
            Ok(Ok(providers)) => providers,
            Ok(_) => HashSet::new(),
            Err(_) => HashSet::new(),
        }
    }

    /// Request the content of the given function from the given peer.
    pub(crate) async fn request_function(
        &self,
        peer: PeerId,
        function_name: String,
        method: String,
        body: Option<Vec<u8>>,
    ) -> Result<FunctionResponse, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        let sender_clone = Arc::clone(&self.sender);
        println!("Sending request function command");
        {
        let mut locked_sender = sender_clone.lock().await;
        locked_sender.send(Command::RequestFunction {
                function_name,
                method,
                body,
                peer,
                sender,
            })
            .await
            .expect("Command receiver not to be dropped.");
        }
        println!("Waiting for response");
        
        let res = receiver.await.expect("Sender not be dropped.");
        println!("Response received");
        println!("Response: {:?}", res);
        res
    }

    /// Respond with the provided function content to the given request.
    pub(crate) async fn respond_function(
        &self,
        function_response_status: u16,
        function_response_body: Vec<u8>,
        channel: ResponseChannel<FunctionResponse>,
    ) -> Result<(), Box<dyn Error + Send>> {
        let sender_clone = Arc::clone(&self.sender);
        {
        let mut locked_sender = sender_clone.lock().await;
        println!("Sender locked. Sending response");
        println!("Response status: {:?}", function_response_status);
        println!("Response body: {:?}", function_response_body);

        let r = locked_sender
            .send(Command::RespondFunction { function_response_status, function_response_body, channel })
            .await;
        match r {
            Ok(_) => println!("Response sent"),
            Err(e) => println!("Error sending response: {:?}", e),
        }
        }
        println!("Response sent");
        Ok(())
    }
}

pub(crate) struct EventLoop {
    swarm: Swarm<Behaviour>,
    command_receiver: mpsc::Receiver<Command>,
    event_sender: Arc<Mutex<mpsc::Sender<Event>>>,
    pending_dial: Arc<Mutex<HashMap<PeerId, oneshot::Sender<Result<(), Box<dyn Error + Send>>>>>>,
    pending_start_providing: Arc<Mutex<HashMap<kad::QueryId, oneshot::Sender<()>>>>,
    pending_get_providers: Arc<Mutex<HashMap<kad::QueryId, oneshot::Sender<HashSet<PeerId>>>>>,
    pending_request_function: Arc<Mutex<HashMap<OutboundRequestId, oneshot::Sender<Result<FunctionResponse, Box<dyn Error + Send>>>>>>,
}

impl EventLoop {
    fn new(
        swarm: Swarm<Behaviour>,
        command_receiver: mpsc::Receiver<Command>,
        event_sender: mpsc::Sender<Event>,
    ) -> Self {
        Self {
            swarm: swarm,
            command_receiver: command_receiver,
            event_sender: Arc::new(Mutex::new(event_sender)),
            pending_dial: Arc::new(Mutex::new(Default::default())),
            pending_start_providing: Arc::new(Mutex::new(Default::default())),
            pending_get_providers: Arc::new(Mutex::new(Default::default())),
            pending_request_function: Arc::new(Mutex::new(Default::default())),
        }
    }

    pub(crate) async fn run(mut self) {
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => self.handle_event(event).await,
                command = self.command_receiver.next() => match command {
                    Some(c) => {
                        println!("Received command: {:?}", c);
                        self.handle_command(c).await;
                    },
                    None => return, // Command channel closed, shutting down
                },
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        let mut event_sender = self.event_sender.lock().await;
        let mut pending_dial = self.pending_dial.lock().await;
        let mut pending_start_providing = self.pending_start_providing.lock().await;
        let mut pending_get_providers = self.pending_get_providers.lock().await;
        let mut pending_request_function = self.pending_request_function.lock().await;
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed {
                    id,
                    result: kad::QueryResult::StartProviding(_),
                    ..
                },
            )) => {
                let sender: oneshot::Sender<()> = pending_start_providing
                    .remove(&id)
                    .expect("Completed query to be previously pending.");
                let _ = sender.send(());
            }
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed {
                    id,
                    result:
                        kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders {
                            providers,
                            ..
                        })),
                    ..
                },
            )) => {
                if let Some(sender) = pending_get_providers.remove(&id) {
                    
                    for provider in &providers {
                        // TODO check if we are already connected to the provider
                        if !self.swarm.is_connected(provider) {
                            match self.swarm.dial(provider.clone()) {
                                Ok(()) => {
                                    let (dial_sender, dial_receiver) = oneshot::channel();
                                    pending_dial.insert(provider.clone(), dial_sender);
                                    println!("Dialing provider {:?}", provider);
                                    // Add 3 sec timeout to avoid infinite waiting, get providers timeout has to be higher than this timeout
                                    timeout(Duration::from_secs(3), dial_receiver).await;
                                }
                                Err(e) => {
                                    println!("Error dialing provider {:?}: {:?}", provider, e);
                                }
                            }                
                        }
                    }

                    sender.send(providers).expect("Receiver not to be dropped");

                    // Finish the query. We are only interested in the first result.
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .query_mut(&id)
                        .unwrap()
                        .finish();
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed {
                    result:
                        kad::QueryResult::GetProviders(Ok(
                            kad::GetProvidersOk::FinishedWithNoAdditionalRecord { .. },
                        )),
                    ..
                },
            )) => {}
            SwarmEvent::Behaviour(BehaviourEvent::Kademlia(_)) => {}
            SwarmEvent::Behaviour(BehaviourEvent::RequestResponse(
                request_response::Event::Message { message, .. },
            )) => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    println!("Sending inbound request event: {:?}", request);
                    event_sender
                        .send(Event::InboundRequest {
                            request: request.0,
                            method: request.1,
                            body: request.2,
                            channel,
                        })
                        .await
                        .expect("Event receiver not to be dropped.");
                }
                request_response::Message::Response {
                    request_id,
                    response,
                } => {
                    let _ = pending_request_function
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
                let _ = pending_request_function
                    .remove(&request_id)
                    .expect("Request to still be pending.")
                    .send(Err(Box::new(error)));
            }
            SwarmEvent::Behaviour(BehaviourEvent::RequestResponse(
                request_response::Event::ResponseSent { .. },
            )) => {}
            SwarmEvent::Behaviour(BehaviourEvent::Identify(IdentifyEvent::Received {
                peer_id,
                info,
            })) => {
                eprintln!("Received identify info from peer {:?}: {:?}", peer_id, info);
                info.clone().listen_addrs.iter().for_each(|addr| {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());
                });
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                let local_peer_id = *self.swarm.local_peer_id();
                eprintln!(
                    "Local node is listening on {:?}",
                    address.with(Protocol::P2p(local_peer_id))
                );
            }
            SwarmEvent::NewExternalAddrOfPeer { peer_id, address, .. } => {
                eprintln!("Discovered external address {:?} for peer {:?}", address, peer_id);
            }
            SwarmEvent::IncomingConnection { .. } => {}
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                println!("Connected to peer: {:?}", peer_id.to_base58());
                if endpoint.is_dialer() {
                    if let Some(sender) = pending_dial.remove(&peer_id) {
                        let _ = sender.send(Ok(()));
                    }
                }
                // Add dialing node to dialed node routing table. (Enable bootstrap node to get_providers)
                else {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, endpoint.get_remote_address().clone());
                }
            }
            SwarmEvent::ConnectionClosed { .. } => {}
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                if let Some(peer_id) = peer_id {
                    if let Some(sender) = pending_dial.remove(&peer_id) {
                        let _ = sender.send(Err(Box::new(error)));
                    }
                }
            }
            SwarmEvent::IncomingConnectionError { .. } => {}
            SwarmEvent::Dialing {
                peer_id: Some(peer_id),
                ..
            } => eprintln!("Dialing {peer_id}"),
            e => eprintln!("Not handled event: {e:?}"),
        }
    }

    async fn handle_command(&mut self, command: Command) {
        let mut pending_dial = self.pending_dial.lock().await;
        let mut pending_start_providing = self.pending_start_providing.lock().await;
        let mut pending_get_providers = self.pending_get_providers.lock().await;
        let mut pending_request_function = self.pending_request_function.lock().await;
        match command {
            Command::StartListening { addr, sender } => {
                let _ = match self.swarm.listen_on(addr) {
                    Ok(_) => sender.send(Ok(())),
                    Err(e) => sender.send(Err(Box::new(e))),
                };
            }
            Command::Dial {
                peer_id,
                peer_addr,
                sender,
            } => {
                if let hash_map::Entry::Vacant(e) = pending_dial.entry(peer_id) {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, peer_addr.clone());
                    match self.swarm.dial(peer_addr.with(Protocol::P2p(peer_id))) {
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
            Command::StartProviding { function_name, sender } => {
                let query_id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .start_providing(function_name.into_bytes().into())
                    .expect("No store error.");
                pending_start_providing.insert(query_id, sender);
            }
            Command::GetProviders { function_name, sender } => {
                let query_id = self.swarm
                    .behaviour_mut()
                    .kademlia
                    .get_providers(function_name.into_bytes().into());
                pending_get_providers.insert(query_id, sender);
            }
            Command::RequestFunction {
                function_name,
                method,
                body,
                peer,
                sender,
            } => {
                let request_id = self
                    .swarm
                    .behaviour_mut()
                    .request_response
                    .send_request(&peer, FunctionRequest(function_name, method, body));
                pending_request_function.insert(request_id, sender);
                println!("Request {:?} stored", request_id);
            }
            Command::RespondFunction { function_response_status, function_response_body, channel } => {
                println!("Command RespondFunction");
                self.swarm
                    .behaviour_mut()
                    .request_response
                    .send_response(channel, FunctionResponse(function_response_status, function_response_body))
                    .expect("Connection to peer to be still open.");
            }
        }
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    request_response: request_response::cbor::Behaviour<FunctionRequest, FunctionResponse>,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    identify: IdentifyBehavior,
}

#[derive(Debug)]
enum Command {
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
        function_name: String,
        sender: oneshot::Sender<()>,
    },
    GetProviders {
        function_name: String,
        sender: oneshot::Sender<HashSet<PeerId>>,
    },
    RequestFunction {
        function_name: String,
        method: String,
        body: Option<Vec<u8>>,
        peer: PeerId,
        sender: oneshot::Sender<Result<FunctionResponse, Box<dyn Error + Send>>>,
    },
    RespondFunction {
        function_response_status: u16,
        function_response_body: Vec<u8>,
        channel: ResponseChannel<FunctionResponse>,
    },
}

#[derive(Debug)]
pub(crate) enum Event {
    InboundRequest {
        request: String,
        method: String,
        body: Option<Vec<u8>>,
        channel: ResponseChannel<FunctionResponse>,
    },
}

// Simple function exchange protocol
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FunctionRequest(String,String,Option<Vec<u8>>);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FunctionResponse(pub u16, pub Vec<u8>);