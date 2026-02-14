//! Integration tests for Channel lifecycle: create, join, leave, heartbeat timeout.

use std::time::Duration;

use bisc_net::channel::Channel;
use bisc_net::endpoint::BiscEndpoint;
use bisc_net::gossip::GossipHandle;

fn init_test_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".into()),
        )
        .with_test_writer()
        .try_init();
}

/// Helper: create an endpoint + gossip pair.
async fn setup_peer() -> (BiscEndpoint, GossipHandle, iroh::protocol::Router) {
    let ep = BiscEndpoint::new().await.expect("endpoint creation failed");
    let (gossip, router) = GossipHandle::new(ep.endpoint());
    (ep, gossip, router)
}

/// Wait for a specific number of peers to appear via channel events.
async fn wait_for_peers(channel: &mut Channel, expected_count: usize, timeout_secs: u64) {
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
        // Drain events to process announcements
        match tokio::time::timeout(Duration::from_millis(500), channel.recv_event()).await {
            Ok(Some(_event)) => {}
            _ => {}
        }
    }
}

/// Wait for a specific peer to leave via events.
async fn wait_for_peer_left(
    channel: &mut Channel,
    peer_id: bisc_protocol::types::EndpointId,
    timeout_secs: u64,
) {
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
            Ok(Some(_event)) => {}
            _ => {}
        }
    }
}

#[tokio::test]
async fn create_channel_produces_valid_ticket() {
    init_test_tracing();

    let (ep, gossip, _router) = setup_peer().await;
    let (channel, ticket) = Channel::create(ep.endpoint(), &gossip, "alice".to_string(), None)
        .await
        .unwrap();

    // Ticket should round-trip
    let ticket_str = ticket.to_ticket_string();
    let parsed: bisc_protocol::ticket::BiscTicket = ticket_str.parse().unwrap();
    assert_eq!(ticket.channel_secret, parsed.channel_secret);
    assert!(!ticket.bootstrap_addrs.is_empty());

    channel.leave();
    ep.close().await;
}

#[tokio::test]
async fn two_peers_see_each_other() {
    init_test_tracing();

    // Peer A creates channel
    let (ep_a, gossip_a, _router_a) = setup_peer().await;
    let (mut channel_a, ticket) =
        Channel::create(ep_a.endpoint(), &gossip_a, "alice".to_string(), None)
            .await
            .unwrap();

    // Wait for A to have its address ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Peer B joins via ticket
    let (ep_b, gossip_b, _router_b) = setup_peer().await;
    let mut channel_b = Channel::join(ep_b.endpoint(), &gossip_b, &ticket, "bob".to_string(), None)
        .await
        .unwrap();

    // Both should see each other
    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    let a_peers = channel_a.peers().await;
    let b_peers = channel_b.peers().await;

    assert_eq!(a_peers.len(), 1, "A should see 1 peer");
    assert_eq!(b_peers.len(), 1, "B should see 1 peer");

    // A should see B
    let b_id = channel_b.our_endpoint_id();
    assert!(a_peers.contains_key(&b_id), "A should see B");
    assert_eq!(a_peers[&b_id].display_name, "bob");

    // B should see A
    let a_id = channel_a.our_endpoint_id();
    assert!(b_peers.contains_key(&a_id), "B should see A");
    assert_eq!(b_peers[&a_id].display_name, "alice");

    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}

#[tokio::test]
async fn three_peers_all_see_each_other() {
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

    // Wait for A and B to see each other before C joins
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

    // All three should see each other (each sees 2 peers)
    wait_for_peers(&mut channel_a, 2, 20).await;
    wait_for_peers(&mut channel_b, 2, 20).await;
    wait_for_peers(&mut channel_c, 2, 20).await;

    let a_peers = channel_a.peers().await;
    let b_peers = channel_b.peers().await;
    let c_peers = channel_c.peers().await;

    assert_eq!(a_peers.len(), 2, "A should see 2 peers");
    assert_eq!(b_peers.len(), 2, "B should see 2 peers");
    assert_eq!(c_peers.len(), 2, "C should see 2 peers");

    channel_a.leave();
    channel_b.leave();
    channel_c.leave();
    ep_a.close().await;
    ep_b.close().await;
    ep_c.close().await;
}

#[tokio::test]
async fn peer_leave_removes_from_list() {
    init_test_tracing();

    let (ep_a, gossip_a, _router_a) = setup_peer().await;
    let (mut channel_a, ticket) =
        Channel::create(ep_a.endpoint(), &gossip_a, "alice".to_string(), None)
            .await
            .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let (ep_b, gossip_b, _router_b) = setup_peer().await;
    let mut channel_b = Channel::join(ep_b.endpoint(), &gossip_b, &ticket, "bob".to_string(), None)
        .await
        .unwrap();

    let b_id = channel_b.our_endpoint_id();

    // Wait for both to see each other
    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    // B leaves
    channel_b.leave();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // A should see B removed
    wait_for_peer_left(&mut channel_a, b_id, 20).await;
    let a_peers = channel_a.peers().await;
    assert!(
        !a_peers.contains_key(&b_id),
        "A should no longer see B after leave"
    );

    channel_a.leave();
    ep_a.close().await;
    ep_b.close().await;
}

#[tokio::test]
async fn ticket_refresh_allows_new_peer_to_join() {
    init_test_tracing();

    // A creates channel
    let (ep_a, gossip_a, _router_a) = setup_peer().await;
    let (mut channel_a, _ticket) =
        Channel::create(ep_a.endpoint(), &gossip_a, "alice".to_string(), None)
            .await
            .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    // B joins
    let (ep_b, gossip_b, _router_b) = setup_peer().await;
    let mut channel_b = Channel::join(
        ep_b.endpoint(),
        &gossip_b,
        &_ticket,
        "bob".to_string(),
        None,
    )
    .await
    .unwrap();

    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    // A leaves
    channel_a.leave();
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // B generates a refreshed ticket
    let new_ticket = channel_b.refresh_ticket();

    // C joins via B's refreshed ticket
    let (ep_c, gossip_c, _router_c) = setup_peer().await;
    let mut channel_c = Channel::join(
        ep_c.endpoint(),
        &gossip_c,
        &new_ticket,
        "charlie".to_string(),
        None,
    )
    .await
    .unwrap();

    // B and C should see each other
    wait_for_peers(&mut channel_b, 1, 20).await;
    wait_for_peers(&mut channel_c, 1, 20).await;

    let b_peers = channel_b.peers().await;
    let c_id = channel_c.our_endpoint_id();
    assert!(b_peers.contains_key(&c_id), "B should see C");

    let c_peers = channel_c.peers().await;
    let b_id = channel_b.our_endpoint_id();
    assert!(c_peers.contains_key(&b_id), "C should see B");

    channel_b.leave();
    channel_c.leave();
    ep_a.close().await;
    ep_b.close().await;
    ep_c.close().await;
}

#[tokio::test]
async fn heartbeat_timeout_removes_peer() {
    init_test_tracing();

    let (ep_a, gossip_a, _router_a) = setup_peer().await;
    let (mut channel_a, ticket) =
        Channel::create(ep_a.endpoint(), &gossip_a, "alice".to_string(), None)
            .await
            .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let (ep_b, gossip_b, _router_b) = setup_peer().await;
    let mut channel_b = Channel::join(ep_b.endpoint(), &gossip_b, &ticket, "bob".to_string(), None)
        .await
        .unwrap();

    let b_id = channel_b.our_endpoint_id();

    // Wait for both to see each other
    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    // Drop B's channel abruptly (no explicit leave, simulates crash)
    // The shutdown_tx fires on drop, but let's shut down the endpoint too
    // to truly stop heartbeats
    drop(channel_b);
    ep_b.close().await;

    // A should eventually time out B (after PEER_TIMEOUT = 15s)
    // Wait up to 25 seconds for the timeout to trigger
    wait_for_peer_left(&mut channel_a, b_id, 25).await;

    let a_peers = channel_a.peers().await;
    assert!(
        !a_peers.contains_key(&b_id),
        "A should have timed out B after no heartbeats"
    );

    channel_a.leave();
    ep_a.close().await;
}
