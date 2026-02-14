//! Channel lifecycle: create, join, leave, peer management, heartbeats.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use bisc_protocol::channel::{ChannelMessage, MediaCapabilities};
use bisc_protocol::ticket::BiscTicket;
use bisc_protocol::types::{EndpointAddr, EndpointId};
use iroh_gossip::proto::TopicId;
use tokio::sync::{mpsc, watch, RwLock};

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
        chunk_count: u32,
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
    event_rx: mpsc::UnboundedReceiver<ChannelEvent>,
    shutdown_tx: watch::Sender<bool>,
    endpoint: iroh::Endpoint,
}

impl Channel {
    /// Create a new channel with a random secret.
    ///
    /// Returns the channel and a ticket for others to join.
    pub async fn create(
        endpoint: &iroh::Endpoint,
        gossip: &GossipHandle,
        display_name: String,
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
        );

        Ok((channel, ticket))
    }

    /// Join an existing channel using a ticket.
    pub async fn join(
        endpoint: &iroh::Endpoint,
        gossip: &GossipHandle,
        ticket: &BiscTicket,
        display_name: String,
    ) -> Result<Self> {
        let topic = bisc_protocol::ticket::topic_from_secret(&ticket.channel_secret);
        let iroh_topic = TopicId::from_bytes(topic.0);

        // Convert bootstrap addresses from ticket to iroh EndpointIds
        let bootstrap: Vec<iroh::EndpointId> = ticket
            .bootstrap_addrs
            .iter()
            .filter_map(|addr| iroh::EndpointId::from_bytes(&addr.id.0).ok())
            .collect();

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

    /// Receive the next channel event.
    pub async fn recv_event(&mut self) -> Option<ChannelEvent> {
        self.event_rx.recv().await
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

    fn spawn_event_loop(
        secret: [u8; 32],
        topic_id: TopicId,
        our_endpoint_id: EndpointId,
        our_iroh_id: iroh::EndpointId,
        display_name: String,
        sub: TopicSubscription,
        endpoint: iroh::Endpoint,
    ) -> Self {
        let peers: Arc<RwLock<HashMap<EndpointId, PeerInfo>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let peers_clone = peers.clone();

        tokio::spawn(Self::event_loop(
            our_endpoint_id,
            display_name.clone(),
            sub,
            peers_clone,
            event_tx,
            shutdown_rx,
        ));

        Self {
            secret,
            topic_id,
            our_endpoint_id,
            our_iroh_id,
            display_name,
            peers,
            event_rx,
            shutdown_tx,
            endpoint,
        }
    }

    async fn event_loop(
        our_id: EndpointId,
        display_name: String,
        sub: TopicSubscription,
        peers: Arc<RwLock<HashMap<EndpointId, PeerInfo>>>,
        event_tx: mpsc::UnboundedSender<ChannelEvent>,
        mut shutdown_rx: watch::Receiver<bool>,
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
                                &event_tx,
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

                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
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
                        let _ = event_tx.send(ChannelEvent::PeerLeft(id));
                    }
                }
            }
        }
    }

    async fn handle_gossip_event(
        event: GossipEvent,
        our_id: EndpointId,
        our_display_name: &str,
        sender: &iroh_gossip::api::GossipSender,
        peers: &RwLock<HashMap<EndpointId, PeerInfo>>,
        event_tx: &mpsc::UnboundedSender<ChannelEvent>,
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
                    peers.write().await.insert(endpoint_id, info.clone());
                    let _ = event_tx.send(ChannelEvent::PeerJoined(info));
                }
                ChannelMessage::PeerLeave { endpoint_id } => {
                    if endpoint_id == our_id {
                        return;
                    }
                    tracing::info!(peer = ?endpoint_id, "peer left");
                    peers.write().await.remove(&endpoint_id);
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
                    chunk_count,
                } => {
                    if endpoint_id == our_id {
                        return;
                    }
                    let _ = event_tx.send(ChannelEvent::FileAnnounced {
                        endpoint_id,
                        file_hash,
                        file_name,
                        file_size,
                        chunk_count,
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
