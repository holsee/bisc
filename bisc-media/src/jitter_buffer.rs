//! Audio jitter buffer: reorder packets, handle gaps, produce smooth playback.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use bisc_protocol::media::MediaPacket;

/// Minimum buffer depth in milliseconds.
const MIN_DEPTH_MS: u32 = 50;

/// Maximum buffer depth in milliseconds.
const MAX_DEPTH_MS: u32 = 200;

/// Default initial buffer depth in milliseconds.
const DEFAULT_DEPTH_MS: u32 = 80;

/// How quickly the depth adapts (exponential moving average factor).
const ADAPT_ALPHA: f64 = 0.05;

/// Jitter buffer statistics.
pub struct JitterStats {
    pub packets_received: AtomicU64,
    pub packets_played: AtomicU64,
    pub packets_dropped_duplicate: AtomicU64,
    pub packets_lost: AtomicU64,
}

impl JitterStats {
    fn new() -> Self {
        Self {
            packets_received: AtomicU64::new(0),
            packets_played: AtomicU64::new(0),
            packets_dropped_duplicate: AtomicU64::new(0),
            packets_lost: AtomicU64::new(0),
        }
    }
}

/// Audio jitter buffer that reorders packets and smooths playback.
pub struct JitterBuffer {
    /// Buffered packets keyed by sequence number.
    buffer: BTreeMap<u32, MediaPacket>,
    /// Current target buffer depth in milliseconds.
    target_depth_ms: f64,
    /// Next expected sequence number for playout.
    next_playout_seq: Option<u32>,
    /// Stream ID this buffer handles (for filtering).
    stream_id: u8,
    /// Timestamp increment per packet (e.g. 960 for 20ms at 48kHz).
    timestamp_step: u32,
    /// Frame duration in milliseconds.
    frame_duration_ms: u32,
    /// Number of packets received before first playout (initial fill).
    initial_fill: u32,
    /// Whether initial fill is complete.
    primed: bool,
    /// Total packets pushed (for initial fill tracking).
    push_count: u32,
    /// Jitter estimator: exponential moving average of arrival jitter.
    jitter_avg_ms: f64,
    /// Last packet arrival time.
    last_arrival: Option<Instant>,
    /// Last packet timestamp (for jitter calculation).
    last_timestamp: Option<u32>,
    /// Statistics.
    pub stats: Arc<JitterStats>,
}

impl JitterBuffer {
    /// Create a new jitter buffer for a specific stream.
    ///
    /// - `stream_id`: filter packets by this stream (e.g. 0 = audio)
    /// - `timestamp_step`: timestamp increment per frame (e.g. 960 for 20ms at 48kHz)
    /// - `frame_duration_ms`: frame duration in milliseconds (e.g. 20)
    pub fn new(stream_id: u8, timestamp_step: u32, frame_duration_ms: u32) -> Self {
        let target_depth_ms = DEFAULT_DEPTH_MS as f64;
        let initial_fill = (target_depth_ms / frame_duration_ms as f64).ceil() as u32;

        tracing::info!(
            stream_id,
            target_depth_ms,
            initial_fill,
            "jitter buffer created"
        );

        Self {
            buffer: BTreeMap::new(),
            target_depth_ms,
            next_playout_seq: None,
            stream_id,
            timestamp_step,
            frame_duration_ms,
            initial_fill,
            primed: false,
            push_count: 0,
            jitter_avg_ms: 0.0,
            last_arrival: None,
            last_timestamp: None,
            stats: Arc::new(JitterStats::new()),
        }
    }

    /// Push a packet into the buffer.
    ///
    /// Duplicate packets are silently dropped.
    /// Packets for a different stream_id are ignored.
    pub fn push(&mut self, packet: MediaPacket) {
        if packet.stream_id != self.stream_id {
            return;
        }

        let seq = packet.sequence;

        // Drop duplicates
        if self.buffer.contains_key(&seq) {
            self.stats
                .packets_dropped_duplicate
                .fetch_add(1, Ordering::Relaxed);
            tracing::trace!(seq, "dropped duplicate packet");
            return;
        }

        // Drop packets that have already been played out
        if let Some(next_seq) = self.next_playout_seq {
            if Self::seq_before(seq, next_seq) {
                self.stats
                    .packets_dropped_duplicate
                    .fetch_add(1, Ordering::Relaxed);
                tracing::trace!(seq, next_playout = next_seq, "dropped late packet");
                return;
            }
        }

        // Calculate jitter from arrival times
        let now = Instant::now();
        if let (Some(last_arrival), Some(last_ts)) = (self.last_arrival, self.last_timestamp) {
            let arrival_diff_ms = now.duration_since(last_arrival).as_secs_f64() * 1000.0;
            let expected_diff_ms = if packet.timestamp >= last_ts {
                (packet.timestamp - last_ts) as f64 / self.timestamp_step as f64
                    * self.frame_duration_ms as f64
            } else {
                0.0
            };
            let jitter_ms = (arrival_diff_ms - expected_diff_ms).abs();
            self.jitter_avg_ms = self.jitter_avg_ms * (1.0 - ADAPT_ALPHA) + jitter_ms * ADAPT_ALPHA;

            // Adapt buffer depth based on jitter
            self.adapt_depth();
        }
        self.last_arrival = Some(now);
        self.last_timestamp = Some(packet.timestamp);

        self.buffer.insert(seq, packet);
        self.push_count += 1;
        self.stats.packets_received.fetch_add(1, Ordering::Relaxed);

        tracing::trace!(
            seq,
            buffer_size = self.buffer.len(),
            "pushed packet into jitter buffer"
        );

        // Prime the buffer after initial fill
        if !self.primed && self.push_count >= self.initial_fill {
            self.primed = true;
            if self.next_playout_seq.is_none() {
                if let Some((&first_seq, _)) = self.buffer.iter().next() {
                    self.next_playout_seq = Some(first_seq);
                }
            }
            tracing::debug!(
                next_seq = ?self.next_playout_seq,
                buffer_size = self.buffer.len(),
                "jitter buffer primed"
            );
        }
    }

    /// Pop the next packet for playout.
    ///
    /// Returns `Some(packet)` if the next expected packet is available.
    /// Returns `None` if the buffer isn't primed or the next packet hasn't arrived.
    /// If the next packet is missing (gap), it is counted as lost and the
    /// sequence advances.
    pub fn pop(&mut self) -> Option<MediaPacket> {
        if !self.primed {
            return None;
        }

        let next_seq = self.next_playout_seq?;

        if let Some(packet) = self.buffer.remove(&next_seq) {
            self.next_playout_seq = Some(next_seq.wrapping_add(1));
            self.stats.packets_played.fetch_add(1, Ordering::Relaxed);

            tracing::trace!(
                seq = next_seq,
                buffer_remaining = self.buffer.len(),
                "popped packet from jitter buffer"
            );

            return Some(packet);
        }

        // Next packet is missing — check if we should skip it (concealment)
        // Only skip if we have later packets in the buffer
        if self.buffer.keys().any(|&s| Self::seq_after(s, next_seq)) {
            self.stats.packets_lost.fetch_add(1, Ordering::Relaxed);
            self.next_playout_seq = Some(next_seq.wrapping_add(1));

            tracing::debug!(seq = next_seq, "packet lost (gap in sequence)");

            return None;
        }

        // No later packets either — buffer might be empty
        None
    }

    /// Get the current buffer depth (number of packets).
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get the current target buffer depth in milliseconds.
    pub fn target_depth_ms(&self) -> f64 {
        self.target_depth_ms
    }

    /// Get the average jitter in milliseconds.
    pub fn average_jitter_ms(&self) -> f64 {
        self.jitter_avg_ms
    }

    /// Get the statistics.
    pub fn stats(&self) -> &Arc<JitterStats> {
        &self.stats
    }

    /// Check if the buffer has been primed (initial fill complete).
    pub fn is_primed(&self) -> bool {
        self.primed
    }

    /// Set the target depth directly (for testing).
    pub fn set_target_depth_ms(&mut self, depth_ms: f64) {
        self.target_depth_ms = depth_ms.clamp(MIN_DEPTH_MS as f64, MAX_DEPTH_MS as f64);
        self.initial_fill = (self.target_depth_ms / self.frame_duration_ms as f64).ceil() as u32;
    }

    /// Adapt buffer depth based on observed jitter.
    fn adapt_depth(&mut self) {
        // Target depth should be ~2-3x the average jitter
        let desired = (self.jitter_avg_ms * 3.0).clamp(MIN_DEPTH_MS as f64, MAX_DEPTH_MS as f64);
        self.target_depth_ms = self.target_depth_ms * (1.0 - ADAPT_ALPHA) + desired * ADAPT_ALPHA;
    }

    /// Compare sequence numbers accounting for wrapping.
    fn seq_before(a: u32, b: u32) -> bool {
        let diff = a.wrapping_sub(b);
        diff > 0x8000_0000
    }

    /// Compare sequence numbers accounting for wrapping.
    fn seq_after(a: u32, b: u32) -> bool {
        Self::seq_before(b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_packet(stream_id: u8, seq: u32, timestamp: u32) -> MediaPacket {
        MediaPacket {
            stream_id,
            sequence: seq,
            timestamp,
            fragment_index: 0,
            fragment_count: 1,
            is_keyframe: false,
            payload: vec![0xAA; 10],
        }
    }

    #[test]
    fn in_order_packets_returned_in_order() {
        let mut jb = JitterBuffer::new(0, 960, 20);
        // Push enough to prime (80ms / 20ms = 4 packets)
        for i in 0..4 {
            jb.push(make_packet(0, i, i * 960));
        }
        assert!(jb.is_primed());

        // Pop should return in order
        for i in 0..4 {
            let pkt = jb.pop().unwrap();
            assert_eq!(pkt.sequence, i);
        }
    }

    #[test]
    fn out_of_order_packets_reordered() {
        let mut jb = JitterBuffer::new(0, 960, 20);

        // Push packets out of order
        jb.push(make_packet(0, 2, 2 * 960));
        jb.push(make_packet(0, 0, 0));
        jb.push(make_packet(0, 3, 3 * 960));
        jb.push(make_packet(0, 1, 1 * 960));

        assert!(jb.is_primed());

        // Should come out in order
        assert_eq!(jb.pop().unwrap().sequence, 0);
        assert_eq!(jb.pop().unwrap().sequence, 1);
        assert_eq!(jb.pop().unwrap().sequence, 2);
        assert_eq!(jb.pop().unwrap().sequence, 3);
    }

    #[test]
    fn duplicate_packets_dropped() {
        let mut jb = JitterBuffer::new(0, 960, 20);

        for i in 0..4 {
            jb.push(make_packet(0, i, i * 960));
        }

        // Push duplicates
        jb.push(make_packet(0, 0, 0));
        jb.push(make_packet(0, 2, 2 * 960));

        assert_eq!(
            jb.stats.packets_dropped_duplicate.load(Ordering::Relaxed),
            2
        );
        assert_eq!(jb.len(), 4); // Still only 4 unique packets
    }

    #[test]
    fn gap_in_sequence_produces_none_and_counts_loss() {
        let mut jb = JitterBuffer::new(0, 960, 20);

        // Push packets 0, 1, 2, 4 (missing 3)
        jb.push(make_packet(0, 0, 0));
        jb.push(make_packet(0, 1, 960));
        jb.push(make_packet(0, 2, 2 * 960));
        jb.push(make_packet(0, 4, 4 * 960)); // skip 3

        assert!(jb.is_primed());

        // Pop 0, 1, 2 normally
        assert_eq!(jb.pop().unwrap().sequence, 0);
        assert_eq!(jb.pop().unwrap().sequence, 1);
        assert_eq!(jb.pop().unwrap().sequence, 2);

        // Pop 3 — missing, should return None (gap) and advance
        let result = jb.pop();
        assert!(result.is_none(), "gap should produce None");
        assert_eq!(jb.stats.packets_lost.load(Ordering::Relaxed), 1);

        // Pop 4 — should now be available
        assert_eq!(jb.pop().unwrap().sequence, 4);
    }

    #[test]
    fn buffer_depth_adapts_to_jitter() {
        let mut jb = JitterBuffer::new(0, 960, 20);
        let initial_depth = jb.target_depth_ms();

        // Simulate high jitter by setting the jitter average directly
        // (normally computed from arrival times, but we test the adaptation logic)
        jb.jitter_avg_ms = 100.0; // 100ms jitter
        jb.adapt_depth();
        let high_jitter_depth = jb.target_depth_ms();

        // With 100ms jitter, desired = 300ms, clamped to MAX_DEPTH_MS (200ms)
        // After one adaptation step: depth moves slightly toward 200ms
        assert!(
            high_jitter_depth > initial_depth,
            "depth should increase with high jitter: {} > {}",
            high_jitter_depth,
            initial_depth
        );

        // Now simulate low jitter for many iterations
        jb.jitter_avg_ms = 5.0; // 5ms jitter
        for _ in 0..200 {
            jb.adapt_depth();
        }
        let low_jitter_depth = jb.target_depth_ms();

        assert!(
            low_jitter_depth < high_jitter_depth,
            "depth should decrease with low jitter: {} < {}",
            low_jitter_depth,
            high_jitter_depth
        );
    }

    #[test]
    fn statistics_tracked_correctly() {
        let mut jb = JitterBuffer::new(0, 960, 20);

        // Push 5 packets
        for i in 0..5 {
            jb.push(make_packet(0, i, i * 960));
        }
        assert_eq!(jb.stats.packets_received.load(Ordering::Relaxed), 5);

        // Pop 4 (the initial fill primes at 4)
        let mut played = 0;
        while jb.pop().is_some() {
            played += 1;
        }
        assert_eq!(played, 5);
        assert_eq!(jb.stats.packets_played.load(Ordering::Relaxed), 5);

        // Push duplicate
        jb.push(make_packet(0, 0, 0));
        assert_eq!(
            jb.stats.packets_dropped_duplicate.load(Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn different_stream_id_ignored() {
        let mut jb = JitterBuffer::new(0, 960, 20);

        // Push packets for stream 1 — should be ignored
        for i in 0..4 {
            jb.push(make_packet(1, i, i * 960));
        }
        assert_eq!(jb.len(), 0);
        assert!(!jb.is_primed());
    }

    #[test]
    fn seq_comparison_handles_wrapping() {
        // u32::MAX - 1 should be before u32::MAX
        assert!(JitterBuffer::seq_before(u32::MAX - 1, u32::MAX));
        // u32::MAX should be before 0 (wrapping)
        assert!(JitterBuffer::seq_before(u32::MAX, 0));
        // 0 should be after u32::MAX (wrapping)
        assert!(JitterBuffer::seq_after(0, u32::MAX));
    }
}
