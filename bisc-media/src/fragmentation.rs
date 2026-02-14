//! Frame fragmentation and reassembly for video datagrams.
//!
//! Splits large encoded frames into datagram-sized [`MediaPacket`] fragments
//! and reassembles them on the receiving side.

use std::collections::HashMap;

use bisc_protocol::media::MediaPacket;

/// A fully reassembled frame.
#[derive(Debug, Clone)]
pub struct ReassembledFrame {
    /// Complete encoded frame data.
    pub data: Vec<u8>,
    /// Media timestamp.
    pub timestamp: u32,
    /// Stream identifier.
    pub stream_id: u8,
    /// Whether this frame is a keyframe.
    pub is_keyframe: bool,
}

/// Splits encoded frame data into [`MediaPacket`] fragments.
pub struct Fragmenter {
    next_sequence: u32,
}

impl Fragmenter {
    /// Create a new fragmenter.
    pub fn new() -> Self {
        Self { next_sequence: 0 }
    }

    /// Fragment encoded frame data into one or more [`MediaPacket`]s.
    ///
    /// Each packet's payload will be at most `max_payload_size` bytes.
    /// `max_payload_size` must be > 0.
    pub fn fragment(
        &mut self,
        stream_id: u8,
        timestamp: u32,
        is_keyframe: bool,
        data: &[u8],
        max_payload_size: usize,
    ) -> Vec<MediaPacket> {
        assert!(max_payload_size > 0, "max_payload_size must be > 0");

        let fragment_count = if data.is_empty() {
            1
        } else {
            data.len().div_ceil(max_payload_size)
        };

        // fragment_count must fit in u8
        let fragment_count = fragment_count.min(255) as u8;

        let mut packets = Vec::with_capacity(fragment_count as usize);

        for i in 0..fragment_count {
            let start = i as usize * max_payload_size;
            let end = (start + max_payload_size).min(data.len());
            let payload = data[start..end].to_vec();

            let seq = self.next_sequence;
            self.next_sequence = self.next_sequence.wrapping_add(1);

            packets.push(MediaPacket {
                stream_id,
                sequence: seq,
                timestamp,
                fragment_index: i,
                fragment_count,
                is_keyframe,
                payload,
            });
        }

        tracing::trace!(
            stream_id,
            timestamp,
            fragment_count,
            total_bytes = data.len(),
            "fragmented frame"
        );

        packets
    }
}

impl Default for Fragmenter {
    fn default() -> Self {
        Self::new()
    }
}

/// State for a partially received frame.
struct PendingFrame {
    fragments: Vec<Option<Vec<u8>>>,
    received_count: u8,
    fragment_count: u8,
    is_keyframe: bool,
    stream_id: u8,
    created_at: tokio::time::Instant,
}

/// Collects fragments and produces complete frames.
///
/// Handles out-of-order fragments and discards incomplete frames after a timeout.
pub struct Reassembler {
    /// Pending frames keyed by (stream_id, timestamp).
    pending: HashMap<(u8, u32), PendingFrame>,
    /// Timeout after which incomplete frames are discarded.
    timeout: std::time::Duration,
}

impl Reassembler {
    /// Create a new reassembler with the default 100ms timeout.
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            timeout: std::time::Duration::from_millis(100),
        }
    }

    /// Create a new reassembler with a custom timeout.
    pub fn with_timeout(timeout: std::time::Duration) -> Self {
        Self {
            pending: HashMap::new(),
            timeout,
        }
    }

    /// Push a received fragment.
    ///
    /// Returns `Some(ReassembledFrame)` when all fragments for a frame have arrived.
    /// Expired incomplete frames are cleaned up during this call.
    pub fn push(&mut self, packet: MediaPacket) -> Option<ReassembledFrame> {
        // Clean up expired frames
        self.expire_stale();

        let key = (packet.stream_id, packet.timestamp);

        let pending = self.pending.entry(key).or_insert_with(|| PendingFrame {
            fragments: vec![None; packet.fragment_count as usize],
            received_count: 0,
            fragment_count: packet.fragment_count,
            is_keyframe: packet.is_keyframe,
            stream_id: packet.stream_id,
            created_at: tokio::time::Instant::now(),
        });

        let idx = packet.fragment_index as usize;

        // Validate fragment
        if idx >= pending.fragments.len() {
            tracing::warn!(
                stream_id = packet.stream_id,
                timestamp = packet.timestamp,
                fragment_index = packet.fragment_index,
                fragment_count = pending.fragment_count,
                "fragment index out of bounds"
            );
            return None;
        }

        // Ignore duplicates
        if pending.fragments[idx].is_some() {
            tracing::trace!(
                stream_id = packet.stream_id,
                timestamp = packet.timestamp,
                fragment_index = packet.fragment_index,
                "duplicate fragment ignored"
            );
            return None;
        }

        pending.fragments[idx] = Some(packet.payload);
        pending.received_count += 1;

        // Check if complete
        if pending.received_count == pending.fragment_count {
            let pending = self.pending.remove(&key).unwrap();
            let mut data = Vec::new();
            for fragment in pending.fragments {
                data.extend_from_slice(&fragment.unwrap());
            }

            tracing::trace!(
                stream_id = pending.stream_id,
                timestamp = key.1,
                total_bytes = data.len(),
                "reassembled frame"
            );

            Some(ReassembledFrame {
                data,
                timestamp: key.1,
                stream_id: pending.stream_id,
                is_keyframe: pending.is_keyframe,
            })
        } else {
            None
        }
    }

    /// Remove frames that have been pending longer than the timeout.
    fn expire_stale(&mut self) {
        let timeout = self.timeout;
        self.pending.retain(|key, frame| {
            let expired = frame.created_at.elapsed() > timeout;
            if expired {
                tracing::debug!(
                    stream_id = key.0,
                    timestamp = key.1,
                    received = frame.received_count,
                    expected = frame.fragment_count,
                    "discarding incomplete frame (timeout)"
                );
            }
            !expired
        });
    }

    /// Get the number of frames currently pending reassembly.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

impl Default for Reassembler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_test_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "debug".into()),
            )
            .with_test_writer()
            .try_init();
    }

    #[test]
    fn small_frame_single_fragment() {
        init_test_tracing();

        let mut frag = Fragmenter::new();
        let data = vec![1, 2, 3, 4, 5];
        let packets = frag.fragment(1, 1000, false, &data, 100);

        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].fragment_index, 0);
        assert_eq!(packets[0].fragment_count, 1);
        assert_eq!(packets[0].payload, data);
        assert_eq!(packets[0].stream_id, 1);
        assert_eq!(packets[0].timestamp, 1000);
        assert!(!packets[0].is_keyframe);
    }

    #[test]
    fn large_frame_correct_fragment_count() {
        init_test_tracing();

        let mut frag = Fragmenter::new();
        let data = vec![0u8; 1000];
        let packets = frag.fragment(1, 2000, true, &data, 300);

        // ceil(1000 / 300) = 4
        assert_eq!(packets.len(), 4);
        assert_eq!(packets[0].fragment_count, 4);
        assert_eq!(packets[0].payload.len(), 300);
        assert_eq!(packets[1].payload.len(), 300);
        assert_eq!(packets[2].payload.len(), 300);
        assert_eq!(packets[3].payload.len(), 100);

        for (i, pkt) in packets.iter().enumerate() {
            assert_eq!(pkt.fragment_index, i as u8);
            assert_eq!(pkt.timestamp, 2000);
            assert!(pkt.is_keyframe);
        }
    }

    #[tokio::test]
    async fn fragments_reassemble_to_original() {
        init_test_tracing();

        let mut frag = Fragmenter::new();
        let data: Vec<u8> = (0..=255).cycle().take(500).collect();
        let packets = frag.fragment(1, 3000, false, &data, 100);

        let mut reasm = Reassembler::new();
        let mut result = None;
        for pkt in packets {
            if let Some(frame) = reasm.push(pkt) {
                result = Some(frame);
            }
        }

        let frame = result.expect("should reassemble");
        assert_eq!(frame.data, data);
        assert_eq!(frame.timestamp, 3000);
        assert_eq!(frame.stream_id, 1);
        assert!(!frame.is_keyframe);
    }

    #[tokio::test]
    async fn out_of_order_fragments_reassemble() {
        init_test_tracing();

        let mut frag = Fragmenter::new();
        let data = vec![42u8; 500];
        let mut packets = frag.fragment(1, 4000, true, &data, 100);

        // Reverse fragment order
        packets.reverse();

        let mut reasm = Reassembler::new();
        let mut result = None;
        for pkt in packets {
            if let Some(frame) = reasm.push(pkt) {
                result = Some(frame);
            }
        }

        let frame = result.expect("should reassemble even out of order");
        assert_eq!(frame.data, data);
        assert_eq!(frame.timestamp, 4000);
        assert!(frame.is_keyframe);
    }

    #[tokio::test]
    async fn incomplete_frame_discarded_after_timeout() {
        init_test_tracing();
        tokio::time::pause();

        let mut frag = Fragmenter::new();
        let data = vec![1u8; 500];
        let packets = frag.fragment(1, 5000, false, &data, 100);

        // Only push first 3 of 5 fragments
        let mut reasm = Reassembler::with_timeout(std::time::Duration::from_millis(100));
        for pkt in &packets[..3] {
            let result = reasm.push(pkt.clone());
            assert!(result.is_none());
        }
        assert_eq!(reasm.pending_count(), 1);

        // Advance time past timeout
        tokio::time::advance(std::time::Duration::from_millis(150)).await;

        // Push another packet to trigger cleanup
        let probe = MediaPacket {
            stream_id: 1,
            sequence: 999,
            timestamp: 6000,
            fragment_index: 0,
            fragment_count: 1,
            is_keyframe: false,
            payload: vec![0],
        };
        let _ = reasm.push(probe);

        // The old incomplete frame should have been discarded
        assert_eq!(reasm.pending_count(), 0);
    }

    #[tokio::test]
    async fn duplicate_fragments_dont_corrupt() {
        init_test_tracing();

        let mut frag = Fragmenter::new();
        let data = vec![7u8; 300];
        let packets = frag.fragment(1, 7000, false, &data, 100);

        assert_eq!(packets.len(), 3);

        let mut reasm = Reassembler::new();

        // Push fragment 0 twice
        assert!(reasm.push(packets[0].clone()).is_none());
        assert!(reasm.push(packets[0].clone()).is_none()); // duplicate ignored

        // Push remaining fragments
        assert!(reasm.push(packets[1].clone()).is_none());
        let frame = reasm.push(packets[2].clone()).expect("should complete");

        assert_eq!(frame.data, data);
        assert_eq!(frame.timestamp, 7000);
    }

    #[test]
    fn sequence_numbers_increment() {
        let mut frag = Fragmenter::new();

        let p1 = frag.fragment(1, 1000, false, &[1, 2, 3], 100);
        assert_eq!(p1[0].sequence, 0);

        let p2 = frag.fragment(1, 2000, false, &[4, 5, 6], 100);
        assert_eq!(p2[0].sequence, 1);

        // Multiple fragments get sequential sequence numbers
        let p3 = frag.fragment(1, 3000, false, &[0u8; 300], 100);
        assert_eq!(p3[0].sequence, 2);
        assert_eq!(p3[1].sequence, 3);
        assert_eq!(p3[2].sequence, 4);
    }

    #[test]
    fn exact_payload_boundary() {
        init_test_tracing();

        let mut frag = Fragmenter::new();

        // Data exactly at boundary: should produce 1 fragment, not 2
        let data = vec![0u8; 100];
        let packets = frag.fragment(1, 1000, false, &data, 100);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].payload.len(), 100);

        // Data one byte over: should produce 2 fragments
        let data = vec![0u8; 101];
        let packets = frag.fragment(1, 2000, false, &data, 100);
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].payload.len(), 100);
        assert_eq!(packets[1].payload.len(), 1);
    }
}
