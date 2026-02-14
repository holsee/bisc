//! Simulated peer wrapping a [`SimTransport`].

use anyhow::Result;
use bytes::Bytes;
use std::sync::atomic::Ordering;
use tokio::io::DuplexStream;

use crate::transport::{Transport, TransportMetrics};

use super::network::SimPeerId;
use super::transport::SimTransport;

/// Type alias — peer metrics are the transport metrics.
pub type PeerMetrics = TransportMetrics;

/// A simulated peer with the same interface as `PeerConnection`.
pub struct SimPeer {
    id: SimPeerId,
    name: String,
    pub(crate) transport: SimTransport,
}

impl SimPeer {
    pub(crate) fn new(id: SimPeerId, name: String, transport: SimTransport) -> Self {
        Self {
            id,
            name,
            transport,
        }
    }

    /// Get the peer's identifier.
    pub fn id(&self) -> SimPeerId {
        self.id
    }

    /// Get the peer's display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Send an unreliable datagram to the connected peer.
    pub async fn send_datagram(&self, data: Bytes) -> Result<()> {
        self.transport.send_datagram(data).await
    }

    /// Receive an unreliable datagram from the connected peer.
    pub async fn recv_datagram(&self) -> Result<Bytes> {
        self.transport.recv_datagram().await
    }

    /// Open a reliable bidirectional stream.
    pub async fn open_bi_stream(&self) -> Result<(DuplexStream, DuplexStream)> {
        self.transport.open_bi_stream().await
    }

    /// Accept a reliable bidirectional stream opened by the remote peer.
    pub async fn accept_bi_stream(&self) -> Result<(DuplexStream, DuplexStream)> {
        self.transport.accept_bi_stream().await
    }

    /// Get peer metrics (datagrams sent/received/lost, streams, bytes).
    pub fn metrics(&self) -> &PeerMetrics {
        self.transport.transport_metrics()
    }

    /// Get the number of datagrams sent.
    pub fn datagrams_sent(&self) -> u64 {
        self.metrics().datagrams_sent.load(Ordering::Relaxed)
    }

    /// Get the number of datagrams received.
    pub fn datagrams_received(&self) -> u64 {
        self.metrics().datagrams_received.load(Ordering::Relaxed)
    }

    /// Get the number of datagrams dropped (by the network).
    pub fn datagrams_dropped(&self) -> u64 {
        self.metrics().datagrams_dropped.load(Ordering::Relaxed)
    }

    /// Get the underlying transport.
    pub fn transport(&self) -> &SimTransport {
        &self.transport
    }
}
