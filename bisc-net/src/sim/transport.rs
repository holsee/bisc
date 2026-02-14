//! Simulated transport implementing the [`Transport`] trait.

use anyhow::{Context, Result};
use bytes::Bytes;
use std::sync::Arc;
use tokio::io::DuplexStream;
use tokio::sync::{mpsc, Mutex};

use crate::transport::{Transport, TransportMetrics};

use super::network::{SimNetworkInner, SimPeerId};

/// Simulated transport between two peers.
///
/// Routes datagrams through the [`SimNetwork`](super::SimNetwork) which applies
/// latency, loss, and jitter. Reliable streams use `tokio::io::DuplexStream`.
pub struct SimTransport {
    pub(crate) local_id: SimPeerId,
    pub(crate) remote_id: SimPeerId,
    pub(crate) network: Arc<SimNetworkInner>,
    pub(crate) datagram_rx: Mutex<mpsc::UnboundedReceiver<Bytes>>,
    pub(crate) stream_rx: Mutex<mpsc::UnboundedReceiver<(DuplexStream, DuplexStream)>>,
    pub(crate) metrics: TransportMetrics,
}

impl Transport for SimTransport {
    type SendStream = DuplexStream;
    type RecvStream = DuplexStream;

    async fn send_datagram(&self, data: Bytes) -> Result<()> {
        self.network
            .route_datagram(self.local_id, self.remote_id, data, &self.metrics)
            .await
    }

    async fn recv_datagram(&self) -> Result<Bytes> {
        let mut rx = self.datagram_rx.lock().await;
        let data = rx
            .recv()
            .await
            .context("sim transport datagram channel closed")?;
        self.metrics.record_datagram_received(data.len());
        Ok(data)
    }

    async fn open_bi_stream(&self) -> Result<(DuplexStream, DuplexStream)> {
        // Create two duplex pairs: one for each direction
        let (a_to_b_write, a_to_b_read) = tokio::io::duplex(8192);
        let (b_to_a_write, b_to_a_read) = tokio::io::duplex(8192);

        // Deliver the remote end (remote_send, remote_recv) to the peer
        self.network
            .deliver_stream(self.local_id, self.remote_id, (b_to_a_write, a_to_b_read))
            .await?;

        self.metrics.record_stream_opened();
        tracing::debug!(
            local = self.local_id.0,
            remote = self.remote_id.0,
            "opened sim bi stream"
        );

        // Return the local end (local_send, local_recv)
        Ok((a_to_b_write, b_to_a_read))
    }

    async fn accept_bi_stream(&self) -> Result<(DuplexStream, DuplexStream)> {
        let mut rx = self.stream_rx.lock().await;
        let (send, recv) = rx
            .recv()
            .await
            .context("sim transport stream channel closed")?;
        tracing::debug!(
            local = self.local_id.0,
            remote = self.remote_id.0,
            "accepted sim bi stream"
        );
        Ok((send, recv))
    }

    fn transport_metrics(&self) -> &TransportMetrics {
        &self.metrics
    }
}
