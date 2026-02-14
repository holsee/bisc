//! Integration tests for media control channel over real QUIC connections.
//!
//! Uses a single connection pair to test all control message scenarios,
//! avoiding repeated expensive channel setup.

use std::time::Duration;

use bisc_media::control::{MediaControlReceiver, MediaControlSender, ReceiverReportGenerator};
use bisc_media::jitter_buffer::JitterBuffer;
use bisc_media::transport::MediaTransport;
use bisc_net::testing::{
    init_test_tracing, setup_peer_with_media, wait_for_connection, wait_for_peers, TestTimer,
    CONNECTION_TIMEOUT_SECS, PEER_DISCOVERY_TIMEOUT_SECS,
};
use bisc_protocol::media::{MediaControl, MediaPacket};

/// Comprehensive control channel test: single connection setup, multiple scenarios.
///
/// Tests all acceptance criteria:
/// - RequestKeyframe sent and received
/// - ReceiverReport with correct packet counts
/// - RTT estimation on localhost
/// - Multiple interleaved control message types
#[tokio::test]
async fn control_channel_over_quic() {
    init_test_tracing();
    let mut timer = TestTimer::new("control_channel_over_quic");

    // --- Setup: establish channel and connection ---
    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = bisc_net::channel::Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let (ep_b, gossip_b, _router_b, incoming_b) = setup_peer_with_media().await;
    let mut channel_b = bisc_net::channel::Channel::join(
        ep_b.endpoint(),
        &gossip_b,
        &ticket,
        "bob".to_string(),
        Some(incoming_b),
    )
    .await
    .unwrap();

    timer.phase("channel_setup");

    wait_for_peers(&mut channel_a, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    wait_for_peers(&mut channel_b, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;

    timer.phase("peer_discovery");

    let a_id = channel_a.our_endpoint_id();
    let b_id = channel_b.our_endpoint_id();

    wait_for_connection(&mut channel_a, b_id, CONNECTION_TIMEOUT_SECS).await;
    wait_for_connection(&mut channel_b, a_id, CONNECTION_TIMEOUT_SECS).await;

    timer.phase("connection_established");

    let peer_conn_a = channel_a
        .connection(&b_id)
        .await
        .expect("A should have connection to B");
    let peer_conn_b = channel_b
        .connection(&a_id)
        .await
        .expect("B should have connection to A");

    // Determine roles: higher ID opens bi-streams, lower ID accepts.
    let (opener_conn, accepter_conn) = if a_id.0 > b_id.0 {
        (
            peer_conn_a.connection().clone(),
            peer_conn_b.connection().clone(),
        )
    } else {
        (
            peer_conn_b.connection().clone(),
            peer_conn_a.connection().clone(),
        )
    };

    // --- Test 1: RequestKeyframe sent and received ---
    // Spawn both sides concurrently (QUIC streams need concurrent open/accept).
    let opener = opener_conn.clone();
    let accepter = accepter_conn.clone();

    let sender_handle = tokio::spawn(async move {
        let (send, _recv) = opener.open_bi().await.unwrap();
        let mut sender = MediaControlSender::new(send);
        let msg = MediaControl::RequestKeyframe { stream_id: 1 };
        sender.send(&msg).await.unwrap();
        assert_eq!(sender.messages_sent(), 1);
        msg
    });

    let receiver_handle = tokio::spawn(async move {
        let (_send, recv) = accepter.accept_bi().await.unwrap();
        let mut receiver = MediaControlReceiver::new(recv);
        let received = receiver.recv().await.unwrap();
        assert_eq!(receiver.messages_received(), 1);
        received
    });

    let sent_msg = tokio::time::timeout(Duration::from_secs(10), sender_handle)
        .await
        .expect("sender timed out")
        .expect("sender panicked");
    let received_msg = tokio::time::timeout(Duration::from_secs(10), receiver_handle)
        .await
        .expect("receiver timed out")
        .expect("receiver panicked");
    assert_eq!(sent_msg, received_msg);
    tracing::info!("Test 1 passed: RequestKeyframe sent and received");

    timer.phase("test1_request_keyframe");

    // --- Test 2: Multiple interleaved control message types ---
    let opener = opener_conn.clone();
    let accepter = accepter_conn.clone();

    let messages = vec![
        MediaControl::RequestKeyframe { stream_id: 1 },
        MediaControl::ReceiverReport {
            stream_id: 0,
            packets_received: 100,
            packets_lost: 2,
            jitter: 15,
            rtt_estimate_ms: 25,
        },
        MediaControl::QualityChange {
            stream_id: 1,
            bitrate_bps: 2_000_000,
            resolution: (1280, 720),
            framerate: 30,
        },
        MediaControl::RequestKeyframe { stream_id: 2 },
        MediaControl::ReceiverReport {
            stream_id: 0,
            packets_received: 200,
            packets_lost: 5,
            jitter: 20,
            rtt_estimate_ms: 30,
        },
    ];

    let msgs_to_send = messages.clone();
    let sender_handle = tokio::spawn(async move {
        let (send, _recv) = opener.open_bi().await.unwrap();
        let mut sender = MediaControlSender::new(send);
        for msg in &msgs_to_send {
            sender.send(msg).await.unwrap();
        }
        assert_eq!(sender.messages_sent(), 5);
    });

    let expected_messages = messages.clone();
    let receiver_handle = tokio::spawn(async move {
        let (_send, recv) = accepter.accept_bi().await.unwrap();
        let mut receiver = MediaControlReceiver::new(recv);
        for expected in &expected_messages {
            let received = receiver.recv().await.unwrap();
            assert_eq!(&received, expected);
        }
        assert_eq!(receiver.messages_received(), 5);
    });

    tokio::time::timeout(Duration::from_secs(10), sender_handle)
        .await
        .expect("sender timed out")
        .expect("sender panicked");
    tokio::time::timeout(Duration::from_secs(10), receiver_handle)
        .await
        .expect("receiver timed out")
        .expect("receiver panicked");
    tracing::info!("Test 2 passed: multiple interleaved control messages");

    timer.phase("test2_interleaved_messages");

    // --- Test 3: RTT estimation via bidirectional report exchange ---
    let opener = opener_conn.clone();
    let accepter = accepter_conn.clone();

    // Both sides open their own bi-stream for bidirectional control.
    // Opener sends report, accepter receives and sends back.
    let opener_handle = tokio::spawn(async move {
        let (send, recv) = opener.open_bi().await.unwrap();
        let mut sender = MediaControlSender::new(send);
        let mut receiver = MediaControlReceiver::new(recv);
        let mut gen = ReceiverReportGenerator::new(0);

        // Send a report
        let transport_metrics = bisc_media::transport::TransportMetrics::default();
        let jitter_stats = bisc_media::jitter_buffer::JitterStats::default();
        let report = gen.generate_report(&transport_metrics, &jitter_stats, 0.0);
        sender.send(&report).await.unwrap();

        // Wait for response
        let _response = receiver.recv().await.unwrap();

        // Update RTT
        gen.update_rtt_from_response();
        gen.rtt_estimate_ms()
    });

    let accepter_handle = tokio::spawn(async move {
        let (send, recv) = accepter.accept_bi().await.unwrap();
        let mut sender = MediaControlSender::new(send);
        let mut receiver = MediaControlReceiver::new(recv);
        let mut gen = ReceiverReportGenerator::new(0);

        // Receive report from opener
        let _received = receiver.recv().await.unwrap();

        // Send report back
        let transport_metrics = bisc_media::transport::TransportMetrics::default();
        let jitter_stats = bisc_media::jitter_buffer::JitterStats::default();
        let report = gen.generate_report(&transport_metrics, &jitter_stats, 0.0);
        sender.send(&report).await.unwrap();
    });

    let rtt = tokio::time::timeout(Duration::from_secs(10), opener_handle)
        .await
        .expect("opener timed out")
        .expect("opener panicked");
    tokio::time::timeout(Duration::from_secs(10), accepter_handle)
        .await
        .expect("accepter timed out")
        .expect("accepter panicked");

    tracing::info!(rtt_ms = rtt, "measured RTT on localhost");
    assert!(rtt < 100, "localhost RTT should be < 100ms, got {rtt}ms");
    tracing::info!("Test 3 passed: RTT estimation on localhost");

    timer.phase("test3_rtt_estimation");

    // --- Test 4: ReceiverReport with correct packet counts (via datagrams) ---
    let conn_a_for_transport = peer_conn_a.connection().clone();
    let conn_b_for_transport = peer_conn_b.connection().clone();
    let mut transport_a = MediaTransport::new(conn_a_for_transport);
    let mut transport_b = MediaTransport::new(conn_b_for_transport);

    let num_packets: u32 = 10;
    for i in 0..num_packets {
        let mut pkt = MediaPacket {
            stream_id: 0,
            sequence: 0,
            timestamp: i * 960,
            fragment_index: 0,
            fragment_count: 1,
            is_keyframe: i == 0,
            payload: vec![0xAA; 100],
        };
        transport_a.send_media_packet(&mut pkt).unwrap();
    }

    let mut jitter = JitterBuffer::new(0, 960, 20);
    for _ in 0..num_packets {
        let pkt = tokio::time::timeout(Duration::from_secs(5), transport_b.recv_media_packet())
            .await
            .expect("timed out receiving packet")
            .expect("recv failed");
        jitter.push(pkt);
    }

    while jitter.pop().is_some() {}

    let mut gen = ReceiverReportGenerator::new(0);
    let report = gen.generate_report(
        &transport_b.metrics,
        jitter.stats(),
        jitter.average_jitter_ms(),
    );

    match report {
        MediaControl::ReceiverReport {
            stream_id,
            packets_received,
            packets_lost,
            ..
        } => {
            assert_eq!(stream_id, 0);
            assert_eq!(packets_received, num_packets);
            assert_eq!(packets_lost, 0);
        }
        _ => panic!("expected ReceiverReport"),
    }
    tracing::info!("Test 4 passed: ReceiverReport with correct packet counts");

    timer.phase("test4_receiver_report");

    // --- Cleanup ---
    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}
