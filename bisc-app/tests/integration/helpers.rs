//! Shared test helpers for multi-peer integration tests.
//!
//! Re-exports from `bisc_net::testing` for convenience. All test infrastructure
//! is consolidated there to avoid duplication across crates.

pub use bisc_net::testing::{
    init_test_tracing, setup_peer, setup_peer_with_media, wait_for_connection, wait_for_peer_left,
    wait_for_peers, TestTimer, CONNECTION_TIMEOUT_SECS, PEER_DISCOVERY_TIMEOUT_SECS,
    PEER_LEFT_TIMEOUT_SECS,
};
