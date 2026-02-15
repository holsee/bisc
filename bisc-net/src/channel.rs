//! Channel lifecycle: create, join, leave, peer management, heartbeats.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use bisc_protocol::channel::{ChannelMessage, MediaCapabilities};
use bisc_protocol::ticket::BiscTicket;
use bisc_protocol::types::{EndpointAddr, EndpointId};
use iroh::address_lookup::memory::MemoryLookup;
use iroh_gossip::proto::TopicId;
use tokio::sync::{mpsc, watch, RwLock};

use crate::connection::PeerConnection;
use crate::gossip::{GossipEvent, GossipHandle, TopicSubscription};

/// How often to send heartbeats.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// How long before a peer is considered timed out.
const PEER_TIMEOUT: Duration = Duration::from_secs(15);

/// Information about a known peer in the channel.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub endpoint_id: EndpointId,
    pub display_name: String,
    pub capabilities: MediaCapabilities,
    pub last_heartbeat: Instant,
}

/// Events emitted by the channel to the application layer.
#[derive(Debug, Clone)]
pub enum ChannelEvent {
    /// A new peer has joined the channel.
    PeerJoined(PeerInfo),
    /// A direct QUIC connection has been established to a peer.
    PeerConnected(EndpointId),
    /// A peer has left the channel (explicit leave or timeout).
    PeerLeft(EndpointId),
    /// A peer's media state has changed.
    MediaStateChanged {
        endpoint_id: EndpointId,
        audio_muted: bool,
        video_enabled: bool,
        screen_sharing: bool,
        app_audio_sharing: bool,
    },
    /// A file has been announced by a peer.
    FileAnnounced {
        endpoint_id: EndpointId,
        file_hash: [u8; 32],
        file_name: String,
        file_size: u64,
    },
}

/// A channel represents a group of peers communicating via gossip.
#[allow(dead_code)]
pub struct Channel {
    secret: [u8; 32],
    topic_id: TopicId,
    our_endpoint_id: EndpointId,
    our_iroh_id: iroh::EndpointId,
    display_name: String,
    peers: Arc<RwLock<HashMap<EndpointId, PeerInfo>>>,
    connections: Arc<RwLock<HashMap<EndpointId, Arc<PeerConnection>>>>,
    event_rx: Option<mpsc::UnboundedReceiver<ChannelEvent>>,
    shutdown_tx: watch::Sender<bool>,
    endpoint: iroh::Endpoint,
    /// Command channel for broadcasting messages via the event loop.
    broadcast_cmd_tx: mpsc::UnboundedSender<ChannelMessage>,
}

impl Channel {
    /// Create a new channel with a random secret.
    ///
    /// Returns the channel and a ticket for others to join.
    /// Pass `incoming_rx` from `MediaProtocol::new()` to receive incoming connections.
    pub async fn create(
        endpoint: &iroh::Endpoint,
        gossip: &GossipHandle,
        display_name: String,
        incoming_rx: Option<mpsc::UnboundedReceiver<iroh::endpoint::Connection>>,
    ) -> Result<(Self, BiscTicket)> {
        let secret: [u8; 32] = rand::random();
        let topic = bisc_protocol::ticket::topic_from_secret(&secret);
        let iroh_topic = TopicId::from_bytes(topic.0);

        tracing::info!(?iroh_topic, "creating channel");

        let sub = gossip.subscribe(iroh_topic, vec![]).await?;

        let our_iroh_id = endpoint.id();
        let our_endpoint_id = EndpointId(*our_iroh_id.as_bytes());

        // Broadcast our announce
        let announce = ChannelMessage::PeerAnnounce {
            endpoint_id: our_endpoint_id,
            display_name: display_name.clone(),
            capabilities: MediaCapabilities {
                audio: true,
                video: true,
                screen_share: true,
            },
        };
        sub.broadcast(&announce).await?;

        let ticket = Self::make_ticket(&secret, endpoint);

        let channel = Self::spawn_event_loop(
            secret,
            iroh_topic,
            our_endpoint_id,
            our_iroh_id,
            display_name,
            sub,
            endpoint.clone(),
            incoming_rx,
        );

        Ok((channel, ticket))
    }

    /// Join an existing channel using a ticket.
    /// Pass `incoming_rx` from `MediaProtocol::new()` to receive incoming connections.
    pub async fn join(
        endpoint: &iroh::Endpoint,
        gossip: &GossipHandle,
        ticket: &BiscTicket,
        display_name: String,
        incoming_rx: Option<mpsc::UnboundedReceiver<iroh::endpoint::Connection>>,
    ) -> Result<Self> {
        let topic = bisc_protocol::ticket::topic_from_secret(&ticket.channel_secret);
        let iroh_topic = TopicId::from_bytes(topic.0);

        // Convert bootstrap addresses from ticket to iroh EndpointIds
        let bootstrap: Vec<iroh::EndpointId> = ticket
            .bootstrap_addrs
            .iter()
            .filter_map(|addr| iroh::EndpointId::from_bytes(&addr.id.0).ok())
            .collect();

        // Register bootstrap peer addresses with iroh so it can reach them
        // without relay servers (critical for direct localhost connections in tests
        // and self-hosted deployments).
        let memory_lookup = MemoryLookup::new();
        for addr_info in &ticket.bootstrap_addrs {
            if let Ok(peer_id) = iroh::EndpointId::from_bytes(&addr_info.id.0) {
                let mut transport_addrs = Vec::new();
                for addr_str in &addr_info.addrs {
                    if let Ok(socket_addr) = addr_str.parse::<std::net::SocketAddr>() {
                        transport_addrs.push(iroh::TransportAddr::Ip(socket_addr));
                    }
                }
                if let Some(ref relay) = addr_info.relay_url {
                    if let Ok(url) = relay.parse() {
                        transport_addrs.push(iroh::TransportAddr::Relay(url));
                    }
                }
                if !transport_addrs.is_empty() {
                    let iroh_addr = iroh::EndpointAddr {
                        id: peer_id,
                        addrs: transport_addrs.into_iter().collect(),
                    };
                    memory_lookup.add_endpoint_info(iroh_addr);
                    tracing::debug!(peer = %peer_id, "registered bootstrap peer address");
                }
            }
        }
        endpoint.address_lookup().add(memory_lookup);

        tracing::info!(
            ?iroh_topic,
            bootstrap_count = bootstrap.len(),
            "joining channel"
        );

        let sub = gossip.subscribe_and_join(iroh_topic, bootstrap).await?;

        let our_iroh_id = endpoint.id();
        let our_endpoint_id = EndpointId(*our_iroh_id.as_bytes());

        // Broadcast our announce
        let announce = ChannelMessage::PeerAnnounce {
            endpoint_id: our_endpoint_id,
            display_name: display_name.clone(),
            capabilities: MediaCapabilities {
                audio: true,
                video: true,
                screen_share: true,
            },
        };
        sub.broadcast(&announce).await?;

        let channel = Self::spawn_event_loop(
            ticket.channel_secret,
            iroh_topic,
            our_endpoint_id,
            our_iroh_id,
            display_name,
            sub,
            endpoint.clone(),
            incoming_rx,
        );

        Ok(channel)
    }

    /// Generate a refreshed ticket with this peer's address as bootstrap.
    pub fn refresh_ticket(&self) -> BiscTicket {
        Self::make_ticket(&self.secret, &self.endpoint)
    }

    /// Get a snapshot of the current peer list.
    pub async fn peers(&self) -> HashMap<EndpointId, PeerInfo> {
        let peers = self.peers.read().await.clone();
        tracing::trace!(count = peers.len(), "peers() called");
        peers
    }

    /// Take the event receiver out of the channel.
    ///
    /// Returns `None` if the receiver has already been taken. Once taken,
    /// `recv_event()` will pend forever — the caller owns the receiver.
    pub fn take_event_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<ChannelEvent>> {
        self.event_rx.take()
    }

    /// Receive the next channel event.
    ///
    /// Returns `None` (pends forever) if the receiver was already taken
    /// via `take_event_receiver()`.
    pub async fn recv_event(&mut self) -> Option<ChannelEvent> {
        match &mut self.event_rx {
            Some(rx) => rx.recv().await,
            None => std::future::pending().await,
        }
    }

    /// Get a direct connection to a specific peer, if established.
    pub async fn connection(&self, peer_id: &EndpointId) -> Option<Arc<PeerConnection>> {
        self.connections.read().await.get(peer_id).cloned()
    }

    /// Get our own endpoint ID in this channel.
    pub fn our_endpoint_id(&self) -> EndpointId {
        self.our_endpoint_id
    }

    /// Signal the event loop to shut down (leave the channel).
    pub fn leave(&self) {
        tracing::info!("leaving channel");
        let _ = self.shutdown_tx.send(true);
    }

    /// Get the underlying QUIC connection to a peer (non-async).
    ///
    /// Returns `None` if the peer has no connection or if the lock is
    /// currently held by a writer.
    pub fn peer_quic_connection(&self, peer_id: &EndpointId) -> Option<iroh::endpoint::Connection> {
        self.connections
            .try_read()
            .ok()?
            .get(peer_id)
            .map(|pc| pc.connection().clone())
    }

    /// Broadcast a channel message via gossip.
    ///
    /// The message is sent to the event loop which performs the actual broadcast.
    pub fn broadcast_message(&self, msg: ChannelMessage) {
        if self.broadcast_cmd_tx.send(msg).is_err() {
            tracing::warn!("broadcast command channel closed");
        }
    }

    fn make_ticket(secret: &[u8; 32], endpoint: &iroh::Endpoint) -> BiscTicket {
        let iroh_addr = endpoint.addr();
        let mut addrs = Vec::new();
        let mut relay_url = None;
        for addr in &iroh_addr.addrs {
            match addr {
                iroh::TransportAddr::Ip(sock) => addrs.push(sock.to_string()),
                iroh::TransportAddr::Relay(url) => relay_url = Some(url.to_string()),
                _ => {} // Future transport types
            }
        }
        BiscTicket {
            channel_secret: *secret,
            bootstrap_addrs: vec![EndpointAddr {
                id: EndpointId(*iroh_addr.id.as_bytes()),
                addrs,
                relay_url,
            }],
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_event_loop(
        secret: [u8; 32],
        topic_id: TopicId,
        our_endpoint_id: EndpointId,
        our_iroh_id: iroh::EndpointId,
        display_name: String,
        sub: TopicSubscription,
        endpoint: iroh::Endpoint,
        incoming_rx: Option<mpsc::UnboundedReceiver<iroh::endpoint::Connection>>,
    ) -> Self {
        let peers: Arc<RwLock<HashMap<EndpointId, PeerInfo>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let connections: Arc<RwLock<HashMap<EndpointId, Arc<PeerConnection>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (broadcast_cmd_tx, broadcast_cmd_rx) = mpsc::unbounded_channel();

        let peers_clone = peers.clone();
        let connections_clone = connections.clone();

        tokio::spawn(Self::event_loop(
            our_endpoint_id,
            display_name.clone(),
            sub,
            peers_clone,
            connections_clone,
            event_tx,
            shutdown_rx,
            endpoint.clone(),
            incoming_rx,
            broadcast_cmd_rx,
        ));

        Self {
            secret,
            topic_id,
            our_endpoint_id,
            our_iroh_id,
            display_name,
            peers,
            connections,
            event_rx: Some(event_rx),
            shutdown_tx,
            endpoint,
            broadcast_cmd_tx,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn event_loop(
        our_id: EndpointId,
        display_name: String,
        sub: TopicSubscription,
        peers: Arc<RwLock<HashMap<EndpointId, PeerInfo>>>,
        connections: Arc<RwLock<HashMap<EndpointId, Arc<PeerConnection>>>>,
        event_tx: mpsc::UnboundedSender<ChannelEvent>,
        mut shutdown_rx: watch::Receiver<bool>,
        endpoint: iroh::Endpoint,
        mut incoming_rx: Option<mpsc::UnboundedReceiver<iroh::endpoint::Connection>>,
        mut broadcast_cmd_rx: mpsc::UnboundedReceiver<ChannelMessage>,
    ) {
        let (sender, mut receiver) = sub.into_parts();
        let mut heartbeat_interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        let mut timeout_check_interval = tokio::time::interval(Duration::from_secs(3));

        tracing::debug!("channel event loop started");

        loop {
            tokio::select! {
                biased;

                event = n0_future::StreamExt::try_next(&mut receiver) => {
                    match event {
                        Ok(Some(raw_event)) => {
                            let gossip_event: GossipEvent = raw_event.into();
                            Self::handle_gossip_event(
                                gossip_event,
                                our_id,
                                &display_name,
                                &sender,
                                &peers,
                                &connections,
                                &event_tx,
                                &endpoint,
                            ).await;
                        }
                        Ok(None) => {
                            tracing::info!("gossip subscription ended");
                            break;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "gossip receive error");
                        }
                    }
                }

                // Accept incoming media connections from the MediaProtocol handler
                Some(conn) = async {
                    if let Some(ref mut rx) = incoming_rx {
                        rx.recv().await
                    } else {
                        // No incoming receiver, pend forever
                        std::future::pending::<Option<iroh::endpoint::Connection>>().await
                    }
                } => {
                    let remote_iroh_id = conn.remote_id();
                    let peer_endpoint_id = EndpointId(*remote_iroh_id.as_bytes());

                    if peers.read().await.contains_key(&peer_endpoint_id) {
                        tracing::info!(
                            peer = ?peer_endpoint_id,
                            "accepted direct connection from known peer"
                        );
                        let peer_conn = Arc::new(PeerConnection::from_connection(conn));
                        connections.write().await.insert(peer_endpoint_id, peer_conn);
                        let _ = event_tx.send(ChannelEvent::PeerConnected(peer_endpoint_id));
                    } else {
                        tracing::debug!(
                            peer = ?peer_endpoint_id,
                            "incoming connection from unknown peer, closing"
                        );
                        conn.close(1u8.into(), b"unknown peer");
                    }
                }

                // Handle broadcast commands from the application layer
                Some(msg) = broadcast_cmd_rx.recv() => {
                    let data = bisc_protocol::channel::encode_channel_message(&msg)
                        .unwrap_or_default();
                    if let Err(e) = sender.broadcast(data.into()).await {
                        tracing::warn!(error = %e, "failed to broadcast command message");
                    }
                }

                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        // Close all peer connections
                        let conns = connections.read().await;
                        for (id, conn) in conns.iter() {
                            tracing::info!(peer = ?id, "closing connection on leave");
                            conn.close();
                        }
                        drop(conns);

                        let leave_msg = ChannelMessage::PeerLeave {
                            endpoint_id: our_id,
                        };
                        let data = bisc_protocol::channel::encode_channel_message(&leave_msg)
                            .unwrap_or_default();
                        let _ = sender.broadcast(data.into()).await;
                        tracing::info!("channel event loop shutting down");
                        break;
                    }
                }

                _ = heartbeat_interval.tick() => {
                    let heartbeat = ChannelMessage::Heartbeat {
                        endpoint_id: our_id,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    };
                    let data = bisc_protocol::channel::encode_channel_message(&heartbeat)
                        .unwrap_or_default();
                    if let Err(e) = sender.broadcast(data.into()).await {
                        tracing::warn!(error = %e, "failed to send heartbeat");
                    }
                }

                _ = timeout_check_interval.tick() => {
                    let now = Instant::now();
                    let mut peers_write = peers.write().await;
                    let timed_out: Vec<EndpointId> = peers_write
                        .iter()
                        .filter(|(_, info)| now.duration_since(info.last_heartbeat) > PEER_TIMEOUT)
                        .map(|(id, _)| *id)
                        .collect();
                    for id in timed_out {
                        tracing::info!(peer = ?id, "peer timed out");
                        peers_write.remove(&id);
                        // Close connection for timed-out peer
                        if let Some(conn) = connections.write().await.remove(&id) {
                            conn.close();
                        }
                        let _ = event_tx.send(ChannelEvent::PeerLeft(id));
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_gossip_event(
        event: GossipEvent,
        our_id: EndpointId,
        our_display_name: &str,
        sender: &iroh_gossip::api::GossipSender,
        peers: &RwLock<HashMap<EndpointId, PeerInfo>>,
        connections: &Arc<RwLock<HashMap<EndpointId, Arc<PeerConnection>>>>,
        event_tx: &mpsc::UnboundedSender<ChannelEvent>,
        endpoint: &iroh::Endpoint,
    ) {
        match event {
            GossipEvent::Message { message, .. } => match message {
                ChannelMessage::PeerAnnounce {
                    endpoint_id,
                    display_name,
                    capabilities,
                } => {
                    if endpoint_id == our_id {
                        return;
                    }
                    tracing::info!(peer = ?endpoint_id, name = %display_name, "peer announced");
                    let info = PeerInfo {
                        endpoint_id,
                        display_name,
                        capabilities,
                        last_heartbeat: Instant::now(),
                    };
                    let is_new = !peers.read().await.contains_key(&endpoint_id);
                    peers.write().await.insert(endpoint_id, info.clone());
                    let _ = event_tx.send(ChannelEvent::PeerJoined(info));

                    // Only connect if this is a new peer, we don't already have a connection,
                    // and our ID is higher (to avoid duplicate connections from both sides).
                    // Spawn as a background task to avoid blocking the event loop.
                    if is_new
                        && !connections.read().await.contains_key(&endpoint_id)
                        && our_id.0 > endpoint_id.0
                    {
                        let ep = endpoint.clone();
                        let conns = connections.clone();
                        let tx = event_tx.clone();
                        tokio::spawn(async move {
                            Self::establish_connection(&ep, endpoint_id, &conns, &tx).await;
                        });
                    }
                }
                ChannelMessage::PeerLeave { endpoint_id } => {
                    if endpoint_id == our_id {
                        return;
                    }
                    tracing::info!(peer = ?endpoint_id, "peer left");
                    peers.write().await.remove(&endpoint_id);
                    // Close and remove the connection
                    if let Some(conn) = connections.write().await.remove(&endpoint_id) {
                        conn.close();
                    }
                    let _ = event_tx.send(ChannelEvent::PeerLeft(endpoint_id));
                }
                ChannelMessage::Heartbeat {
                    endpoint_id,
                    timestamp: _,
                } => {
                    if endpoint_id == our_id {
                        return;
                    }
                    if let Some(info) = peers.write().await.get_mut(&endpoint_id) {
                        info.last_heartbeat = Instant::now();
                        tracing::trace!(peer = ?endpoint_id, "heartbeat received");
                    }
                }
                ChannelMessage::MediaStateUpdate {
                    endpoint_id,
                    audio_muted,
                    video_enabled,
                    screen_sharing,
                    app_audio_sharing,
                } => {
                    if endpoint_id == our_id {
                        return;
                    }
                    let _ = event_tx.send(ChannelEvent::MediaStateChanged {
                        endpoint_id,
                        audio_muted,
                        video_enabled,
                        screen_sharing,
                        app_audio_sharing,
                    });
                }
                ChannelMessage::FileAnnounce {
                    endpoint_id,
                    file_hash,
                    file_name,
                    file_size,
                } => {
                    if endpoint_id == our_id {
                        return;
                    }
                    let _ = event_tx.send(ChannelEvent::FileAnnounced {
                        endpoint_id,
                        file_hash,
                        file_name,
                        file_size,
                    });
                }
                ChannelMessage::TicketRefresh { .. } => {
                    // Ticket refresh is informational; doesn't affect our peer map
                    tracing::debug!("received ticket refresh");
                }
            },
            GossipEvent::InvalidMessage { from, error } => {
                tracing::warn!(from = %from, error = %error, "received invalid message");
            }
            GossipEvent::NeighborUp(_peer_id) => {
                // Re-announce ourselves so the new neighbor learns about us
                let announce = ChannelMessage::PeerAnnounce {
                    endpoint_id: our_id,
                    display_name: our_display_name.to_string(),
                    capabilities: MediaCapabilities {
                        audio: true,
                        video: true,
                        screen_share: true,
                    },
                };
                let data =
                    bisc_protocol::channel::encode_channel_message(&announce).unwrap_or_default();
                if let Err(e) = sender.broadcast(data.into()).await {
                    tracing::warn!(error = %e, "failed to re-announce on neighbor up");
                }
            }
            GossipEvent::NeighborDown(_) | GossipEvent::Lagged => {}
        }
    }

    /// Establish a direct QUIC connection to a peer.
    async fn establish_connection(
        endpoint: &iroh::Endpoint,
        peer_endpoint_id: EndpointId,
        connections: &Arc<RwLock<HashMap<EndpointId, Arc<PeerConnection>>>>,
        event_tx: &mpsc::UnboundedSender<ChannelEvent>,
    ) {
        let remote_iroh_id = match iroh::EndpointId::from_bytes(&peer_endpoint_id.0) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(error = %e, "invalid peer endpoint id for connection");
                return;
            }
        };

        match PeerConnection::connect(endpoint, remote_iroh_id, crate::endpoint::MEDIA_ALPN).await {
            Ok(conn) => {
                tracing::info!(peer = ?peer_endpoint_id, "direct connection established");
                let conn = Arc::new(conn);
                connections.write().await.insert(peer_endpoint_id, conn);
                let _ = event_tx.send(ChannelEvent::PeerConnected(peer_endpoint_id));
            }
            Err(e) => {
                tracing::warn!(
                    peer = ?peer_endpoint_id,
                    error = %e,
                    "failed to establish direct connection"
                );
            }
        }
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        // Signal shutdown when channel is dropped
        let _ = self.shutdown_tx.send(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bisc_protocol::ticket::BiscTicket;

    #[test]
    fn create_produces_valid_ticket() {
        // We can verify ticket structure without async
        let secret = [0x42u8; 32];
        let addr = EndpointAddr {
            id: EndpointId([0x01; 32]),
            addrs: vec!["127.0.0.1:12345".to_string()],
            relay_url: None,
        };
        let ticket = BiscTicket {
            channel_secret: secret,
            bootstrap_addrs: vec![addr],
        };
        let s = ticket.to_ticket_string();
        let parsed: BiscTicket = s.parse().unwrap();
        assert_eq!(ticket, parsed);
        assert_eq!(parsed.channel_secret, secret);
    }
}
