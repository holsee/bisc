//! MediaPacket framing and datagram transport over QUIC connections.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use bisc_protocol::media::{decode_media_packet, encode_media_packet, MediaPacket};
use bytes::Bytes;

/// Transport for sending and receiving `MediaPacket`s over a QUIC connection.
///
/// Handles serialization (postcard), sequence number tracking per stream_id,
/// and loss detection. Invalid datagrams are silently dropped (expected for
/// lossy media transport).
pub struct MediaTransport {
    connection: iroh::endpoint::Connection,
    /// Next sequence number to assign per stream_id when sending.
    send_sequences: HashMap<u8, u32>,
    /// Last received sequence number per stream_id for loss detection.
    recv_sequences: HashMap<u8, u32>,
    /// Metrics counters.
    pub metrics: Arc<TransportMetrics>,
}

/// Metrics for observability and testing.
pub struct TransportMetrics {
    pub packets_sent: AtomicU64,
    pub packets_received: AtomicU64,
    pub packets_dropped: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
}

impl TransportMetrics {
    fn new() -> Self {
        Self {
            packets_sent: AtomicU64::new(0),
            packets_received: AtomicU64::new(0),
            packets_dropped: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
        }
    }
}

impl MediaTransport {
    /// Create a new media transport wrapping a QUIC connection.
    pub fn new(connection: iroh::endpoint::Connection) -> Self {
        let remote = connection.remote_id();
        tracing::info!(peer = %remote, "media transport created");
        Self {
            connection,
            send_sequences: HashMap::new(),
            recv_sequences: HashMap::new(),
            metrics: Arc::new(TransportMetrics::new()),
        }
    }

    /// Send a media packet as a QUIC datagram.
    ///
    /// Automatically assigns the next sequence number for the packet's stream_id.
    pub fn send_media_packet(&mut self, packet: &mut MediaPacket) -> Result<()> {
        // Assign sequence number
        let seq = self.send_sequences.entry(packet.stream_id).or_insert(0);
        packet.sequence = *seq;
        *seq = seq.wrapping_add(1);

        let data = encode_media_packet(packet).context("failed to serialize MediaPacket")?;
        let len = data.len();

        self.connection
            .send_datagram(Bytes::from(data))
            .context("failed to send datagram")?;

        self.metrics.packets_sent.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .bytes_sent
            .fetch_add(len as u64, Ordering::Relaxed);

        tracing::trace!(
            stream_id = packet.stream_id,
            sequence = packet.sequence,
            payload_bytes = packet.payload.len(),
            wire_bytes = len,
            "sent media packet"
        );

        Ok(())
    }

    /// Send a pre-built media packet without modifying its sequence number.
    pub fn send_raw_packet(&self, packet: &MediaPacket) -> Result<()> {
        let data = encode_media_packet(packet).context("failed to serialize MediaPacket")?;
        let len = data.len();

        self.connection
            .send_datagram(Bytes::from(data))
            .context("failed to send datagram")?;

        self.metrics.packets_sent.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .bytes_sent
            .fetch_add(len as u64, Ordering::Relaxed);

        Ok(())
    }

    /// Receive the next media packet from the connection.
    ///
    /// Invalid datagrams are silently dropped and the next datagram is tried.
    pub async fn recv_media_packet(&mut self) -> Result<MediaPacket> {
        loop {
            let data = self
                .connection
                .read_datagram()
                .await
                .context("failed to receive datagram")?;

            match decode_media_packet(&data) {
                Ok(packet) => {
                    self.metrics
                        .packets_received
                        .fetch_add(1, Ordering::Relaxed);
                    self.metrics
                        .bytes_received
                        .fetch_add(data.len() as u64, Ordering::Relaxed);

                    // Track sequence for loss detection
                    let last_seq = self
                        .recv_sequences
                        .entry(packet.stream_id)
                        .or_insert(packet.sequence);

                    if packet.sequence != *last_seq {
                        let expected = last_seq.wrapping_add(1);
                        if packet.sequence != expected {
                            let gap = packet.sequence.wrapping_sub(*last_seq);
                            tracing::debug!(
                                stream_id = packet.stream_id,
                                expected = expected,
                                got = packet.sequence,
                                gap,
                                "sequence gap detected (possible packet loss)"
                            );
                        }
                        *last_seq = packet.sequence;
                    }

                    tracing::trace!(
                        stream_id = packet.stream_id,
                        sequence = packet.sequence,
                        payload_bytes = packet.payload.len(),
                        "received media packet"
                    );

                    return Ok(packet);
                }
                Err(e) => {
                    self.metrics.packets_dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!(
                        error = %e,
                        bytes = data.len(),
                        "dropped malformed datagram"
                    );
                    // Continue to next datagram
                }
            }
        }
    }

    /// Get the underlying connection.
    pub fn connection(&self) -> &iroh::endpoint::Connection {
        &self.connection
    }

    /// Get the transport metrics.
    pub fn metrics(&self) -> &Arc<TransportMetrics> {
        &self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_datagram_does_not_panic() {
        // Feed garbage bytes to the deserializer
        let garbage = vec![0xFF, 0x00, 0xAB, 0xCD, 0xEF];
        let result = decode_media_packet(&garbage);
        assert!(result.is_err(), "garbage bytes should fail to deserialize");
    }

    #[test]
    fn empty_datagram_does_not_panic() {
        let result = decode_media_packet(&[]);
        assert!(result.is_err(), "empty bytes should fail to deserialize");
    }

    #[test]
    fn sequence_numbers_increment() {
        // We can't test the full transport without a connection,
        // but we can test the sequence number logic directly.
        let mut sequences: HashMap<u8, u32> = HashMap::new();

        for i in 0..5 {
            let stream_id = 0u8;
            let seq = sequences.entry(stream_id).or_insert(0);
            assert_eq!(*seq, i);
            *seq = seq.wrapping_add(1);
        }

        // Different stream_id has independent sequence
        for i in 0..3 {
            let stream_id = 1u8;
            let seq = sequences.entry(stream_id).or_insert(0);
            assert_eq!(*seq, i);
            *seq = seq.wrapping_add(1);
        }

        assert_eq!(*sequences.get(&0).unwrap(), 5);
        assert_eq!(*sequences.get(&1).unwrap(), 3);
    }

    #[test]
    fn media_packet_serialization_roundtrip() {
        let packet = MediaPacket {
            stream_id: 0,
            sequence: 42,
            timestamp: 96_000,
            fragment_index: 0,
            fragment_count: 1,
            is_keyframe: false,
            payload: vec![0xAA; 100],
        };

        let encoded = encode_media_packet(&packet).unwrap();
        let decoded = decode_media_packet(&encoded).unwrap();

        assert_eq!(decoded.stream_id, packet.stream_id);
        assert_eq!(decoded.sequence, packet.sequence);
        assert_eq!(decoded.timestamp, packet.timestamp);
        assert_eq!(decoded.payload, packet.payload);
    }
}
