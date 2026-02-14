//! Integration tests for direct peer connections.

use std::time::Duration;

use bisc_net::channel::Channel;
use bisc_net::testing::{
    init_test_tracing, setup_peer_with_media, wait_for_connection, wait_for_peer_left,
    wait_for_peers, TestTimer, CONNECTION_TIMEOUT_SECS, PEER_DISCOVERY_TIMEOUT_SECS,
    PEER_LEFT_TIMEOUT_SECS,
};
use bytes::Bytes;

#[tokio::test]
async fn peers_auto_connect_after_discovery() {
    init_test_tracing();
    let mut timer = TestTimer::new("peers_auto_connect_after_discovery");

    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let (ep_b, gossip_b, _router_b, incoming_b) = setup_peer_with_media().await;
    let mut channel_b = Channel::join(
        ep_b.endpoint(),
        &gossip_b,
        &ticket,
        "bob".to_string(),
        Some(incoming_b),
    )
    .await
    .unwrap();
    timer.phase("setup");

    // Wait for peer discovery
    wait_for_peers(&mut channel_a, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    wait_for_peers(&mut channel_b, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    timer.phase("peer_discovery");

    let a_id = channel_a.our_endpoint_id();
    let b_id = channel_b.our_endpoint_id();

    // Wait for direct connection to be established on both sides
    let a_connected = wait_for_connection(&mut channel_a, b_id, CONNECTION_TIMEOUT_SECS).await;
    let b_connected = wait_for_connection(&mut channel_b, a_id, CONNECTION_TIMEOUT_SECS).await;
    timer.phase("connection_setup");

    assert!(a_connected, "A should have a direct connection to B");
    assert!(b_connected, "B should have a direct connection to A");

    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}

#[tokio::test]
async fn datagrams_can_be_exchanged() {
    init_test_tracing();
    let mut timer = TestTimer::new("datagrams_can_be_exchanged");

    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let (ep_b, gossip_b, _router_b, incoming_b) = setup_peer_with_media().await;
    let mut channel_b = Channel::join(
        ep_b.endpoint(),
        &gossip_b,
        &ticket,
        "bob".to_string(),
        Some(incoming_b),
    )
    .await
    .unwrap();

    wait_for_peers(&mut channel_a, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    wait_for_peers(&mut channel_b, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;

    let a_id = channel_a.our_endpoint_id();
    let b_id = channel_b.our_endpoint_id();

    wait_for_connection(&mut channel_a, b_id, CONNECTION_TIMEOUT_SECS).await;
    wait_for_connection(&mut channel_b, a_id, CONNECTION_TIMEOUT_SECS).await;
    timer.phase("setup_and_connect");

    let conn_a = channel_a
        .connection(&b_id)
        .await
        .expect("A should have connection to B");

    let conn_b = channel_b
        .connection(&a_id)
        .await
        .expect("B should have connection to A");

    // Send datagram from A to B
    let payload = Bytes::from_static(b"hello datagram");
    conn_a.send_datagram(payload.clone()).unwrap();

    // Receive datagram on B
    let received = tokio::time::timeout(Duration::from_secs(5), conn_b.recv_datagram())
        .await
        .expect("timed out waiting for datagram")
        .expect("failed to receive datagram");
    timer.phase("datagram_exchange");

    assert_eq!(received, payload);

    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}

#[tokio::test]
async fn reliable_stream_data_exchange() {
    init_test_tracing();
    let mut timer = TestTimer::new("reliable_stream_data_exchange");

    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let (ep_b, gossip_b, _router_b, incoming_b) = setup_peer_with_media().await;
    let mut channel_b = Channel::join(
        ep_b.endpoint(),
        &gossip_b,
        &ticket,
        "bob".to_string(),
        Some(incoming_b),
    )
    .await
    .unwrap();

    wait_for_peers(&mut channel_a, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    wait_for_peers(&mut channel_b, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;

    let a_id = channel_a.our_endpoint_id();
    let b_id = channel_b.our_endpoint_id();

    wait_for_connection(&mut channel_a, b_id, CONNECTION_TIMEOUT_SECS).await;
    wait_for_connection(&mut channel_b, a_id, CONNECTION_TIMEOUT_SECS).await;
    timer.phase("setup_and_connect");

    // The peer with higher ID initiated; it opens a bi stream
    let (initiator, acceptor) = if a_id.0 > b_id.0 {
        (
            channel_a.connection(&b_id).await.unwrap(),
            channel_b.connection(&a_id).await.unwrap(),
        )
    } else {
        (
            channel_b.connection(&a_id).await.unwrap(),
            channel_a.connection(&b_id).await.unwrap(),
        )
    };

    // Open a bi stream from initiator
    let (mut send_stream, _recv_stream) = initiator.open_control_stream().await.unwrap();

    // Accept on the other side
    let accept_handle = tokio::spawn(async move {
        let (_send, mut recv) = acceptor.accept_bi().await.unwrap();
        recv.read_to_end(1024).await.unwrap()
    });

    // Write data and finish
    send_stream.write_all(b"hello stream").await.unwrap();
    iroh::endpoint::SendStream::finish(&mut send_stream).unwrap();

    let received = tokio::time::timeout(Duration::from_secs(5), accept_handle)
        .await
        .expect("timed out waiting for stream data")
        .expect("task panicked");
    timer.phase("stream_exchange");

    assert_eq!(received, b"hello stream");

    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}

#[tokio::test]
async fn peer_leave_closes_connection() {
    init_test_tracing();
    let mut timer = TestTimer::new("peer_leave_closes_connection");

    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let (ep_b, gossip_b, _router_b, incoming_b) = setup_peer_with_media().await;
    let mut channel_b = Channel::join(
        ep_b.endpoint(),
        &gossip_b,
        &ticket,
        "bob".to_string(),
        Some(incoming_b),
    )
    .await
    .unwrap();

    let b_id = channel_b.our_endpoint_id();

    wait_for_peers(&mut channel_a, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    wait_for_peers(&mut channel_b, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;

    wait_for_connection(&mut channel_a, b_id, CONNECTION_TIMEOUT_SECS).await;
    timer.phase("setup_and_connect");

    // B leaves
    channel_b.leave();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Wait for A to see B leave
    wait_for_peer_left(&mut channel_a, b_id, PEER_LEFT_TIMEOUT_SECS).await;
    timer.phase("peer_left");

    // A's connection to B should be removed
    assert!(
        channel_a.connection(&b_id).await.is_none(),
        "connection to B should be removed after B leaves"
    );

    channel_a.leave();
    ep_a.close().await;
    ep_b.close().await;
}

#[tokio::test]
async fn connection_handles_unreachable_peer() {
    init_test_tracing();
    let mut timer = TestTimer::new("connection_handles_unreachable_peer");

    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let (ep_b, gossip_b, _router_b, incoming_b) = setup_peer_with_media().await;
    let mut channel_b = Channel::join(
        ep_b.endpoint(),
        &gossip_b,
        &ticket,
        "bob".to_string(),
        Some(incoming_b),
    )
    .await
    .unwrap();

    let b_id = channel_b.our_endpoint_id();

    wait_for_peers(&mut channel_a, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    wait_for_peers(&mut channel_b, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    timer.phase("peer_discovery");

    // Abruptly close B (simulates crash)
    drop(channel_b);
    ep_b.close().await;

    // A should eventually time out B and clean up
    wait_for_peer_left(&mut channel_a, b_id, 25).await;
    timer.phase("crash_timeout");

    // Connection should be cleaned up
    assert!(
        channel_a.connection(&b_id).await.is_none(),
        "connection to unreachable peer should be cleaned up"
    );

    channel_a.leave();
    ep_a.close().await;
}
