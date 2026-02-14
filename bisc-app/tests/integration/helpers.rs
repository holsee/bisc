//! Shared test helpers for multi-peer integration tests.

use std::time::Duration;

use bisc_net::channel::Channel;
use bisc_net::connection::MediaProtocol;
use bisc_net::endpoint::BiscEndpoint;
use bisc_net::gossip::GossipHandle;
use bisc_protocol::types::EndpointId;
use tokio::sync::mpsc;

pub fn init_test_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".into()),
        )
        .with_test_writer()
        .try_init();
}

/// Create an endpoint + gossip pair (no media protocol).
pub async fn setup_peer() -> (BiscEndpoint, GossipHandle, iroh::protocol::Router) {
    let ep = BiscEndpoint::new().await.expect("endpoint creation failed");
    let (gossip, router) = GossipHandle::new(ep.endpoint());
    (ep, gossip, router)
}

/// Create an endpoint + gossip pair with media protocol support.
pub async fn setup_peer_with_media() -> (
    BiscEndpoint,
    GossipHandle,
    iroh::protocol::Router,
    mpsc::UnboundedReceiver<iroh::endpoint::Connection>,
) {
    let ep = BiscEndpoint::new().await.expect("endpoint creation failed");
    let (media_proto, incoming_rx) = MediaProtocol::new();
    let (gossip, router) = GossipHandle::with_protocols(ep.endpoint(), media_proto);
    (ep, gossip, router, incoming_rx)
}

/// Wait for a channel to see at least `expected_count` peers.
pub async fn wait_for_peers(channel: &mut Channel, expected_count: usize, timeout_secs: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let peers = channel.peers().await;
        if peers.len() >= expected_count {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!(
                "timed out waiting for {} peers, got {}",
                expected_count,
                peers.len()
            );
        }
        match tokio::time::timeout(Duration::from_millis(500), channel.recv_event()).await {
            Ok(Some(_)) => {}
            _ => {}
        }
    }
}

/// Wait for a specific peer to appear as connected.
pub async fn wait_for_connection(
    channel: &mut Channel,
    peer_id: EndpointId,
    timeout_secs: u64,
) -> bool {
    use bisc_net::channel::ChannelEvent;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if channel.connection(&peer_id).await.is_some() {
            return true;
        }
        if tokio::time::Instant::now() > deadline {
            return false;
        }
        match tokio::time::timeout(Duration::from_millis(500), channel.recv_event()).await {
            Ok(Some(ChannelEvent::PeerConnected(id))) if id == peer_id => return true,
            Ok(Some(_)) => {}
            _ => {}
        }
    }
}

/// Wait for a specific peer to leave.
pub async fn wait_for_peer_left(channel: &mut Channel, peer_id: EndpointId, timeout_secs: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let peers = channel.peers().await;
        if !peers.contains_key(&peer_id) {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("timed out waiting for peer {:?} to leave", peer_id);
        }
        match tokio::time::timeout(Duration::from_millis(500), channel.recv_event()).await {
            Ok(Some(_)) => {}
            _ => {}
        }
    }
}
