//! Integration test for media negotiation over real QUIC connections.

use std::time::Duration;

use bisc_media::negotiation::{default_offer, negotiate_as_answerer, negotiate_as_offerer};
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

#[tokio::test]
async fn two_peers_negotiate_successfully() {
    init_test_tracing();

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
