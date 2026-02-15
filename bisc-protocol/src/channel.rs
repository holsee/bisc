//! Gossip protocol messages for channel coordination.

use serde::{Deserialize, Serialize};

use crate::types::{EndpointAddr, EndpointId};

/// Capabilities a peer advertises when joining a channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaCapabilities {
    pub audio: bool,
    pub video: bool,
    pub screen_share: bool,
}

/// Messages broadcast on a channel's gossip topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelMessage {
    PeerAnnounce {
        endpoint_id: EndpointId,
        display_name: String,
        capabilities: MediaCapabilities,
    },
    PeerLeave {
        endpoint_id: EndpointId,
    },
    Heartbeat {
        endpoint_id: EndpointId,
        timestamp: u64,
    },
    MediaStateUpdate {
        endpoint_id: EndpointId,
        audio_muted: bool,
        video_enabled: bool,
        screen_sharing: bool,
        app_audio_sharing: bool,
    },
    FileAnnounce {
        endpoint_id: EndpointId,
        file_hash: [u8; 32],
        file_name: String,
        file_size: u64,
    },
    TicketRefresh {
        new_bootstrap: EndpointAddr,
    },
}

/// Serialize a `ChannelMessage` to compact binary via postcard.
pub fn encode_channel_message(msg: &ChannelMessage) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(msg)
}

/// Deserialize a `ChannelMessage` from postcard bytes.
pub fn decode_channel_message(data: &[u8]) -> Result<ChannelMessage, postcard::Error> {
    postcard::from_bytes(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EndpointId;

    #[test]
    fn channel_message_peer_announce_roundtrip() {
        let msg = ChannelMessage::PeerAnnounce {
            endpoint_id: EndpointId([42; 32]),
            display_name: "alice".to_string(),
            capabilities: MediaCapabilities {
                audio: true,
                video: true,
                screen_share: false,
            },
        };
        let encoded = encode_channel_message(&msg).unwrap();
        let decoded = decode_channel_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn channel_message_peer_leave_roundtrip() {
        let msg = ChannelMessage::PeerLeave {
            endpoint_id: EndpointId([1; 32]),
        };
        let encoded = encode_channel_message(&msg).unwrap();
        let decoded = decode_channel_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn channel_message_heartbeat_roundtrip() {
        let msg = ChannelMessage::Heartbeat {
            endpoint_id: EndpointId([2; 32]),
            timestamp: 1234567890,
        };
        let encoded = encode_channel_message(&msg).unwrap();
        let decoded = decode_channel_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn channel_message_media_state_update_roundtrip() {
        let msg = ChannelMessage::MediaStateUpdate {
            endpoint_id: EndpointId([3; 32]),
            audio_muted: true,
            video_enabled: false,
            screen_sharing: true,
            app_audio_sharing: false,
        };
        let encoded = encode_channel_message(&msg).unwrap();
        let decoded = decode_channel_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn channel_message_file_announce_roundtrip() {
        let msg = ChannelMessage::FileAnnounce {
            endpoint_id: EndpointId([4; 32]),
            file_hash: [0xAB; 32],
            file_name: "document.pdf".to_string(),
            file_size: 1_048_576,
        };
        let encoded = encode_channel_message(&msg).unwrap();
        let decoded = decode_channel_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn channel_message_ticket_refresh_roundtrip() {
        let msg = ChannelMessage::TicketRefresh {
            new_bootstrap: EndpointAddr {
                id: EndpointId([5; 32]),
                addrs: vec!["192.168.1.1:12345".to_string()],
                relay_url: Some("https://relay.example.com".to_string()),
            },
        };
        let encoded = encode_channel_message(&msg).unwrap();
        let decoded = decode_channel_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }
}
