//! Integration tests for MediaTransport over real QUIC connections.

use std::sync::atomic::Ordering;
use std::time::Duration;

use bisc_media::transport::MediaTransport;
use bisc_net::channel::{Channel, ChannelEvent};
use bisc_net::connection::MediaProtocol;
use bisc_net::endpoint::BiscEndpoint;
use bisc_net::gossip::GossipHandle;
use bisc_protocol::media::MediaPacket;
use bisc_protocol::types::EndpointId;
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

/// Wait for peers to discover each other.
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

/// Wait for a PeerConnected event.
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

#[tokio::test]
async fn media_packet_roundtrip_over_quic() {
    init_test_tracing();

    // Set up two peers with media connections
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

    // Get the underlying connections for transport
    let conn_a = channel_a
        .connection(&b_id)
        .await
        .expect("A should have connection to B");
    let conn_b = channel_b
        .connection(&a_id)
        .await
        .expect("B should have connection to A");

    let mut transport_a = MediaTransport::new(conn_a.connection().clone());
    let mut transport_b = MediaTransport::new(conn_b.connection().clone());

    // Send a media packet from A to B
    let mut packet = MediaPacket {
        stream_id: 0,
        sequence: 0, // will be overwritten
        timestamp: 48_000,
        fragment_index: 0,
        fragment_count: 1,
        is_keyframe: false,
        payload: vec![0xAA; 100],
    };

    transport_a.send_media_packet(&mut packet).unwrap();

    // Receive on B
    let received = tokio::time::timeout(Duration::from_secs(5), transport_b.recv_media_packet())
        .await
        .expect("timed out waiting for packet")
        .expect("failed to receive packet");

    assert_eq!(received.stream_id, 0);
    assert_eq!(received.sequence, 0);
    assert_eq!(received.timestamp, 48_000);
    assert_eq!(received.payload, vec![0xAA; 100]);

    // Check metrics
    assert_eq!(
        transport_a.metrics().packets_sent.load(Ordering::Relaxed),
        1
    );
    assert_eq!(
        transport_b
            .metrics()
            .packets_received
            .load(Ordering::Relaxed),
        1
    );

    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}

#[tokio::test]
async fn audio_sized_packets_sent_and_received() {
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

    let conn_a = channel_a.connection(&b_id).await.unwrap();
    let conn_b = channel_b.connection(&a_id).await.unwrap();

    let mut transport_a = MediaTransport::new(conn_a.connection().clone());
    let mut transport_b = MediaTransport::new(conn_b.connection().clone());

    // Send 10 audio-sized packets (typical Opus frame at 64kbps = ~160 bytes)
    let packet_count = 10;
    for i in 0..packet_count {
        let mut packet = MediaPacket {
            stream_id: 0,
            sequence: 0,
            timestamp: i * 960, // 20ms at 48kHz
            fragment_index: 0,
            fragment_count: 1,
            is_keyframe: false,
            payload: vec![0xBB; 160], // typical Opus frame size
        };
        transport_a.send_media_packet(&mut packet).unwrap();
    }

    // Receive all packets on B
    for i in 0..packet_count {
        let received =
            tokio::time::timeout(Duration::from_secs(5), transport_b.recv_media_packet())
                .await
                .expect("timed out waiting for audio packet")
                .expect("failed to receive audio packet");

        assert_eq!(received.stream_id, 0);
        assert_eq!(received.sequence, i);
        assert_eq!(received.timestamp, i * 960);
        assert_eq!(received.payload.len(), 160);
    }

    assert_eq!(
        transport_a.metrics().packets_sent.load(Ordering::Relaxed),
        packet_count as u64
    );
    assert_eq!(
        transport_b
            .metrics()
            .packets_received
            .load(Ordering::Relaxed),
        packet_count as u64
    );

    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}
