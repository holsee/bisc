//! Integration test for media negotiation over real QUIC connections.

use std::time::Duration;

use bisc_media::negotiation::{default_offer, negotiate_as_answerer, negotiate_as_offerer};
use bisc_net::testing::{
    init_test_tracing, setup_peer_with_media, wait_for_connection, wait_for_peers, TestTimer,
    CONNECTION_TIMEOUT_SECS, PEER_DISCOVERY_TIMEOUT_SECS,
};

#[tokio::test]
async fn two_peers_negotiate_successfully() {
    init_test_tracing();
    let mut timer = TestTimer::new("two_peers_negotiate_successfully");

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

    let conn_a = channel_a
        .connection(&b_id)
        .await
        .expect("A should have connection to B");
    let conn_b = channel_b
        .connection(&a_id)
        .await
        .expect("B should have connection to A");

    // Determine who is offerer (higher ID)
    let (offerer_conn, answerer_conn) = if a_id.0 > b_id.0 {
        (conn_a, conn_b)
    } else {
        (conn_b, conn_a)
    };

    let offer = default_offer();

    // Run negotiation in parallel
    let offerer_handle = {
        let offer = offer.clone();
        let conn = offerer_conn.connection().clone();
        tokio::spawn(async move {
            let (mut send, mut recv) = conn.open_bi().await.unwrap();
            negotiate_as_offerer(&mut send, &mut recv, &offer).await
        })
    };

    let answerer_handle = {
        let offer = offer.clone();
        let conn = answerer_conn.connection().clone();
        tokio::spawn(async move {
            let (mut send, mut recv) = conn.accept_bi().await.unwrap();
            negotiate_as_answerer(&mut send, &mut recv, &offer).await
        })
    };

    let offerer_answer = tokio::time::timeout(Duration::from_secs(10), offerer_handle)
        .await
        .expect("offerer timed out")
        .expect("offerer task panicked")
        .expect("offerer negotiation failed");

    let answerer_answer = tokio::time::timeout(Duration::from_secs(10), answerer_handle)
        .await
        .expect("answerer timed out")
        .expect("answerer task panicked")
        .expect("answerer negotiation failed");

    timer.phase("negotiation");

    // Both should agree
    assert_eq!(offerer_answer, answerer_answer);
    assert_eq!(offerer_answer.selected_audio_codec.name, "opus");
    assert_eq!(offerer_answer.selected_video_codec.name, "h264");
    assert_eq!(offerer_answer.negotiated_resolution, (1920, 1080));
    assert_eq!(offerer_answer.negotiated_framerate, 30);

    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}
