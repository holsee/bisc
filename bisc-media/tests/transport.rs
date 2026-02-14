//! Integration tests for MediaTransport over real QUIC connections.

use std::sync::atomic::Ordering;
use std::time::Duration;

use bisc_media::transport::MediaTransport;
use bisc_net::channel::Channel;
use bisc_net::testing::{
    init_test_tracing, setup_peer_with_media, wait_for_connection, wait_for_peers, TestTimer,
    CONNECTION_TIMEOUT_SECS, PEER_DISCOVERY_TIMEOUT_SECS,
};
use bisc_protocol::media::MediaPacket;

#[tokio::test]
async fn media_packet_roundtrip_over_quic() {
    init_test_tracing();
    let mut timer = TestTimer::new("media_packet_roundtrip_over_quic");

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
    timer.phase("packet_roundtrip");

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
    let mut timer = TestTimer::new("audio_sized_packets_sent_and_received");

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
    timer.phase("packet_burst");

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
