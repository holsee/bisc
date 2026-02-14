//! SimNetwork integration tests — deterministic, no real I/O.
//!
//! These tests use `start_paused = true` for instant deterministic time,
//! demonstrating that SimNetwork tests complete in <1 second.

use std::time::Duration;

use bisc_net::sim::SimNetwork;
use bisc_net::testing::{init_test_tracing, TestTimer};
use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Simulate a file-transfer-like scenario: sender pushes chunks over a reliable stream,
/// receiver reads them all, verifies integrity. With latency + loss configured, the
/// reliable stream still delivers everything in order.
#[tokio::test(start_paused = true)]
async fn sim_file_transfer_over_reliable_stream() {
    init_test_tracing();
    let mut timer = TestTimer::new("sim_file_transfer_over_reliable_stream");

    let mut sim = SimNetwork::new();
    let (peer_a, peer_b, _ticket) = sim.create_channel().await;

    // Configure adverse network conditions
    sim.set_latency(Duration::from_millis(50)).await;
    sim.set_loss_rate(0.1).await;
    timer.phase("network_setup");

    // Simulate sending a "file" as 10 chunks of 1KB each
    let chunk_size = 1024usize;
    let chunk_count = 10usize;
    let mut original_data = Vec::new();
    for i in 0..chunk_count {
        let chunk: Vec<u8> = (0..chunk_size)
            .map(|j| ((i * chunk_size + j) % 256) as u8)
            .collect();
        original_data.extend_from_slice(&chunk);
    }

    // Open bi-directional stream: sender writes, receiver reads
    let (mut a_send, _a_recv) = peer_a.open_bi_stream().await.unwrap();
    let (_b_send, mut b_recv) = peer_b.accept_bi_stream().await.unwrap();
    timer.phase("stream_open");

    // Send and receive concurrently — the DuplexStream buffer (8KB) is smaller
    // than total data (10KB), so sequential send-then-receive would deadlock.
    let data_to_send = original_data.clone();
    let (_, received) = tokio::join!(
        async {
            for chunk_idx in 0..chunk_count {
                let start = chunk_idx * chunk_size;
                let end = start + chunk_size;
                a_send.write_all(&data_to_send[start..end]).await.unwrap();
            }
            a_send.shutdown().await.unwrap();
        },
        async {
            let mut received = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                match b_recv.read(&mut buf).await.unwrap() {
                    0 => break,
                    n => received.extend_from_slice(&buf[..n]),
                }
            }
            received
        }
    );
    timer.phase("transfer_complete");

    assert_eq!(
        received.len(),
        original_data.len(),
        "received {} bytes, expected {}",
        received.len(),
        original_data.len()
    );
    assert_eq!(received, original_data, "data integrity check failed");

    tracing::info!(
        bytes = original_data.len(),
        chunks = chunk_count,
        "file transfer verified over simulated network"
    );
}

/// Simulate a reconnection scenario: peer A sends datagrams, gets disconnected,
/// messages are lost, peer reconnects, new messages flow again.
/// Uses paused time for deterministic latency measurement.
#[tokio::test(start_paused = true)]
async fn sim_disconnect_reconnect_with_latency() {
    init_test_tracing();
    let mut timer = TestTimer::new("sim_disconnect_reconnect_with_latency");

    let mut sim = SimNetwork::new();
    let a_id = sim.create_peer("alice");
    let b_id = sim.create_peer("bob");
    let (peer_a, peer_b) = sim.connect(a_id, b_id).await;

    sim.set_latency(Duration::from_millis(25)).await;
    timer.phase("setup");

    // Phase 1: Normal communication
    for i in 0..5u32 {
        peer_a
            .send_datagram(Bytes::from(format!("msg-{i}")))
            .await
            .unwrap();
    }

    let mut received_phase1 = 0;
    for _ in 0..5 {
        let _data = peer_b.recv_datagram().await.unwrap();
        received_phase1 += 1;
    }
    assert_eq!(
        received_phase1, 5,
        "all messages should arrive before disconnect"
    );
    timer.phase("phase1_normal");

    // Phase 2: Disconnect — messages should be dropped
    sim.disconnect(a_id).await;

    let msgs_during_disconnect = 10u64;
    for i in 0..msgs_during_disconnect {
        peer_a
            .send_datagram(Bytes::from(format!("dropped-{i}")))
            .await
            .unwrap();
    }

    // Advance time past latency to ensure messages would have arrived if not disconnected
    tokio::time::advance(Duration::from_millis(100)).await;
    tokio::task::yield_now().await;

    let result = tokio::time::timeout(Duration::from_millis(50), peer_b.recv_datagram()).await;
    assert!(
        result.is_err(),
        "no messages should arrive while disconnected"
    );

    let dropped = peer_a.datagrams_dropped();
    assert_eq!(
        dropped, msgs_during_disconnect,
        "all messages during disconnect should be dropped"
    );
    timer.phase("phase2_disconnected");

    // Phase 3: Reconnect — messages flow again
    sim.reconnect(a_id).await;

    for i in 0..3u32 {
        peer_a
            .send_datagram(Bytes::from(format!("after-{i}")))
            .await
            .unwrap();
    }

    let mut received_phase3 = 0;
    for _ in 0..3 {
        let _data = peer_b.recv_datagram().await.unwrap();
        received_phase3 += 1;
    }
    assert_eq!(received_phase3, 3, "messages should flow after reconnect");
    timer.phase("phase3_reconnected");

    // Verify metrics
    let total_sent = peer_a.datagrams_sent();
    let total_dropped = peer_a.datagrams_dropped();
    let total_received = peer_b.datagrams_received();

    // datagrams_sent only counts packets that were successfully routed (not dropped).
    // During disconnect, packets are dropped before being counted as "sent".
    assert_eq!(total_sent, 8, "5 + 3 = 8 successfully routed");
    assert_eq!(total_dropped, 10, "10 dropped during disconnect");
    assert_eq!(total_received, 8, "5 + 3 = 8 received");
    assert_eq!(
        total_sent + total_dropped,
        18,
        "8 routed + 10 dropped = 18 total attempted"
    );

    tracing::info!(
        total_sent,
        total_dropped,
        total_received,
        "disconnect/reconnect scenario verified"
    );
}
