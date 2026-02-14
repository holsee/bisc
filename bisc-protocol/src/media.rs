//! Media transport types: packets, negotiation, and control messages.

use serde::{Deserialize, Serialize};

/// Information about a supported codec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodecInfo {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u8,
}

/// Quality preset for audio or video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityPreset {
    Low,
    Medium,
    High,
}

/// A media packet sent as a QUIC datagram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaPacket {
    /// Stream identifier: 0 = audio, 1 = camera, 2 = screen, etc.
    pub stream_id: u8,
    /// Monotonic sequence number.
    pub sequence: u32,
    /// Media timestamp (90kHz for video, 48kHz for audio).
    pub timestamp: u32,
    /// Fragment N of `fragment_count`.
    pub fragment_index: u8,
    /// Total fragments for this frame.
    pub fragment_count: u8,
    /// Whether this fragment belongs to a keyframe.
    pub is_keyframe: bool,
    /// Encoded media data.
    pub payload: Vec<u8>,
}

/// Offered media capabilities during negotiation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaOffer {
    pub audio_codecs: Vec<CodecInfo>,
    pub video_codecs: Vec<CodecInfo>,
    pub max_resolution: (u32, u32),
    pub max_framerate: u32,
}

/// Answer to a media offer with selected parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaAnswer {
    pub selected_audio_codec: CodecInfo,
    pub selected_video_codec: CodecInfo,
    pub negotiated_resolution: (u32, u32),
    pub negotiated_framerate: u32,
}

/// Control messages sent over a reliable QUIC stream per peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaControl {
    RequestKeyframe {
        stream_id: u8,
    },
    ReceiverReport {
        stream_id: u8,
        packets_received: u32,
        packets_lost: u32,
        jitter: u32,
        rtt_estimate_ms: u16,
    },
    QualityChange {
        stream_id: u8,
        bitrate_bps: u32,
        resolution: (u32, u32),
        framerate: u32,
    },
}

/// Serialize a `MediaPacket` to compact binary via postcard.
pub fn encode_media_packet(pkt: &MediaPacket) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(pkt)
}

/// Deserialize a `MediaPacket` from postcard bytes.
pub fn decode_media_packet(data: &[u8]) -> Result<MediaPacket, postcard::Error> {
    postcard::from_bytes(data)
}

/// Serialize a `MediaControl` to compact binary via postcard.
pub fn encode_media_control(ctrl: &MediaControl) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(ctrl)
}

/// Deserialize a `MediaControl` from postcard bytes.
pub fn decode_media_control(data: &[u8]) -> Result<MediaControl, postcard::Error> {
    postcard::from_bytes(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_packet_roundtrip() {
        let pkt = MediaPacket {
            stream_id: 0,
            sequence: 42,
            timestamp: 480000,
            fragment_index: 0,
            fragment_count: 1,
            is_keyframe: false,
            payload: b"opus frame data".to_vec(),
        };
        let encoded = encode_media_packet(&pkt).unwrap();
        let decoded = decode_media_packet(&encoded).unwrap();
        assert_eq!(pkt, decoded);
    }

    #[test]
    fn media_packet_fragmented_roundtrip() {
        for i in 0..3 {
            let pkt = MediaPacket {
                stream_id: 1,
                sequence: 100,
                timestamp: 900000,
                fragment_index: i,
                fragment_count: 3,
                is_keyframe: i == 0,
                payload: vec![i; 1200],
            };
            let encoded = encode_media_packet(&pkt).unwrap();
            let decoded = decode_media_packet(&encoded).unwrap();
            assert_eq!(pkt, decoded);
        }
    }

    #[test]
    fn media_offer_answer_roundtrip() {
        let offer = MediaOffer {
            audio_codecs: vec![CodecInfo {
                name: "opus".to_string(),
                sample_rate: 48000,
                channels: 2,
            }],
            video_codecs: vec![CodecInfo {
                name: "h264".to_string(),
                sample_rate: 90000,
                channels: 0,
            }],
            max_resolution: (1920, 1080),
            max_framerate: 30,
        };
        let encoded: Vec<u8> = postcard::to_allocvec(&offer).unwrap();
        let decoded: MediaOffer = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(offer, decoded);

        let answer = MediaAnswer {
            selected_audio_codec: CodecInfo {
                name: "opus".to_string(),
                sample_rate: 48000,
                channels: 2,
            },
            selected_video_codec: CodecInfo {
                name: "h264".to_string(),
                sample_rate: 90000,
                channels: 0,
            },
            negotiated_resolution: (1280, 720),
            negotiated_framerate: 30,
        };
        let encoded: Vec<u8> = postcard::to_allocvec(&answer).unwrap();
        let decoded: MediaAnswer = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(answer, decoded);
    }

    #[test]
    fn media_control_request_keyframe_roundtrip() {
        let ctrl = MediaControl::RequestKeyframe { stream_id: 1 };
        let encoded = encode_media_control(&ctrl).unwrap();
        let decoded = decode_media_control(&encoded).unwrap();
        assert_eq!(ctrl, decoded);
    }

    #[test]
    fn media_control_receiver_report_roundtrip() {
        let ctrl = MediaControl::ReceiverReport {
            stream_id: 0,
            packets_received: 1000,
            packets_lost: 5,
            jitter: 20,
            rtt_estimate_ms: 45,
        };
        let encoded = encode_media_control(&ctrl).unwrap();
        let decoded = decode_media_control(&encoded).unwrap();
        assert_eq!(ctrl, decoded);
    }

    #[test]
    fn media_control_quality_change_roundtrip() {
        let ctrl = MediaControl::QualityChange {
            stream_id: 1,
            bitrate_bps: 2_000_000,
            resolution: (1280, 720),
            framerate: 30,
        };
        let encoded = encode_media_control(&ctrl).unwrap();
        let decoded = decode_media_control(&encoded).unwrap();
        assert_eq!(ctrl, decoded);
    }
}
