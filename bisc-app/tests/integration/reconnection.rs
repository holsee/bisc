//! Reconnection: peer disconnects and rejoins via refreshed ticket.

use std::time::Duration;

use bisc_net::channel::Channel;

use crate::helpers::{
    init_test_tracing, setup_peer, wait_for_peer_left, wait_for_peers, TestTimer,
    PEER_DISCOVERY_TIMEOUT_SECS, PEER_LEFT_TIMEOUT_SECS,
};

/// A peer disconnects abruptly and a new peer joins via a refreshed ticket.
#[tokio::test]
async fn peer_disconnects_new_peer_joins_via_refreshed_ticket() {
    init_test_tracing();
    let mut timer = TestTimer::new("peer_disconnects_new_peer_joins_via_refreshed_ticket");

    // A creates channel
    let (ep_a, gossip_a, _router_a) = setup_peer().await;
    let (mut channel_a, ticket) =
        Channel::create(ep_a.endpoint(), &gossip_a, "alice".to_string(), None)
            .await
            .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // B joins
    let (ep_b, gossip_b, _router_b) = setup_peer().await;
    let mut channel_b = Channel::join(ep_b.endpoint(), &gossip_b, &ticket, "bob".to_string(), None)
        .await
        .unwrap();

    wait_for_peers(&mut channel_a, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    wait_for_peers(&mut channel_b, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    timer.phase("ab_discovery");

    let b_id = channel_b.our_endpoint_id();

    tracing::info!("A and B connected");

    // B leaves gracefully
    channel_b.leave();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // A detects B's departure
    wait_for_peer_left(&mut channel_a, b_id, PEER_LEFT_TIMEOUT_SECS).await;
    timer.phase("b_left");
    tracing::info!("A detected B left");

    // A refreshes ticket for new joiners
    let refreshed_ticket = channel_a.refresh_ticket();

    // C joins via refreshed ticket
    let (ep_c, gossip_c, _router_c) = setup_peer().await;
    let mut channel_c = Channel::join(
        ep_c.endpoint(),
        &gossip_c,
        &refreshed_ticket,
        "charlie".to_string(),
        None,
    )
    .await
    .unwrap();

    // A and C should see each other
    wait_for_peers(&mut channel_a, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    wait_for_peers(&mut channel_c, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    timer.phase("ac_discovery");

    let a_peers = channel_a.peers().await;
    let c_peers = channel_c.peers().await;
    let c_id = channel_c.our_endpoint_id();
    let a_id = channel_a.our_endpoint_id();

    assert!(a_peers.contains_key(&c_id), "A should see C");
    assert_eq!(a_peers[&c_id].display_name, "charlie");
    assert!(c_peers.contains_key(&a_id), "C should see A");
    assert_eq!(c_peers[&a_id].display_name, "alice");

    tracing::info!("reconnection via refreshed ticket verified");

    // Cleanup
    channel_a.leave();
    channel_c.leave();
    ep_a.close().await;
    ep_b.close().await;
    ep_c.close().await;
}
