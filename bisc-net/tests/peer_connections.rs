//! Integration tests for direct peer connections.

use std::time::Duration;

use bisc_net::channel::{Channel, ChannelEvent};
use bisc_net::connection::MediaProtocol;
use bisc_net::endpoint::BiscEndpoint;
use bisc_net::gossip::GossipHandle;
use bisc_protocol::types::EndpointId;
use bytes::Bytes;
use tokio::sync::mpsc;

fn init_test_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".into()),
        )
        .with_test_writer()
        .try_init();
}

/// Helper: create an endpoint + gossip pair with media protocol support.
async fn setup_peer_with_media() -> (
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

/// Wait for a specific number of peers to appear.
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
        match tokio::time::timeout(Duration::from_millis(500), channel.recv_event()).await {
            Ok(Some(_event)) => {}
            _ => {}
        }
    }
}

/// Wait for a PeerConnected event for a specific peer.
async fn wait_for_connection(
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
        match tokio::time::timeout(Duration::from_millis(500), channel.recv_event()).await {
            Ok(Some(ChannelEvent::PeerConnected(id))) if id == peer_id => return true,
            Ok(Some(_)) => {}
            _ => {}
        }
    }
}

/// Wait for a peer to leave.
async fn wait_for_peer_left(channel: &mut Channel, peer_id: EndpointId, timeout_secs: u64) {
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
async fn peers_auto_connect_after_discovery() {
    init_test_tracing();

    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

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

    // Wait for peer discovery
    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    let a_id = channel_a.our_endpoint_id();
    let b_id = channel_b.our_endpoint_id();

    // Wait for direct connection to be established on both sides
    let a_connected = wait_for_connection(&mut channel_a, b_id, 20).await;
    let b_connected = wait_for_connection(&mut channel_b, a_id, 20).await;

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

    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

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

    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    let a_id = channel_a.our_endpoint_id();
    let b_id = channel_b.our_endpoint_id();

    wait_for_connection(&mut channel_a, b_id, 20).await;
    wait_for_connection(&mut channel_b, a_id, 20).await;

    // The initiator has the outgoing connection (higher ID connects)
    // The receiver has the incoming connection (via MediaProtocol)
    // Both connections point to the same QUIC connection pair
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

    assert_eq!(received, payload);

    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}

#[tokio::test]
async fn reliable_stream_data_exchange() {
    init_test_tracing();

    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

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

    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    let a_id = channel_a.our_endpoint_id();
    let b_id = channel_b.our_endpoint_id();

    wait_for_connection(&mut channel_a, b_id, 20).await;
    wait_for_connection(&mut channel_b, a_id, 20).await;

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

    assert_eq!(received, b"hello stream");

    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}

#[tokio::test]
async fn peer_leave_closes_connection() {
    init_test_tracing();

    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

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

    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    wait_for_connection(&mut channel_a, b_id, 20).await;

    // B leaves
    channel_b.leave();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Wait for A to see B leave
    wait_for_peer_left(&mut channel_a, b_id, 20).await;

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

    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

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

    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    // Abruptly close B (simulates crash)
    drop(channel_b);
    ep_b.close().await;

    // A should eventually time out B and clean up
    wait_for_peer_left(&mut channel_a, b_id, 25).await;

    // Connection should be cleaned up
    assert!(
        channel_a.connection(&b_id).await.is_none(),
        "connection to unreachable peer should be cleaned up"
    );

    channel_a.leave();
    ep_a.close().await;
}
