//! Core protocol types shared across all bisc crates.

use serde::{Deserialize, Serialize};

/// Unique identifier for an endpoint (iroh node).
/// Wraps a 32-byte public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EndpointId(pub [u8; 32]);

impl EndpointId {
    /// Encode the endpoint ID as a lowercase hex string.
    pub fn to_hex(&self) -> String {
        data_encoding::HEXLOWER.encode(&self.0)
    }

    /// Decode an endpoint ID from a lowercase hex string.
    ///
    /// Returns `None` if the string is not valid hex or not exactly 32 bytes.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let bytes = data_encoding::HEXLOWER.decode(hex.as_bytes()).ok()?;
        if bytes.len() != 32 {
            return None;
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Some(Self(arr))
    }
}

/// Address information for connecting to an endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointAddr {
    pub id: EndpointId,
    /// Direct addresses (IP:port pairs) serialized as strings.
    pub addrs: Vec<String>,
    /// Optional relay URL for NAT traversal fallback.
    pub relay_url: Option<String>,
}

/// Identifier for a gossip topic (derived from channel secret).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TopicId(pub [u8; 32]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_id_hex_roundtrip() {
        let id = EndpointId([42; 32]);
        let hex = id.to_hex();
        let parsed = EndpointId::from_hex(&hex).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn endpoint_id_from_hex_invalid() {
        // Too short
        assert!(EndpointId::from_hex("abcd").is_none());
        // Not hex
        assert!(EndpointId::from_hex("zzzz").is_none());
        // Too long (33 bytes = 66 hex chars)
        let long = "aa".repeat(33);
        assert!(EndpointId::from_hex(&long).is_none());
        // Empty
        assert!(EndpointId::from_hex("").is_none());
    }

    #[test]
    fn endpoint_id_from_hex_exact_32_bytes() {
        let hex = "01".repeat(32);
        let id = EndpointId::from_hex(&hex).unwrap();
        assert_eq!(id.0, [1u8; 32]);
    }
}
