//! Abstract transport trait for peer-to-peer communication.
//!
//! Defines the [`Transport`] trait implemented by both real iroh connections
//! ([`PeerConnection`](crate::PeerConnection)) and simulated connections
//! ([`SimTransport`](crate::sim::SimTransport)) for testing.

use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use bytes::Bytes;

/// Metrics tracked by a transport implementation.
pub struct TransportMetrics {
    pub datagrams_sent: AtomicU64,
    pub datagrams_received: AtomicU64,
    pub datagrams_dropped: AtomicU64,
    pub streams_opened: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
}

impl TransportMetrics {
    /// Create new zeroed metrics.
    pub fn new() -> Self {
        Self {
            datagrams_sent: AtomicU64::new(0),
            datagrams_received: AtomicU64::new(0),
            datagrams_dropped: AtomicU64::new(0),
            streams_opened: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
        }
    }

    /// Record a sent datagram.
    pub fn record_datagram_sent(&self, bytes: usize) {
        self.datagrams_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// Record a received datagram.
    pub fn record_datagram_received(&self, bytes: usize) {
        self.datagrams_received.fetch_add(1, Ordering::Relaxed);
        self.bytes_received
            .fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// Record a dropped datagram.
    pub fn record_datagram_dropped(&self) {
        self.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an opened stream.
    pub fn record_stream_opened(&self) {
        self.streams_opened.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for TransportMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Abstract transport for peer-to-peer communication.
///
/// Implemented by both [`PeerConnection`](crate::PeerConnection) (real iroh QUIC)
/// and [`SimTransport`](crate::sim::SimTransport) (simulated in-process).
#[allow(async_fn_in_trait)]
pub trait Transport: Send + Sync {
    /// The send half of a bidirectional stream.
    type SendStream: tokio::io::AsyncWrite + Send + Unpin;
    /// The receive half of a bidirectional stream.
    type RecvStream: tokio::io::AsyncRead + Send + Unpin;

    /// Send an unreliable datagram to the remote peer.
    async fn send_datagram(&self, data: Bytes) -> Result<()>;

    /// Receive an unreliable datagram from the remote peer.
    async fn recv_datagram(&self) -> Result<Bytes>;

    /// Open a reliable bidirectional stream.
    async fn open_bi_stream(&self) -> Result<(Self::SendStream, Self::RecvStream)>;

    /// Accept a reliable bidirectional stream opened by the remote peer.
    async fn accept_bi_stream(&self) -> Result<(Self::SendStream, Self::RecvStream)>;

    /// Get transport metrics.
    fn transport_metrics(&self) -> &TransportMetrics;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_default_is_zeroed() {
        let m = TransportMetrics::new();
        assert_eq!(m.datagrams_sent.load(Ordering::Relaxed), 0);
        assert_eq!(m.datagrams_received.load(Ordering::Relaxed), 0);
        assert_eq!(m.datagrams_dropped.load(Ordering::Relaxed), 0);
        assert_eq!(m.streams_opened.load(Ordering::Relaxed), 0);
        assert_eq!(m.bytes_sent.load(Ordering::Relaxed), 0);
        assert_eq!(m.bytes_received.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn metrics_record_helpers() {
        let m = TransportMetrics::new();

        m.record_datagram_sent(100);
        m.record_datagram_sent(200);
        assert_eq!(m.datagrams_sent.load(Ordering::Relaxed), 2);
        assert_eq!(m.bytes_sent.load(Ordering::Relaxed), 300);

        m.record_datagram_received(50);
        assert_eq!(m.datagrams_received.load(Ordering::Relaxed), 1);
        assert_eq!(m.bytes_received.load(Ordering::Relaxed), 50);

        m.record_datagram_dropped();
        m.record_datagram_dropped();
        assert_eq!(m.datagrams_dropped.load(Ordering::Relaxed), 2);

        m.record_stream_opened();
        assert_eq!(m.streams_opened.load(Ordering::Relaxed), 1);
    }
}
