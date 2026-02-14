//! Integration tests for the end-to-end video pipeline over real QUIC connections.
//!
//! Uses small synthetic frames (32x32) to keep encoding fast.

#![cfg(feature = "video-codec")]

use std::sync::atomic::Ordering;
use std::time::Duration;

use bisc_media::negotiation::{default_offer, negotiate_as_answerer, negotiate_as_offerer};
use bisc_media::video_pipeline::{VideoConfig, VideoPipeline};
use bisc_media::video_types::{PixelFormat, RawFrame};
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

/// Generate a synthetic RGBA test frame (gradient pattern).
fn synthetic_frame(width: u32, height: u32, frame_index: u32) -> RawFrame {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            data[idx] = ((x * 255 / width) as u8).wrapping_add(frame_index as u8);
            data[idx + 1] = ((y * 255 / height) as u8).wrapping_add(frame_index as u8);
            data[idx + 2] = 128;
            data[idx + 3] = 255;
        }
    }
    RawFrame {
        data,
        width,
        height,
        format: PixelFormat::Rgba,
    }
}

/// Comprehensive video pipeline integration test.
///
/// 1. Two peers exchange video frames
/// 2. Camera off stops transmission
/// 3. Camera on resumes transmission
/// 4. Keyframe request triggers IDR
/// 5. Quality change adjusts bitrate
/// 6. Clean shutdown
#[tokio::test]
async fn video_pipeline_end_to_end() {
    init_test_tracing();

    let width = 32u32;
    let height = 32u32;
    let config = VideoConfig {
        width,
        height,
        framerate: 15,
        bitrate_bps: 100_000,
    };

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

    let offerer_handle = tokio::spawn(async move {
        let (mut send, mut recv) = opener.open_bi().await.unwrap();
        negotiate_as_offerer(&mut send, &mut recv, &offer)
            .await
            .unwrap()
    });

    let answerer_handle = tokio::spawn(async move {
        let (mut send, mut recv) = accepter.accept_bi().await.unwrap();
        negotiate_as_answerer(&mut send, &mut recv, &local_offer)
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

    // --- Create video pipelines ---
    let conn_a = peer_conn_a.connection().clone();
    let conn_b = peer_conn_b.connection().clone();

    let (frame_in_a_tx, frame_in_a_rx) = mpsc::unbounded_channel::<RawFrame>();
    let (frame_out_a_tx, mut frame_out_a_rx) = mpsc::unbounded_channel::<RawFrame>();
    let (frame_in_b_tx, frame_in_b_rx) = mpsc::unbounded_channel::<RawFrame>();
    let (frame_out_b_tx, mut frame_out_b_rx) = mpsc::unbounded_channel::<RawFrame>();

    let mut pipeline_a = VideoPipeline::new(conn_a, config.clone(), frame_in_a_rx, frame_out_a_tx);
    let mut pipeline_b = VideoPipeline::new(conn_b, config.clone(), frame_in_b_rx, frame_out_b_tx);

    pipeline_a.start().await.unwrap();
    pipeline_b.start().await.unwrap();
    tracing::info!("video pipelines started");

    // --- Inject synthetic frames into both peers ---
    let producer_a_tx = frame_in_a_tx.clone();
    let producer_a = tokio::spawn(async move {
        let mut idx = 0u32;
        let mut interval = tokio::time::interval(Duration::from_millis(66)); // ~15fps
        loop {
            interval.tick().await;
            let frame = synthetic_frame(width, height, idx);
            if producer_a_tx.send(frame).is_err() {
                break;
            }
            idx += 1;
        }
    });

    let producer_b_tx = frame_in_b_tx.clone();
    let producer_b = tokio::spawn(async move {
        let mut idx = 0u32;
        let mut interval = tokio::time::interval(Duration::from_millis(66));
        loop {
            interval.tick().await;
            let frame = synthetic_frame(width, height, idx);
            if producer_b_tx.send(frame).is_err() {
                break;
            }
            idx += 1;
        }
    });

    // Drain output channels
    let drain_a = tokio::spawn(async move { while frame_out_a_rx.recv().await.is_some() {} });
    let drain_b = tokio::spawn(async move { while frame_out_b_rx.recv().await.is_some() {} });

    // --- Test 1: Two peers exchange video fragments ---
    tokio::time::sleep(Duration::from_secs(3)).await;

    let a_frag_sent = pipeline_a.metrics().fragments_sent.load(Ordering::Relaxed);
    let b_frag_recv = pipeline_b
        .metrics()
        .fragments_received
        .load(Ordering::Relaxed);
    let b_frag_sent = pipeline_b.metrics().fragments_sent.load(Ordering::Relaxed);
    let a_frag_recv = pipeline_a
        .metrics()
        .fragments_received
        .load(Ordering::Relaxed);

    tracing::info!(
        a_frag_sent,
        a_frag_recv,
        b_frag_sent,
        b_frag_recv,
        "fragment counts after 3s"
    );

    assert!(
        a_frag_sent > 0,
        "A should have sent fragments, got {a_frag_sent}"
    );
    assert!(
        b_frag_recv > 0,
        "B should have received fragments, got {b_frag_recv}"
    );
    assert!(
        b_frag_sent > 0,
        "B should have sent fragments, got {b_frag_sent}"
    );
    assert!(
        a_frag_recv > 0,
        "A should have received fragments, got {a_frag_recv}"
    );
    tracing::info!("Test 1 passed: video fragments exchanged");

    // --- Test 2: Camera off stops transmission ---
    let a_sent_before = pipeline_a.metrics().fragments_sent.load(Ordering::Relaxed);
    pipeline_a.set_camera_on(false);
    assert!(!pipeline_a.is_camera_on());

    tokio::time::sleep(Duration::from_secs(1)).await;

    let a_sent_after = pipeline_a.metrics().fragments_sent.load(Ordering::Relaxed);
    tracing::info!(
        before = a_sent_before,
        after = a_sent_after,
        "fragment count around camera off"
    );
    assert_eq!(
        a_sent_before, a_sent_after,
        "no fragments should be sent with camera off"
    );
    tracing::info!("Test 2 passed: camera off stops transmission");

    // --- Test 3: Camera on resumes transmission ---
    pipeline_a.set_camera_on(true);
    assert!(pipeline_a.is_camera_on());

    tokio::time::sleep(Duration::from_secs(1)).await;

    let a_sent_resumed = pipeline_a.metrics().fragments_sent.load(Ordering::Relaxed);
    tracing::info!(
        after_off = a_sent_after,
        after_on = a_sent_resumed,
        "fragment count after camera on"
    );
    assert!(
        a_sent_resumed > a_sent_after,
        "fragments should resume: {} > {}",
        a_sent_resumed,
        a_sent_after
    );
    tracing::info!("Test 3 passed: camera on resumes transmission");

    // --- Test 4: Keyframe request triggers IDR ---
    let kf_before = pipeline_a
        .metrics()
        .keyframes_forced
        .load(Ordering::Relaxed);
    pipeline_a.request_keyframe();

    // Wait for the command to be processed (next frame encode cycle)
    tokio::time::sleep(Duration::from_millis(200)).await;

    let kf_after = pipeline_a
        .metrics()
        .keyframes_forced
        .load(Ordering::Relaxed);
    tracing::info!(
        before = kf_before,
        after = kf_after,
        "keyframe forced count"
    );
    assert!(
        kf_after > kf_before,
        "keyframe should have been forced: {} > {}",
        kf_after,
        kf_before
    );
    tracing::info!("Test 4 passed: keyframe request works");

    // --- Test 5: Quality change adjusts bitrate ---
    pipeline_a.set_bitrate(200_000);
    // Just verify it doesn't panic and the pipeline keeps running
    tokio::time::sleep(Duration::from_millis(200)).await;

    let a_sent_post_bitrate = pipeline_a.metrics().fragments_sent.load(Ordering::Relaxed);
    assert!(
        a_sent_post_bitrate > a_sent_resumed,
        "pipeline should still be running after bitrate change"
    );
    tracing::info!("Test 5 passed: bitrate change applied");

    // --- Test 6: Clean shutdown ---
    drop(frame_in_a_tx);
    drop(frame_in_b_tx);
    producer_a.abort();
    producer_b.abort();

    pipeline_a.stop().await;
    pipeline_b.stop().await;
    tracing::info!("Test 6 passed: clean shutdown");

    // --- Cleanup ---
    drain_a.abort();
    drain_b.abort();
    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}
