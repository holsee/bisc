//! GossipHandle wrapping `iroh_gossip::net::Gossip` for channel messaging.

use anyhow::Result;
use bisc_protocol::channel::ChannelMessage;
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointId};
use iroh_gossip::net::Gossip;
use iroh_gossip::proto::TopicId;
use n0_future::StreamExt;

/// Handle for the gossip subsystem, managing topic subscriptions.
#[derive(Debug, Clone)]
pub struct GossipHandle {
    gossip: Gossip,
}

/// A subscription to a gossip topic with send/receive capabilities.
pub struct TopicSubscription {
    sender: iroh_gossip::api::GossipSender,
    receiver: iroh_gossip::api::GossipReceiver,
}

impl GossipHandle {
    /// Create a new gossip handle and register it with a router.
    ///
    /// Returns both the gossip handle and the router (which must be kept alive).
    pub fn new(endpoint: &Endpoint) -> (Self, Router) {
        let gossip = Gossip::builder().spawn(endpoint.clone());
        let router = Router::builder(endpoint.clone())
            .accept(iroh_gossip::ALPN, gossip.clone())
            .spawn();

        tracing::info!("gossip subsystem initialized");

        (Self { gossip }, router)
    }

    /// Get the underlying gossip instance.
    pub fn gossip(&self) -> &Gossip {
        &self.gossip
    }

    /// Subscribe to a topic, optionally bootstrapping from known peers.
    pub async fn subscribe(
        &self,
        topic: TopicId,
        bootstrap_peers: Vec<EndpointId>,
    ) -> Result<TopicSubscription> {
        tracing::debug!(
            ?topic,
            peers = bootstrap_peers.len(),
            "subscribing to topic"
        );

        let topic_handle = self.gossip.subscribe(topic, bootstrap_peers).await?;
        let (sender, receiver) = topic_handle.split();

        Ok(TopicSubscription { sender, receiver })
    }

    /// Subscribe to a topic and wait until at least one peer is connected.
    pub async fn subscribe_and_join(
        &self,
        topic: TopicId,
        bootstrap_peers: Vec<EndpointId>,
    ) -> Result<TopicSubscription> {
        tracing::debug!(
            ?topic,
            peers = bootstrap_peers.len(),
            "subscribing and joining topic"
        );

        let topic_handle = self
            .gossip
            .subscribe_and_join(topic, bootstrap_peers)
            .await?;
        let (sender, receiver) = topic_handle.split();

        tracing::info!(?topic, "joined topic");

        Ok(TopicSubscription { sender, receiver })
    }
}

impl TopicSubscription {
    /// Broadcast a `ChannelMessage` to all peers on this topic.
    pub async fn broadcast(&self, msg: &ChannelMessage) -> Result<()> {
        let data = bisc_protocol::channel::encode_channel_message(msg)?;
        self.sender.broadcast(data.into()).await?;
        tracing::debug!("broadcast channel message");
        Ok(())
    }

    /// Receive the next event from this topic subscription.
    ///
    /// Returns `None` when the subscription is closed.
    pub async fn recv(&mut self) -> Option<Result<GossipEvent>> {
        match self.receiver.try_next().await {
            Ok(Some(event)) => Some(Ok(event.into())),
            Ok(None) => None,
            Err(e) => Some(Err(e.into())),
        }
    }

    /// Get a reference to the sender for cloning.
    pub fn sender(&self) -> &iroh_gossip::api::GossipSender {
        &self.sender
    }
}

/// Events received from a gossip topic subscription.
#[derive(Debug)]
pub enum GossipEvent {
    /// A channel message was received.
    Message {
        from: EndpointId,
        message: ChannelMessage,
    },
    /// A message was received but could not be decoded.
    InvalidMessage { from: EndpointId, error: String },
    /// A new neighbor peer connected on this topic.
    NeighborUp(EndpointId),
    /// A neighbor peer disconnected from this topic.
    NeighborDown(EndpointId),
    /// The receiver fell behind and some messages were lost.
    Lagged,
}

impl From<iroh_gossip::api::Event> for GossipEvent {
    fn from(event: iroh_gossip::api::Event) -> Self {
        match event {
            iroh_gossip::api::Event::Received(msg) => {
                match bisc_protocol::channel::decode_channel_message(&msg.content) {
                    Ok(channel_msg) => GossipEvent::Message {
                        from: msg.delivered_from,
                        message: channel_msg,
                    },
                    Err(e) => GossipEvent::InvalidMessage {
                        from: msg.delivered_from,
                        error: e.to_string(),
                    },
                }
            }
            iroh_gossip::api::Event::NeighborUp(id) => {
                tracing::debug!(peer = %id, "neighbor up");
                GossipEvent::NeighborUp(id)
            }
            iroh_gossip::api::Event::NeighborDown(id) => {
                tracing::debug!(peer = %id, "neighbor down");
                GossipEvent::NeighborDown(id)
            }
            iroh_gossip::api::Event::Lagged => {
                tracing::warn!("gossip receiver lagged, messages lost");
                GossipEvent::Lagged
            }
        }
    }
}
