//! Simulation network controller.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use bisc_protocol::ticket::BiscTicket;
use bytes::Bytes;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::io::DuplexStream;
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::transport::TransportMetrics;

use super::peer::SimPeer;
use super::transport::SimTransport;

/// Unique identifier for a simulated peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SimPeerId(pub u64);

/// Configuration for a single network link direction.
#[derive(Debug, Clone, Default)]
struct LinkConfig {
    latency: Option<Duration>,
    loss_rate: Option<f64>,
    jitter: Option<Duration>,
}

/// Internal delivery channels for a transport endpoint.
struct DeliveryEndpoint {
    datagram_tx: mpsc::UnboundedSender<Bytes>,
    stream_tx: mpsc::UnboundedSender<(DuplexStream, DuplexStream)>,
}

/// Shared interior of the simulation network.
pub(crate) struct SimNetworkInner {
    /// Per-link configuration overrides.
    link_configs: RwLock<HashMap<(SimPeerId, SimPeerId), LinkConfig>>,
    /// Default one-way latency.
    default_latency: RwLock<Duration>,
    /// Default packet loss rate (0.0–1.0).
    default_loss_rate: RwLock<f64>,
    /// Default maximum jitter.
    default_jitter: RwLock<Duration>,
    /// Set of disconnected peers.
    disconnected: RwLock<HashSet<SimPeerId>>,
    /// Delivery channels keyed by (local_peer, remote_peer).
    delivery: RwLock<HashMap<(SimPeerId, SimPeerId), DeliveryEndpoint>>,
    /// Seeded RNG for deterministic loss/jitter.
    rng: Mutex<StdRng>,
}

impl SimNetworkInner {
    /// Route a datagram from `from` to `to`, applying loss, latency, and jitter.
    pub(crate) async fn route_datagram(
        &self,
        from: SimPeerId,
        to: SimPeerId,
        data: Bytes,
        sender_metrics: &TransportMetrics,
    ) -> Result<()> {
        // Check disconnected
        {
            let disconnected = self.disconnected.read().await;
            if disconnected.contains(&from) || disconnected.contains(&to) {
                sender_metrics.record_datagram_dropped();
                tracing::trace!(
                    from = from.0,
                    to = to.0,
                    "datagram dropped: peer disconnected"
                );
                return Ok(());
            }
        }

        // Get effective link config
        let (latency, loss_rate, jitter) = self.effective_link_config(from, to).await;

        // Apply loss
        let should_drop = if loss_rate > 0.0 {
            let mut rng = self.rng.lock().await;
            rng.random_bool(loss_rate.clamp(0.0, 1.0))
        } else {
            false
        };

        if should_drop {
            sender_metrics.record_datagram_dropped();
            tracing::trace!(
                from = from.0,
                to = to.0,
                "datagram dropped by loss simulation"
            );
            return Ok(());
        }

        // Record send on sender side
        sender_metrics.record_datagram_sent(data.len());

        // Calculate total delay
        let delay = if jitter > Duration::ZERO {
            let mut rng = self.rng.lock().await;
            let jitter_ns = rng.random_range(0..jitter.as_nanos() as u64);
            latency + Duration::from_nanos(jitter_ns)
        } else {
            latency
        };

        // Get delivery channel: delivering to the transport at (to, from)
        let tx = {
            let delivery = self.delivery.read().await;
            let endpoint = delivery
                .get(&(to, from))
                .context("no delivery channel for peer pair")?;
            endpoint.datagram_tx.clone()
        };

        // Schedule delivery
        if delay > Duration::ZERO {
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = tx.send(data);
            });
        } else {
            let _ = tx.send(data);
        }

        Ok(())
    }

    /// Deliver a stream pair to the remote peer's accept queue.
    pub(crate) async fn deliver_stream(
        &self,
        from: SimPeerId,
        to: SimPeerId,
        stream: (DuplexStream, DuplexStream),
    ) -> Result<()> {
        let delivery = self.delivery.read().await;
        let endpoint = delivery
            .get(&(to, from))
            .context("no delivery channel for peer pair (stream)")?;
        endpoint
            .stream_tx
            .send(stream)
            .map_err(|_| anyhow::anyhow!("stream delivery channel closed"))?;
        Ok(())
    }

    /// Get the effective latency, loss rate, and jitter for a link.
    async fn effective_link_config(
        &self,
        from: SimPeerId,
        to: SimPeerId,
    ) -> (Duration, f64, Duration) {
        let configs = self.link_configs.read().await;
        let config = configs.get(&(from, to));
        let default_latency = *self.default_latency.read().await;
        let default_loss_rate = *self.default_loss_rate.read().await;
        let default_jitter = *self.default_jitter.read().await;

        (
            config.and_then(|c| c.latency).unwrap_or(default_latency),
            config
                .and_then(|c| c.loss_rate)
                .unwrap_or(default_loss_rate),
            config.and_then(|c| c.jitter).unwrap_or(default_jitter),
        )
    }
}

/// Simulation network controller.
///
/// Creates simulated peers, manages connections between them, and controls
/// network conditions (latency, loss, jitter, disconnection).
pub struct SimNetwork {
    inner: Arc<SimNetworkInner>,
    next_id: u64,
}

impl SimNetwork {
    /// Create a new simulation network with default seed (42).
    pub fn new() -> Self {
        Self::with_seed(42)
    }

    /// Create a new simulation network with a specific RNG seed.
    pub fn with_seed(seed: u64) -> Self {
        tracing::info!(seed, "created sim network");
        Self {
            inner: Arc::new(SimNetworkInner {
                link_configs: RwLock::new(HashMap::new()),
                default_latency: RwLock::new(Duration::ZERO),
                default_loss_rate: RwLock::new(0.0),
                default_jitter: RwLock::new(Duration::ZERO),
                disconnected: RwLock::new(HashSet::new()),
                delivery: RwLock::new(HashMap::new()),
                rng: Mutex::new(StdRng::seed_from_u64(seed)),
            }),
            next_id: 0,
        }
    }

    /// Create a new peer identity (not yet connected to anyone).
    pub fn create_peer(&mut self, name: &str) -> SimPeerId {
        let id = SimPeerId(self.next_id);
        self.next_id += 1;
        tracing::info!(peer_id = id.0, name, "created sim peer");
        id
    }

    /// Connect two peers, returning a `SimPeer` for each side.
    pub async fn connect(&self, a: SimPeerId, b: SimPeerId) -> (SimPeer, SimPeer) {
        let (a_dgram_tx, a_dgram_rx) = mpsc::unbounded_channel();
        let (b_dgram_tx, b_dgram_rx) = mpsc::unbounded_channel();
        let (a_stream_tx, a_stream_rx) = mpsc::unbounded_channel();
        let (b_stream_tx, b_stream_rx) = mpsc::unbounded_channel();

        // Register delivery endpoints:
        // (a, b) = transport A (local=a, remote=b) receives datagrams here
        // (b, a) = transport B (local=b, remote=a) receives datagrams here
        {
            let mut delivery = self.inner.delivery.write().await;
            delivery.insert(
                (a, b),
                DeliveryEndpoint {
                    datagram_tx: a_dgram_tx,
                    stream_tx: a_stream_tx,
                },
            );
            delivery.insert(
                (b, a),
                DeliveryEndpoint {
                    datagram_tx: b_dgram_tx,
                    stream_tx: b_stream_tx,
                },
            );
        }

        let transport_a = SimTransport {
            local_id: a,
            remote_id: b,
            network: self.inner.clone(),
            datagram_rx: Mutex::new(a_dgram_rx),
            stream_rx: Mutex::new(a_stream_rx),
            metrics: TransportMetrics::new(),
        };

        let transport_b = SimTransport {
            local_id: b,
            remote_id: a,
            network: self.inner.clone(),
            datagram_rx: Mutex::new(b_dgram_rx),
            stream_rx: Mutex::new(b_stream_rx),
            metrics: TransportMetrics::new(),
        };

        tracing::info!(peer_a = a.0, peer_b = b.0, "connected sim peers");

        (
            SimPeer::new(a, format!("peer-{}", a.0), transport_a),
            SimPeer::new(b, format!("peer-{}", b.0), transport_b),
        )
    }

    /// Shorthand: create two peers and connect them, returning a dummy ticket.
    pub async fn create_channel(&mut self) -> (SimPeer, SimPeer, BiscTicket) {
        let a = self.create_peer("peer-a");
        let b = self.create_peer("peer-b");
        let (peer_a, peer_b) = self.connect(a, b).await;
        let ticket = BiscTicket {
            channel_secret: [0u8; 32],
            bootstrap_addrs: vec![],
        };
        (peer_a, peer_b, ticket)
    }

    /// Set the default one-way latency for all links.
    pub async fn set_latency(&self, duration: Duration) {
        *self.inner.default_latency.write().await = duration;
        tracing::debug!(?duration, "set default latency");
    }

    /// Set latency for a specific link (bidirectional).
    pub async fn set_latency_between(&self, a: SimPeerId, b: SimPeerId, duration: Duration) {
        let mut configs = self.inner.link_configs.write().await;
        configs.entry((a, b)).or_default().latency = Some(duration);
        configs.entry((b, a)).or_default().latency = Some(duration);
        tracing::debug!(a = a.0, b = b.0, ?duration, "set link latency");
    }

    /// Set the default packet loss rate for all links (0.0–1.0).
    pub async fn set_loss_rate(&self, rate: f64) {
        *self.inner.default_loss_rate.write().await = rate;
        tracing::debug!(rate, "set default loss rate");
    }

    /// Set loss rate for a specific link (bidirectional).
    pub async fn set_loss_rate_between(&self, a: SimPeerId, b: SimPeerId, rate: f64) {
        let mut configs = self.inner.link_configs.write().await;
        configs.entry((a, b)).or_default().loss_rate = Some(rate);
        configs.entry((b, a)).or_default().loss_rate = Some(rate);
        tracing::debug!(a = a.0, b = b.0, rate, "set link loss rate");
    }

    /// Set the default maximum jitter for all links.
    pub async fn set_jitter(&self, max_jitter: Duration) {
        *self.inner.default_jitter.write().await = max_jitter;
        tracing::debug!(?max_jitter, "set default jitter");
    }

    /// Simulate a peer going offline (all datagrams to/from are dropped).
    pub async fn disconnect(&self, peer: SimPeerId) {
        self.inner.disconnected.write().await.insert(peer);
        tracing::info!(peer = peer.0, "sim peer disconnected");
    }

    /// Simulate a peer coming back online.
    pub async fn reconnect(&self, peer: SimPeerId) {
        self.inner.disconnected.write().await.remove(&peer);
        tracing::info!(peer = peer.0, "sim peer reconnected");
    }

    /// Advance simulated time.
    ///
    /// When used with `tokio::time::pause()`, this advances the simulated clock,
    /// causing pending `sleep` futures (used for latency simulation) to resolve.
    pub async fn advance(&self, duration: Duration) {
        tokio::time::advance(duration).await;
    }
}

impl Default for SimNetwork {
    fn default() -> Self {
        Self::new()
    }
}
