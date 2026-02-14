//! Integration tests for the end-to-end voice pipeline over real QUIC connections.
//!
//! Uses a single connection pair to test all pipeline scenarios,
//! avoiding repeated expensive channel setup.

use std::sync::atomic::Ordering;
use std::time::Duration;

use bisc_media::negotiation::{default_offer, negotiate_as_answerer, negotiate_as_offerer};
use bisc_media::opus_codec::SAMPLES_PER_FRAME;
use bisc_media::voice_pipeline::VoicePipeline;
use bisc_net::channel::{Channel, ChannelEvent};
use bisc_net::connection::MediaProtocol;
use bisc_net::endpoint::BiscEndpoint;
use bisc_net::gossip::GossipHandle;
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
            Ok(Some(_)) => {}
            _ => {}
        }
    }
}

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

    // --- Setup: establish channel and connection ---
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

    // --- Test 4: clean shutdown ---
    // Stop audio producers first
    drop(audio_in_a_tx);
    drop(audio_in_b_tx);
    audio_producer.abort();
    audio_producer_b.abort();

    pipeline_a.stop().await;
    pipeline_b.stop().await;
    tracing::info!("Test 4 passed: clean shutdown");

    // --- Cleanup ---
    drain_a.abort();
    drain_b.abort();
    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}
