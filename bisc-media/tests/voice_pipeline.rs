//! Integration tests for the end-to-end voice pipeline over real QUIC connections.
//!
//! Uses a single connection pair to test all pipeline scenarios,
//! avoiding repeated expensive channel setup.

use std::sync::atomic::Ordering;
use std::time::Duration;

use bisc_media::negotiation::{default_offer, negotiate_as_answerer, negotiate_as_offerer};
use bisc_media::opus_codec::SAMPLES_PER_FRAME;
use bisc_media::voice_pipeline::VoicePipeline;
use bisc_net::testing::{
    init_test_tracing, setup_peer_with_media, wait_for_connection, wait_for_peers, TestTimer,
    CONNECTION_TIMEOUT_SECS, PEER_DISCOVERY_TIMEOUT_SECS,
};
use tokio::sync::mpsc;

/// Generate a 440Hz sine wave frame (mono, 960 samples = 20ms at 48kHz).
fn sine_frame(frame_index: usize) -> Vec<f32> {
    let offset = frame_index * SAMPLES_PER_FRAME;
    (0..SAMPLES_PER_FRAME)
        .map(|i| {
            let t = (offset + i) as f32 / 48_000.0;
            (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5
        })
        .collect()
}

/// Comprehensive voice pipeline test covering all acceptance criteria.
///
/// 1. Two peers establish a channel, negotiate, and start voice pipelines
/// 2. Audio packets flow (verify metrics after ~2s)
/// 3. Muting stops packet transmission
/// 4. Unmuting resumes transmission
/// 5. Clean shutdown without panic
#[tokio::test]
async fn voice_pipeline_end_to_end() {
    init_test_tracing();
    let mut timer = TestTimer::new("voice_pipeline_end_to_end");

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

    // --- Negotiation ---
    let offer = default_offer();
    let local_offer = default_offer();

    let opener = opener_conn.clone();
    let accepter = accepter_conn.clone();
    let neg_offer = offer.clone();
    let neg_local = local_offer;

    let offerer_handle = tokio::spawn(async move {
        let (mut send, mut recv) = opener.open_bi().await.unwrap();
        negotiate_as_offerer(&mut send, &mut recv, &neg_offer)
            .await
            .unwrap()
    });

    let answerer_handle = tokio::spawn(async move {
        let (mut send, mut recv) = accepter.accept_bi().await.unwrap();
        negotiate_as_answerer(&mut send, &mut recv, &neg_local)
            .await
            .unwrap()
    });

    let _answer_offerer = tokio::time::timeout(Duration::from_secs(10), offerer_handle)
        .await
        .expect("offerer timed out")
        .expect("offerer panicked");
    let _answer_answerer = tokio::time::timeout(Duration::from_secs(10), answerer_handle)
        .await
        .expect("answerer timed out")
        .expect("answerer panicked");

    tracing::info!("negotiation complete");

    timer.phase("negotiation");

    // --- Create voice pipelines ---
    let conn_a = peer_conn_a.connection().clone();
    let conn_b = peer_conn_b.connection().clone();

    let (audio_in_a_tx, audio_in_a_rx) = mpsc::unbounded_channel::<Vec<f32>>();
    let (audio_out_a_tx, mut audio_out_a_rx) = mpsc::unbounded_channel::<Vec<f32>>();

    let (audio_in_b_tx, audio_in_b_rx) = mpsc::unbounded_channel::<Vec<f32>>();
    let (audio_out_b_tx, mut audio_out_b_rx) = mpsc::unbounded_channel::<Vec<f32>>();

    let mut pipeline_a = VoicePipeline::new(conn_a, audio_in_a_rx, audio_out_a_tx).unwrap();
    let mut pipeline_b = VoicePipeline::new(conn_b, audio_in_b_rx, audio_out_b_tx).unwrap();

    pipeline_a.start().await.unwrap();
    pipeline_b.start().await.unwrap();

    tracing::info!("voice pipelines started");

    timer.phase("pipeline_setup");

    // --- Inject synthetic audio into peer A ---
    // Spawn a producer that sends sine wave frames at ~20ms intervals
    let producer_a = audio_in_a_tx.clone();
    let audio_producer = tokio::spawn(async move {
        let mut frame_idx = 0usize;
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        loop {
            interval.tick().await;
            let frame = sine_frame(frame_idx);
            if producer_a.send(frame).is_err() {
                break;
            }
            frame_idx += 1;
        }
    });

    // Also inject audio into peer B so both sides send
    let producer_b = audio_in_b_tx.clone();
    let audio_producer_b = tokio::spawn(async move {
        let mut frame_idx = 0usize;
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        loop {
            interval.tick().await;
            let frame = sine_frame(frame_idx);
            if producer_b.send(frame).is_err() {
                break;
            }
            frame_idx += 1;
        }
    });

    // Drain output channels so they don't back up.
    let drain_a = tokio::spawn(async move { while audio_out_a_rx.recv().await.is_some() {} });
    let drain_b = tokio::spawn(async move { while audio_out_b_rx.recv().await.is_some() {} });

    // --- Test 1: packets flow in both directions ---
    tokio::time::sleep(Duration::from_secs(2)).await;

    let a_sent = pipeline_a.metrics().packets_sent.load(Ordering::Relaxed);
    let a_recv = pipeline_a
        .metrics()
        .packets_received
        .load(Ordering::Relaxed);
    let b_sent = pipeline_b.metrics().packets_sent.load(Ordering::Relaxed);
    let b_recv = pipeline_b
        .metrics()
        .packets_received
        .load(Ordering::Relaxed);

    tracing::info!(a_sent, a_recv, b_sent, b_recv, "packet counts after 2s");

    assert!(a_sent > 0, "A should have sent packets, got {a_sent}");
    assert!(
        b_recv > 0,
        "B should have received packets from A, got {b_recv}"
    );
    assert!(b_sent > 0, "B should have sent packets, got {b_sent}");
    assert!(
        a_recv > 0,
        "A should have received packets from B, got {a_recv}"
    );
    tracing::info!("Test 1 passed: packets flow in both directions");

    timer.phase("test1_packet_flow");

    // --- Test 2: muting stops packet transmission ---
    let a_sent_before_mute = pipeline_a.metrics().packets_sent.load(Ordering::Relaxed);
    pipeline_a.set_muted(true);
    assert!(pipeline_a.is_muted());

    tokio::time::sleep(Duration::from_secs(1)).await;

    let a_sent_after_mute = pipeline_a.metrics().packets_sent.load(Ordering::Relaxed);
    tracing::info!(
        before = a_sent_before_mute,
        after = a_sent_after_mute,
        "packet count around mute"
    );
    assert_eq!(
        a_sent_before_mute, a_sent_after_mute,
        "no packets should be sent while muted"
    );
    tracing::info!("Test 2 passed: muting stops packet transmission");

    timer.phase("test2_mute");

    // --- Test 3: unmuting resumes transmission ---
    pipeline_a.set_muted(false);
    assert!(!pipeline_a.is_muted());

    tokio::time::sleep(Duration::from_secs(1)).await;

    let a_sent_after_unmute = pipeline_a.metrics().packets_sent.load(Ordering::Relaxed);
    tracing::info!(
        after_mute = a_sent_after_mute,
        after_unmute = a_sent_after_unmute,
        "packet count around unmute"
    );
    assert!(
        a_sent_after_unmute > a_sent_after_mute,
        "packets should resume after unmute: {} > {}",
        a_sent_after_unmute,
        a_sent_after_mute
    );
    tracing::info!("Test 3 passed: unmuting resumes transmission");

    timer.phase("test3_unmute");

    // --- Test 4: clean shutdown ---
    // Stop audio producers first
    drop(audio_in_a_tx);
    drop(audio_in_b_tx);
    audio_producer.abort();
    audio_producer_b.abort();

    pipeline_a.stop().await;
    pipeline_b.stop().await;
    tracing::info!("Test 4 passed: clean shutdown");

    timer.phase("test4_shutdown");

    // --- Cleanup ---
    drain_a.abort();
    drain_b.abort();
    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}
