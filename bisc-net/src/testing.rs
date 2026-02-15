//! Shared test utilities for bisc integration tests.
//!
//! Available behind the `test-util` feature or in `#[cfg(test)]` within bisc-net.
//! Provides timing instrumentation, relay-disabled endpoint setup, and common
//! wait helpers that eliminate duplicated test boilerplate.

use std::time::Duration;

use bisc_protocol::types::EndpointId;
use tokio::sync::mpsc;

use crate::channel::{Channel, ChannelEvent};
use crate::connection::MediaProtocol;
use crate::endpoint::BiscEndpoint;
use crate::gossip::GossipHandle;

/// Default timeout for peer discovery in seconds (relay-disabled localhost).
pub const PEER_DISCOVERY_TIMEOUT_SECS: u64 = 5;

/// Default timeout for connection establishment in seconds.
pub const CONNECTION_TIMEOUT_SECS: u64 = 5;

/// Default timeout for peer departure detection in seconds.
pub const PEER_LEFT_TIMEOUT_SECS: u64 = 5;

/// Default poll interval for event draining.
const POLL_INTERVAL_MS: u64 = 100;

/// Initialise a tracing subscriber for tests.
///
/// Respects the `RUST_LOG` environment variable, defaults to `debug`.
/// Uses `with_test_writer()` to integrate with `cargo test` output capture.
/// Safe to call multiple times — subsequent calls are no-ops.
pub fn init_test_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".into()),
        )
        .with_test_writer()
        .try_init();
}

/// Timing instrumentation for test phases.
///
/// Records wall-clock duration of each named phase and logs a summary
/// on drop. Use this at the start of integration tests to identify
/// where time is being spent.
///
/// # Example
///
/// ```ignore
/// let mut timer = TestTimer::new();
/// let (ep, gossip, router) = setup_peer().await;
/// timer.phase("endpoint_create");
/// // ...
/// timer.phase("peer_discovery");
/// // timer logs summary on drop
/// ```
pub struct TestTimer {
    test_name: String,
    start: std::time::Instant,
    last: std::time::Instant,
    phases: Vec<(String, Duration)>,
}

impl TestTimer {
    /// Create a new timer for a test.
    pub fn new(test_name: &str) -> Self {
        let now = std::time::Instant::now();
        tracing::info!(target: "bisc_test::timer", test = test_name, "test started");
        Self {
            test_name: test_name.to_string(),
            start: now,
            last: now,
            phases: Vec::new(),
        }
    }

    /// Record the end of a named phase.
    pub fn phase(&mut self, name: &str) {
        let now = std::time::Instant::now();
        let duration = now - self.last;
        let elapsed = now - self.start;
        tracing::info!(
            target: "bisc_test::timer",
            test = %self.test_name,
            phase = name,
            duration_ms = duration.as_millis() as u64,
            elapsed_ms = elapsed.as_millis() as u64,
            "phase complete"
        );
        self.phases.push((name.to_string(), duration));
        self.last = now;
    }
}

impl Drop for TestTimer {
    fn drop(&mut self) {
        let total = self.start.elapsed();
        for (name, dur) in &self.phases {
            tracing::info!(
                target: "bisc_test::timer",
                test = %self.test_name,
                phase = %name,
                duration_ms = dur.as_millis() as u64,
                "  phase"
            );
        }
        tracing::info!(
            target: "bisc_test::timer",
            test = %self.test_name,
            total_ms = total.as_millis() as u64,
            phase_count = self.phases.len(),
            "test timing summary"
        );
    }
}

/// Create an endpoint + gossip pair for testing (no media protocol, relay disabled).
pub async fn setup_peer() -> (BiscEndpoint, GossipHandle, iroh::protocol::Router) {
    let ep = BiscEndpoint::for_testing()
        .await
        .expect("test endpoint creation failed");
    let (gossip, router) = GossipHandle::new(ep.endpoint());
    (ep, gossip, router)
}

/// Create an endpoint + gossip pair with media protocol support (relay disabled).
pub async fn setup_peer_with_media() -> (
    BiscEndpoint,
    GossipHandle,
    iroh::protocol::Router,
    mpsc::UnboundedReceiver<iroh::endpoint::Connection>,
) {
    let ep = BiscEndpoint::for_testing()
        .await
        .expect("test endpoint creation failed");
    let (media_proto, incoming_rx) = MediaProtocol::new();
    let (gossip, router) = GossipHandle::with_protocols(ep.endpoint(), media_proto);
    (ep, gossip, router, incoming_rx)
}

/// Create an endpoint + gossip pair with media and blobs protocols (relay disabled).
pub async fn setup_peer_with_all_protocols(
    blob_store: &iroh_blobs::store::mem::MemStore,
) -> (
    BiscEndpoint,
    GossipHandle,
    iroh::protocol::Router,
    mpsc::UnboundedReceiver<iroh::endpoint::Connection>,
) {
    let ep = BiscEndpoint::for_testing()
        .await
        .expect("test endpoint creation failed");
    let (media_proto, incoming_rx) = MediaProtocol::new();
    let blobs_protocol = iroh_blobs::BlobsProtocol::new(blob_store, None);
    let (gossip, router) =
        GossipHandle::with_all_protocols(ep.endpoint(), media_proto, blobs_protocol);
    (ep, gossip, router, incoming_rx)
}

/// Wait for a channel to see at least `expected_count` peers.
///
/// Panics if the timeout is reached.
pub async fn wait_for_peers(channel: &mut Channel, expected_count: usize, timeout_secs: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let peers = channel.peers().await;
        if peers.len() >= expected_count {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!(
                "timed out waiting for {} peers, got {} (after {}s)",
                expected_count,
                peers.len(),
                timeout_secs,
            );
        }
        match tokio::time::timeout(
            Duration::from_millis(POLL_INTERVAL_MS),
            channel.recv_event(),
        )
        .await
        {
            Ok(Some(_)) => {}
            _ => {}
        }
    }
}

/// Wait for a specific peer to appear as connected.
///
/// Returns `true` if connected, `false` on timeout.
pub async fn wait_for_connection(
    channel: &mut Channel,
    peer_id: EndpointId,
    timeout_secs: u64,
) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if channel.connection(&peer_id).await.is_some() {
            return true;
        }
        if tokio::time::Instant::now() > deadline {
            return false;
        }
        match tokio::time::timeout(
            Duration::from_millis(POLL_INTERVAL_MS),
            channel.recv_event(),
        )
        .await
        {
            Ok(Some(ChannelEvent::PeerConnected(id))) if id == peer_id => return true,
            Ok(Some(_)) => {}
            _ => {}
        }
    }
}

/// Wait for a specific peer to leave.
///
/// Panics if the timeout is reached.
pub async fn wait_for_peer_left(channel: &mut Channel, peer_id: EndpointId, timeout_secs: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let peers = channel.peers().await;
        if !peers.contains_key(&peer_id) {
            return;
        }
        if tokio::time::Instant::now() > deadline {
            panic!(
                "timed out waiting for peer {:?} to leave (after {}s)",
                peer_id, timeout_secs
            );
        }
        match tokio::time::timeout(
            Duration::from_millis(POLL_INTERVAL_MS),
            channel.recv_event(),
        )
        .await
        {
            Ok(Some(_)) => {}
            _ => {}
        }
    }
}
