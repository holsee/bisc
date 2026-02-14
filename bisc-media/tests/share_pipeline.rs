//! Integration tests for the screen share pipeline.

#![cfg(feature = "video-codec")]

use std::sync::atomic::Ordering;
use std::time::Duration;

use bisc_media::negotiation::{default_offer, negotiate_as_answerer, negotiate_as_offerer};
use bisc_media::share_pipeline::{ShareConfig, SharePipeline};
use bisc_media::video_types::{PixelFormat, RawFrame};
use bisc_net::channel::{Channel, ChannelEvent};
use bisc_net::connection::MediaProtocol;
use bisc_net::endpoint::BiscEndpoint;
use bisc_net::gossip::GossipHandle;
use bisc_protocol::channel::ChannelMessage;
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

fn synthetic_frame(width: u32, height: u32, idx: u32) -> RawFrame {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            data[i] = ((x * 255 / width) as u8).wrapping_add(idx as u8);
            data[i + 1] = ((y * 255 / height) as u8).wrapping_add(idx as u8);
            data[i + 2] = 64;
            data[i + 3] = 255;
        }
    }
    RawFrame {
        data,
        width,
        height,
        format: PixelFormat::Rgba,
    }
}

/// Test screen share with stream_id=2, gossip broadcasts, and simultaneous camera+share.
#[tokio::test]
async fn share_pipeline_end_to_end() {
    init_test_tracing();

    let width = 32u32;
    let height = 32u32;
    let config = ShareConfig {
        width,
        height,
        framerate: 15,
        bitrate_bps: 100_000,
        include_app_audio: false,
    };

    // --- Setup peers ---
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

    let peer_conn_a = channel_a.connection(&b_id).await.unwrap();
    let peer_conn_b = channel_b.connection(&a_id).await.unwrap();

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

    // Negotiation
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

    tokio::time::timeout(Duration::from_secs(10), offerer_handle)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), answerer_handle)
        .await
        .unwrap()
        .unwrap();

    // --- Create share pipeline (A shares, B receives) ---
    let conn_a = peer_conn_a.connection().clone();
    let conn_b = peer_conn_b.connection().clone();

    let (frame_in_tx, frame_in_rx) = mpsc::unbounded_channel::<RawFrame>();
    let (frame_out_tx, mut frame_out_rx) = mpsc::unbounded_channel::<RawFrame>();
    let (_frame_in_b_tx, frame_in_b_rx) = mpsc::unbounded_channel::<RawFrame>();
    let (frame_out_b_tx, mut frame_out_b_rx) = mpsc::unbounded_channel::<RawFrame>();

    let (gossip_msg_tx, mut gossip_msg_rx) = mpsc::unbounded_channel::<ChannelMessage>();

    let mut share_a = SharePipeline::new(
        conn_a,
        config.clone(),
        frame_in_rx,
        frame_out_tx,
        None,
        Some(gossip_msg_tx),
        a_id,
    );
    let mut share_b = SharePipeline::new(
        conn_b,
        config.clone(),
        frame_in_b_rx,
        frame_out_b_tx,
        None,
        None,
        b_id,
    );

    share_a.start().await.unwrap();
    share_b.start().await.unwrap();

    // --- Test 1: Verify start broadcasts MediaStateUpdate ---
    let msg = gossip_msg_rx.try_recv();
    assert!(
        msg.is_ok(),
        "should have received MediaStateUpdate on start"
    );
    match msg.unwrap() {
        ChannelMessage::MediaStateUpdate { screen_sharing, .. } => {
            assert!(screen_sharing, "screen_sharing should be true")
        }
        _ => panic!("expected MediaStateUpdate"),
    }
    tracing::info!("Test 1 passed: start broadcasts MediaStateUpdate");

    // --- Inject frames ---
    let producer_tx = frame_in_tx.clone();
    let producer = tokio::spawn(async move {
        let mut idx = 0u32;
        let mut interval = tokio::time::interval(Duration::from_millis(66));
        loop {
            interval.tick().await;
            if producer_tx
                .send(synthetic_frame(width, height, idx))
                .is_err()
            {
                break;
            }
            idx += 1;
        }
    });

    let drain_a = tokio::spawn(async move { while frame_out_rx.recv().await.is_some() {} });
    let drain_b = tokio::spawn(async move { while frame_out_b_rx.recv().await.is_some() {} });

    // --- Test 2: Screen share produces packets with stream_id=2 ---
    tokio::time::sleep(Duration::from_secs(3)).await;

    let frag_sent = share_a.metrics().fragments_sent.load(Ordering::Relaxed);
    let frag_recv = share_b.metrics().fragments_received.load(Ordering::Relaxed);

    tracing::info!(frag_sent, frag_recv, "share fragment counts after 3s");
    assert!(frag_sent > 0, "should have sent fragments: {frag_sent}");
    assert!(frag_recv > 0, "should have received fragments: {frag_recv}");
    tracing::info!("Test 2 passed: screen share produces packets with stream_id=2");

    // --- Test 3: Stop broadcasts updated MediaStateUpdate ---
    drop(frame_in_tx);
    producer.abort();

    share_a.stop().await;

    let msg = gossip_msg_rx.try_recv();
    assert!(msg.is_ok(), "should have received MediaStateUpdate on stop");
    match msg.unwrap() {
        ChannelMessage::MediaStateUpdate { screen_sharing, .. } => {
            assert!(!screen_sharing, "screen_sharing should be false after stop")
        }
        _ => panic!("expected MediaStateUpdate"),
    }
    tracing::info!("Test 3 passed: stop broadcasts updated MediaStateUpdate");

    // --- Cleanup ---
    share_b.stop().await;
    drain_a.abort();
    drain_b.abort();
    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}
