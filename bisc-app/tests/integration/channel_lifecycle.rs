//! Channel lifecycle: create, 3 peers join, peers leave, channel survives.

use std::time::Duration;

use bisc_net::channel::Channel;

use crate::helpers::{init_test_tracing, setup_peer, wait_for_peer_left, wait_for_peers};

/// Create a channel, 3 peers join, one leaves, remaining peers still see each other.
#[tokio::test]
async fn three_peers_join_one_leaves_channel_survives() {
    init_test_tracing();

    // Peer A creates channel
    let (ep_a, gossip_a, _router_a) = setup_peer().await;
    let (mut channel_a, ticket) =
        Channel::create(ep_a.endpoint(), &gossip_a, "alice".to_string(), None)
            .await
            .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Peer B joins
    let (ep_b, gossip_b, _router_b) = setup_peer().await;
    let mut channel_b = Channel::join(ep_b.endpoint(), &gossip_b, &ticket, "bob".to_string(), None)
        .await
        .unwrap();

    // Wait for A and B to discover each other
    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    // Peer C joins
    let (ep_c, gossip_c, _router_c) = setup_peer().await;
    let mut channel_c = Channel::join(
        ep_c.endpoint(),
        &gossip_c,
        &ticket,
        "charlie".to_string(),
        None,
    )
    .await
    .unwrap();

    // All three should see each other
    wait_for_peers(&mut channel_a, 2, 20).await;
    wait_for_peers(&mut channel_b, 2, 20).await;
    wait_for_peers(&mut channel_c, 2, 20).await;

    tracing::info!("all 3 peers see each other");

    // B leaves gracefully
    let b_id = channel_b.our_endpoint_id();
    channel_b.leave();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // A and C should detect B's departure
    wait_for_peer_left(&mut channel_a, b_id, 20).await;
    wait_for_peer_left(&mut channel_c, b_id, 20).await;

    // A and C still see each other
    let a_peers = channel_a.peers().await;
    let c_peers = channel_c.peers().await;
    let c_id = channel_c.our_endpoint_id();
    let a_id = channel_a.our_endpoint_id();

    assert!(
        a_peers.contains_key(&c_id),
        "A should still see C after B left"
    );
    assert!(
        c_peers.contains_key(&a_id),
        "C should still see A after B left"
    );
    assert_eq!(a_peers.len(), 1, "A should see exactly 1 peer");
    assert_eq!(c_peers.len(), 1, "C should see exactly 1 peer");

    tracing::info!("channel survived peer departure");

    // Cleanup
    channel_a.leave();
    channel_c.leave();
    ep_a.close().await;
    ep_b.close().await;
    ep_c.close().await;
}
