//! iroh endpoint, gossip, channels, and peer connections.

pub mod channel;
pub mod connection;
pub mod endpoint;
pub mod gossip;
#[cfg(any(test, feature = "sim"))]
pub mod sim;
#[cfg(any(test, feature = "test-util"))]
pub mod testing;
pub mod transport;

pub use channel::{Channel, ChannelEvent, PeerInfo};
pub use connection::{ConnectionState, MediaProtocol, PeerConnection};
pub use endpoint::BiscEndpoint;
pub use gossip::{GossipEvent, GossipHandle, TopicSubscription};
pub use transport::{Transport, TransportMetrics};
