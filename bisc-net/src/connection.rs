//! Direct peer connections for media and file transfer.

use anyhow::Result;
use bytes::Bytes;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use tokio::sync::mpsc;

use crate::endpoint::MEDIA_ALPN;

/// State of a peer connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connection is being established.
    Connecting,
    /// Connection is active and ready for data.
    Connected,
    /// Connection has been closed.
    Disconnected,
}

/// Protocol handler that accepts incoming media connections and forwards them.
#[derive(Debug, Clone)]
pub struct MediaProtocol {
    incoming_tx: mpsc::UnboundedSender<Connection>,
}

impl MediaProtocol {
    /// Create a new media protocol handler.
    ///
    /// Returns the handler and a receiver for incoming connections.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Connection>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { incoming_tx: tx }, rx)
    }

    /// Get the ALPN protocol identifier.
    pub fn alpn() -> &'static [u8] {
        MEDIA_ALPN
    }
}

impl ProtocolHandler for MediaProtocol {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        tracing::info!(
            peer = %connection.remote_id(),
            "accepted incoming media connection"
        );
        let _ = self.incoming_tx.send(connection);
        Ok(())
    }
}

/// A direct QUIC connection to a peer for media and file transfer.
pub struct PeerConnection {
    connection: Connection,
    remote_id: iroh::EndpointId,
}

impl PeerConnection {
    /// Initiate a connection to a remote peer.
    pub async fn connect(
        endpoint: &iroh::Endpoint,
        remote_id: iroh::EndpointId,
        alpn: &[u8],
    ) -> Result<Self> {
        tracing::info!(peer = %remote_id, alpn = ?String::from_utf8_lossy(alpn), "connecting to peer");
        let connection = endpoint.connect(remote_id, alpn).await?;
        tracing::info!(peer = %remote_id, "connected to peer");
        Ok(Self {
            connection,
            remote_id,
        })
    }

    /// Wrap an already-established connection (e.g. from a ProtocolHandler).
    pub fn from_connection(connection: Connection) -> Self {
        let remote_id = connection.remote_id();
        Self {
            connection,
            remote_id,
        }
    }

    /// Open a reliable bidirectional stream for control messages.
    pub async fn open_control_stream(
        &self,
    ) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream)> {
        tracing::debug!(peer = %self.remote_id, "opening control stream");
        let (send, recv) = self.connection.open_bi().await?;
        Ok((send, recv))
    }

    /// Accept a bidirectional stream opened by the remote peer.
    pub async fn accept_bi(
        &self,
    ) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream)> {
        tracing::debug!(peer = %self.remote_id, "accepting bi stream");
        let (send, recv) = self.connection.accept_bi().await?;
        Ok((send, recv))
    }

    /// Send an unreliable datagram to the peer.
    pub fn send_datagram(&self, data: Bytes) -> Result<()> {
        self.connection.send_datagram(data)?;
        Ok(())
    }

    /// Send a datagram with backpressure (waits for buffer space).
    pub async fn send_datagram_wait(&self, data: Bytes) -> Result<()> {
        self.connection.send_datagram_wait(data).await?;
        Ok(())
    }

    /// Receive an unreliable datagram from the peer.
    pub async fn recv_datagram(&self) -> Result<Bytes> {
        let data = self.connection.read_datagram().await?;
        Ok(data)
    }

    /// Get the maximum datagram size for this connection.
    pub fn max_datagram_size(&self) -> Option<usize> {
        self.connection.max_datagram_size()
    }

    /// Close the connection gracefully.
    pub fn close(&self) {
        tracing::info!(peer = %self.remote_id, "closing connection");
        self.connection.close(0u8.into(), b"bye");
    }

    /// Wait for the connection to be closed by either side.
    pub async fn closed(&self) -> iroh::endpoint::ConnectionError {
        self.connection.closed().await
    }

    /// Get the connection state.
    pub fn state(&self) -> ConnectionState {
        if self.connection.close_reason().is_some() {
            ConnectionState::Disconnected
        } else {
            ConnectionState::Connected
        }
    }

    /// Get the remote peer's endpoint ID.
    pub fn remote_id(&self) -> iroh::EndpointId {
        self.remote_id
    }

    /// Get the underlying iroh connection.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Get the round-trip time to the peer on the given path.
    pub fn rtt(&self, path_id: iroh::endpoint::PathId) -> Option<std::time::Duration> {
        self.connection.rtt(path_id)
    }
}

impl std::fmt::Debug for PeerConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeerConnection")
            .field("remote_id", &self.remote_id)
            .field("state", &self.state())
            .finish()
    }
}
