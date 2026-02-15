//! Graceful degradation: app handles missing hardware and invalid input without crashing.

use std::time::Duration;

use bisc_net::channel::Channel;

use crate::helpers::{init_test_tracing, setup_peer, TestTimer};

/// Channel join with an invalid ticket produces an error, not a panic.
/// The app remains in a recoverable state.
#[tokio::test]
async fn invalid_ticket_returns_error() {
    init_test_tracing();
    let _timer = TestTimer::new("invalid_ticket_returns_error");

    let (ep, gossip, _router) = setup_peer().await;

    // Try to parse a malformed ticket string
    let result = "not-a-valid-ticket".parse::<bisc_protocol::ticket::BiscTicket>();
    assert!(
        result.is_err(),
        "parsing invalid ticket should return an error"
    );
    tracing::info!(error = %result.unwrap_err(), "invalid ticket parsed: error as expected");

    // Try to join with a well-formed but unreachable ticket (no live peers)
    let fake_ticket = create_unreachable_ticket();
    let join_result = tokio::time::timeout(
        Duration::from_secs(5),
        Channel::join(
            ep.endpoint(),
            &gossip,
            &fake_ticket,
            "tester".to_string(),
            None,
        ),
    )
    .await;

    match join_result {
        Ok(Ok(channel)) => {
            // If join succeeded (unlikely with unreachable peers), clean up gracefully.
            tracing::info!("join unexpectedly succeeded (will clean up)");
            channel.leave();
        }
        Ok(Err(e)) => {
            tracing::info!(error = %e, "join failed as expected with unreachable ticket");
        }
        Err(_) => {
            tracing::info!("join timed out as expected (unreachable peers)");
        }
    }

    ep.close().await;
}

/// Voice pipeline handles peer disconnect gracefully.
/// When the connection is closed, the pipeline loops exit without panicking.
#[tokio::test]
async fn voice_pipeline_survives_connection_close() {
    init_test_tracing();
    let _timer = TestTimer::new("voice_pipeline_survives_connection_close");

    use bisc_media::voice_pipeline::{VoiceConfig, VoicePipeline};
    use bisc_net::channel::Channel;
    use tokio::sync::mpsc;

    use crate::helpers::{
        setup_peer_with_media, wait_for_connection, wait_for_peers, CONNECTION_TIMEOUT_SECS,
        PEER_DISCOVERY_TIMEOUT_SECS,
    };

    // Setup two peers
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

    let peer_conn_a = channel_a.connection(&b_id).await.unwrap();

    // Start a voice pipeline on A
    let (_mic_tx, mic_rx) = mpsc::unbounded_channel();
    let (spk_tx, _spk_rx) = mpsc::unbounded_channel();

    let config = VoiceConfig::default();
    let mut pipeline =
        VoicePipeline::new(peer_conn_a.connection().clone(), config, mic_rx, spk_tx).unwrap();
    pipeline.start().await.unwrap();

    // Close B's endpoint abruptly — simulates peer crash
    channel_b.leave();
    ep_b.close().await;

    // Give pipeline time to detect closed connection
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Pipeline stop should complete without panic
    pipeline.stop().await;
    tracing::info!("pipeline survived connection close gracefully");

    channel_a.leave();
    ep_a.close().await;
}

/// No audio hardware: creating a pipeline still works (the pipeline uses channels,
/// not hardware directly). The audio hub layer handles missing hardware.
#[tokio::test]
async fn voice_pipeline_works_without_audio_hardware() {
    init_test_tracing();

    use bisc_media::voice_pipeline::{VoiceConfig, VoicePipeline};
    use bisc_net::channel::Channel;
    use tokio::sync::mpsc;

    use crate::helpers::{
        setup_peer_with_media, wait_for_connection, wait_for_peers, CONNECTION_TIMEOUT_SECS,
        PEER_DISCOVERY_TIMEOUT_SECS,
    };

    // Setup two peers
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

    let peer_conn = channel_a.connection(&b_id).await.unwrap();

    // Create pipeline with channels but no real audio hardware
    // (the mic_rx is immediately dropped, simulating no audio input)
    let (mic_tx, mic_rx) = mpsc::unbounded_channel::<Vec<f32>>();
    let (spk_tx, _spk_rx) = mpsc::unbounded_channel::<Vec<f32>>();

    let config = VoiceConfig::default();
    let mut pipeline =
        VoicePipeline::new(peer_conn.connection().clone(), config, mic_rx, spk_tx).unwrap();
    pipeline.start().await.unwrap();

    // Drop the mic sender immediately (simulates no audio device)
    drop(mic_tx);

    // Pipeline should handle closed input gracefully
    tokio::time::sleep(Duration::from_millis(500)).await;
    pipeline.stop().await;

    tracing::info!("pipeline handled missing audio input gracefully");

    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}

/// Create a ticket pointing to an unreachable peer for testing error paths.
fn create_unreachable_ticket() -> bisc_protocol::ticket::BiscTicket {
    use bisc_protocol::types::{EndpointAddr, EndpointId};

    let fake_addr = EndpointAddr {
        id: EndpointId([0xAA; 32]),
        addrs: vec!["192.0.2.1:1234".to_string()], // RFC 5737 TEST-NET
        relay_url: None,
    };
    bisc_protocol::ticket::BiscTicket {
        channel_secret: [0xBB; 32],
        bootstrap_addrs: vec![fake_addr],
    }
}
