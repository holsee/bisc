//! Voice call: 2 peers join, voice packets flow bidirectionally, mute/unmute works.

use std::sync::atomic::Ordering;
use std::time::Duration;

use bisc_media::negotiation::{default_offer, negotiate_as_answerer, negotiate_as_offerer};
use bisc_media::opus_codec::SAMPLES_PER_FRAME;
use bisc_media::voice_pipeline::VoicePipeline;
use bisc_net::channel::Channel;
use tokio::sync::mpsc;

use crate::helpers::{
    init_test_tracing, setup_peer_with_media, wait_for_connection, wait_for_peers,
};

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

/// Two peers establish a voice call with bidirectional audio and mute/unmute control.
#[tokio::test]
async fn two_peer_voice_call_with_mute() {
    init_test_tracing();

    // Setup peers with media protocol
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

    // Discover peers and establish direct connections
    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    let a_id = channel_a.our_endpoint_id();
    let b_id = channel_b.our_endpoint_id();

    wait_for_connection(&mut channel_a, b_id, 20).await;
    wait_for_connection(&mut channel_b, a_id, 20).await;

    let peer_conn_a = channel_a.connection(&b_id).await.unwrap();
    let peer_conn_b = channel_b.connection(&a_id).await.unwrap();

    // Negotiate media: higher ID opens, lower accepts
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

    let offer = default_offer();
    let local_offer = default_offer();

    let opener = opener_conn;
    let accepter = accepter_conn;

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
        .expect("offerer timed out")
        .expect("offerer panicked");
    tokio::time::timeout(Duration::from_secs(10), answerer_handle)
        .await
        .expect("answerer timed out")
        .expect("answerer panicked");

    tracing::info!("negotiation complete");

    // Create voice pipelines
    let (audio_in_a_tx, audio_in_a_rx) = mpsc::unbounded_channel::<Vec<f32>>();
    let (audio_out_a_tx, mut audio_out_a_rx) = mpsc::unbounded_channel::<Vec<f32>>();
    let (audio_in_b_tx, audio_in_b_rx) = mpsc::unbounded_channel::<Vec<f32>>();
    let (audio_out_b_tx, mut audio_out_b_rx) = mpsc::unbounded_channel::<Vec<f32>>();

    let mut pipeline_a = VoicePipeline::new(
        peer_conn_a.connection().clone(),
        audio_in_a_rx,
        audio_out_a_tx,
    )
    .unwrap();
    let mut pipeline_b = VoicePipeline::new(
        peer_conn_b.connection().clone(),
        audio_in_b_rx,
        audio_out_b_tx,
    )
    .unwrap();

    pipeline_a.start().await.unwrap();
    pipeline_b.start().await.unwrap();

    // Inject synthetic audio into both peers
    let producer_a = audio_in_a_tx.clone();
    let audio_producer_a = tokio::spawn(async move {
        let mut idx = 0usize;
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        loop {
            interval.tick().await;
            if producer_a.send(sine_frame(idx)).is_err() {
                break;
            }
            idx += 1;
        }
    });
    let producer_b = audio_in_b_tx.clone();
    let audio_producer_b = tokio::spawn(async move {
        let mut idx = 0usize;
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        loop {
            interval.tick().await;
            if producer_b.send(sine_frame(idx)).is_err() {
                break;
            }
            idx += 1;
        }
    });

    // Drain output channels
    let drain_a = tokio::spawn(async move { while audio_out_a_rx.recv().await.is_some() {} });
    let drain_b = tokio::spawn(async move { while audio_out_b_rx.recv().await.is_some() {} });

    // Verify bidirectional packet flow
    tokio::time::sleep(Duration::from_secs(2)).await;

    let a_sent = pipeline_a.metrics().packets_sent.load(Ordering::Relaxed);
    let b_recv = pipeline_b
        .metrics()
        .packets_received
        .load(Ordering::Relaxed);
    let b_sent = pipeline_b.metrics().packets_sent.load(Ordering::Relaxed);
    let a_recv = pipeline_a
        .metrics()
        .packets_received
        .load(Ordering::Relaxed);

    assert!(a_sent > 0, "A should have sent packets");
    assert!(b_recv > 0, "B should have received packets from A");
    assert!(b_sent > 0, "B should have sent packets");
    assert!(a_recv > 0, "A should have received packets from B");
    tracing::info!(
        a_sent,
        b_recv,
        b_sent,
        a_recv,
        "bidirectional flow verified"
    );

    // Mute A → no new packets from A
    let a_sent_before = pipeline_a.metrics().packets_sent.load(Ordering::Relaxed);
    pipeline_a.set_muted(true);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let a_sent_after = pipeline_a.metrics().packets_sent.load(Ordering::Relaxed);
    assert_eq!(a_sent_before, a_sent_after, "no packets sent while muted");

    // Unmute A → packets resume
    pipeline_a.set_muted(false);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let a_sent_resumed = pipeline_a.metrics().packets_sent.load(Ordering::Relaxed);
    assert!(
        a_sent_resumed > a_sent_after,
        "packets should resume after unmute"
    );

    tracing::info!("mute/unmute verified");

    // Clean shutdown
    drop(audio_in_a_tx);
    drop(audio_in_b_tx);
    audio_producer_a.abort();
    audio_producer_b.abort();
    pipeline_a.stop().await;
    pipeline_b.stop().await;
    drain_a.abort();
    drain_b.abort();
    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}
