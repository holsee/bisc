//! Core protocol types shared across all bisc crates.

use serde::{Deserialize, Serialize};

/// Unique identifier for an endpoint (iroh node).
/// Wraps a 32-byte public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EndpointId(pub [u8; 32]);

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
