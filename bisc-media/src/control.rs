//! Media control channel: send/receive control messages over a reliable QUIC stream.
//!
//! Control messages (keyframe requests, receiver reports, quality changes) are
//! exchanged over a reliable bidirectional stream, separate from the lossy
//! datagram transport used for media packets.

use std::sync::atomic::Ordering;
use std::time::Instant;

use anyhow::{Context, Result};
use bisc_protocol::media::MediaControl;
use iroh::endpoint::{RecvStream, SendStream};

use crate::jitter_buffer::JitterStats;
use crate::transport::TransportMetrics;

/// Write a length-prefixed postcard-encoded message to a stream.
async fn write_message<T: serde::Serialize>(stream: &mut SendStream, msg: &T) -> Result<()> {
    let data = postcard::to_allocvec(msg).context("failed to serialize control message")?;
    let len = (data.len() as u32).to_be_bytes();
    stream
        .write_all(&len)
        .await
        .context("failed to write message length")?;
    stream
        .write_all(&data)
        .await
        .context("failed to write message data")?;
    Ok(())
}

/// Read a length-prefixed postcard-encoded message from a stream.
async fn read_message<T: serde::de::DeserializeOwned>(stream: &mut RecvStream) -> Result<T> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .context("failed to read message length")?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 64 * 1024 {
        anyhow::bail!("control message too large: {len} bytes");
    }

    let mut data = vec![0u8; len];
    stream
        .read_exact(&mut data)
        .await
        .context("failed to read message data")?;

    postcard::from_bytes(&data).context("failed to deserialize control message")
}

/// Sends `MediaControl` messages over a reliable QUIC stream.
pub struct MediaControlSender {
    stream: SendStream,
    messages_sent: u64,
}

impl MediaControlSender {
    /// Create a new control sender wrapping a QUIC send stream.
    pub fn new(stream: SendStream) -> Self {
        tracing::info!("media control sender created");
        Self {
            stream,
            messages_sent: 0,
        }
    }

    /// Send a control message.
    pub async fn send(&mut self, msg: &MediaControl) -> Result<()> {
        write_message(&mut self.stream, msg).await?;
        self.messages_sent += 1;

        tracing::debug!(
            message_type = control_type_name(msg),
            total_sent = self.messages_sent,
            "sent control message"
        );

        Ok(())
    }

    /// Get the number of messages sent.
    pub fn messages_sent(&self) -> u64 {
        self.messages_sent
    }
}

/// Receives `MediaControl` messages from a reliable QUIC stream.
pub struct MediaControlReceiver {
    stream: RecvStream,
    messages_received: u64,
}

impl MediaControlReceiver {
    /// Create a new control receiver wrapping a QUIC receive stream.
    pub fn new(stream: RecvStream) -> Self {
        tracing::info!("media control receiver created");
        Self {
            stream,
            messages_received: 0,
        }
    }

    /// Receive the next control message.
    ///
    /// Returns `Ok(msg)` on success, or an error if the stream is closed or malformed.
    pub async fn recv(&mut self) -> Result<MediaControl> {
        let msg: MediaControl = read_message(&mut self.stream).await?;
        self.messages_received += 1;

        tracing::debug!(
            message_type = control_type_name(&msg),
            total_received = self.messages_received,
            "received control message"
        );

        Ok(msg)
    }

    /// Get the number of messages received.
    pub fn messages_received(&self) -> u64 {
        self.messages_received
    }
}

/// Generates `ReceiverReport` control messages from transport and jitter buffer statistics.
///
/// Call `generate_report` periodically (every ~1 second) to produce a report
/// reflecting the current packet statistics and RTT estimate.
pub struct ReceiverReportGenerator {
    stream_id: u8,
    /// Snapshot of packets_received at last report.
    last_packets_received: u64,
    /// Snapshot of packets_lost at last report.
    last_packets_lost: u64,
    /// When we last sent a report (for RTT calculation).
    last_report_sent: Option<Instant>,
    /// Current RTT estimate in milliseconds.
    rtt_estimate_ms: u16,
}

impl ReceiverReportGenerator {
    /// Create a new report generator for the given stream.
    pub fn new(stream_id: u8) -> Self {
        tracing::info!(stream_id, "receiver report generator created");
        Self {
            stream_id,
            last_packets_received: 0,
            last_packets_lost: 0,
            last_report_sent: None,
            rtt_estimate_ms: 0,
        }
    }

    /// Generate a receiver report from current statistics.
    ///
    /// Uses transport metrics for packet counts and jitter buffer stats
    /// for loss/jitter information.
    pub fn generate_report(
        &mut self,
        transport_metrics: &TransportMetrics,
        jitter_stats: &JitterStats,
        jitter_avg_ms: f64,
    ) -> MediaControl {
        let packets_received = transport_metrics.packets_received.load(Ordering::Relaxed) as u32;
        let packets_lost = jitter_stats.packets_lost.load(Ordering::Relaxed) as u32;
        let jitter = jitter_avg_ms as u32;

        self.last_packets_received = packets_received as u64;
        self.last_packets_lost = packets_lost as u64;
        self.last_report_sent = Some(Instant::now());

        let report = MediaControl::ReceiverReport {
            stream_id: self.stream_id,
            packets_received,
            packets_lost,
            jitter,
            rtt_estimate_ms: self.rtt_estimate_ms,
        };

        tracing::debug!(
            stream_id = self.stream_id,
            packets_received,
            packets_lost,
            jitter,
            rtt_estimate_ms = self.rtt_estimate_ms,
            "generated receiver report"
        );

        report
    }

    /// Update the RTT estimate based on receiving a report back from the remote peer.
    ///
    /// Call this when you receive a `ReceiverReport` from the remote side.
    /// The RTT is computed as the time elapsed since the last report was sent.
    pub fn update_rtt_from_response(&mut self) {
        if let Some(sent_at) = self.last_report_sent.take() {
            let rtt_ms = sent_at.elapsed().as_millis() as u16;
            // Exponential moving average for RTT smoothing
            if self.rtt_estimate_ms == 0 {
                self.rtt_estimate_ms = rtt_ms;
            } else {
                // 7/8 old + 1/8 new (similar to TCP SRTT)
                self.rtt_estimate_ms =
                    (self.rtt_estimate_ms as u32 * 7 / 8 + rtt_ms as u32 / 8) as u16;
            }

            tracing::debug!(
                rtt_sample_ms = rtt_ms,
                rtt_estimate_ms = self.rtt_estimate_ms,
                "updated RTT estimate"
            );
        }
    }

    /// Get the current RTT estimate in milliseconds.
    pub fn rtt_estimate_ms(&self) -> u16 {
        self.rtt_estimate_ms
    }

    /// Get the stream ID this generator is tracking.
    pub fn stream_id(&self) -> u8 {
        self.stream_id
    }
}

/// Return a human-readable name for a control message type (for logging).
fn control_type_name(msg: &MediaControl) -> &'static str {
    match msg {
        MediaControl::RequestKeyframe { .. } => "RequestKeyframe",
        MediaControl::ReceiverReport { .. } => "ReceiverReport",
        MediaControl::QualityChange { .. } => "QualityChange",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;

    use super::*;

    fn make_transport_metrics(received: u64) -> TransportMetrics {
        TransportMetrics {
            packets_sent: AtomicU64::new(0),
            packets_received: AtomicU64::new(received),
            packets_dropped: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
        }
    }

    fn make_jitter_stats(lost: u64) -> JitterStats {
        JitterStats {
            packets_received: AtomicU64::new(0),
            packets_played: AtomicU64::new(0),
            packets_dropped_duplicate: AtomicU64::new(0),
            packets_lost: AtomicU64::new(lost),
        }
    }

    #[test]
    fn receiver_report_reflects_packet_counts() {
        let mut gen = ReceiverReportGenerator::new(0);
        let transport = make_transport_metrics(500);
        let jitter = make_jitter_stats(3);

        let report = gen.generate_report(&transport, &jitter, 12.5);

        match report {
            MediaControl::ReceiverReport {
                stream_id,
                packets_received,
                packets_lost,
                jitter,
                rtt_estimate_ms,
            } => {
                assert_eq!(stream_id, 0);
                assert_eq!(packets_received, 500);
                assert_eq!(packets_lost, 3);
                assert_eq!(jitter, 12);
                assert_eq!(rtt_estimate_ms, 0); // No RTT measurement yet
            }
            _ => panic!("expected ReceiverReport"),
        }
    }

    #[test]
    fn rtt_estimate_updates_on_response() {
        let mut gen = ReceiverReportGenerator::new(0);

        // Simulate sending a report
        gen.last_report_sent = Some(Instant::now());

        // Sleep briefly to get a non-zero RTT
        std::thread::sleep(std::time::Duration::from_millis(5));

        gen.update_rtt_from_response();
        let rtt = gen.rtt_estimate_ms();

        // RTT should be at least 5ms (we slept that long)
        assert!(rtt >= 3, "RTT should be at least ~5ms, got {rtt}ms");
        // And less than something reasonable (100ms)
        assert!(rtt < 100, "RTT should be < 100ms, got {rtt}ms");
    }

    #[test]
    fn rtt_smoothing_applies_ema() {
        let mut gen = ReceiverReportGenerator::new(0);

        // Set initial RTT to 100ms
        gen.rtt_estimate_ms = 100;

        // Simulate a new RTT sample of ~10ms
        gen.last_report_sent = Some(Instant::now());
        std::thread::sleep(std::time::Duration::from_millis(10));
        gen.update_rtt_from_response();

        // EMA: 100 * 7/8 + ~10 * 1/8 ≈ 87 + 1 = ~88
        // Should have moved toward the new sample but not jumped to it
        let rtt = gen.rtt_estimate_ms();
        assert!(
            rtt > 50 && rtt < 100,
            "RTT should be smoothed between old(100) and new(~10), got {rtt}"
        );
    }

    #[test]
    fn control_type_name_returns_correct_names() {
        assert_eq!(
            control_type_name(&MediaControl::RequestKeyframe { stream_id: 0 }),
            "RequestKeyframe"
        );
        assert_eq!(
            control_type_name(&MediaControl::ReceiverReport {
                stream_id: 0,
                packets_received: 0,
                packets_lost: 0,
                jitter: 0,
                rtt_estimate_ms: 0,
            }),
            "ReceiverReport"
        );
        assert_eq!(
            control_type_name(&MediaControl::QualityChange {
                stream_id: 0,
                bitrate_bps: 0,
                resolution: (0, 0),
                framerate: 0,
            }),
            "QualityChange"
        );
    }
}
