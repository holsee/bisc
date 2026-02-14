//! Integration tests for BiscEndpoint and GossipHandle.

use bisc_net::{BiscEndpoint, GossipEvent, GossipHandle};
use bisc_protocol::channel::{ChannelMessage, MediaCapabilities};
use bisc_protocol::types::EndpointId;

fn init_test_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".into()),
        )
        .with_test_writer()
        .try_init();
}

#[tokio::test]
async fn endpoint_creates_and_has_id() {
    init_test_tracing();

    let ep = BiscEndpoint::new().await.expect("endpoint creation failed");
    let id = ep.id();
    tracing::info!(endpoint_id = %id, "created endpoint");

    // EndpointId should be non-zero (a valid public key)
    assert_ne!(
        *id.as_bytes(),
        [0u8; 32],
        "endpoint id should not be all zeros"
    );

    ep.close().await;
}

#[tokio::test]
async fn endpoint_shuts_down_cleanly() {
    init_test_tracing();

    let ep = BiscEndpoint::new().await.expect("endpoint creation failed");
    tracing::info!(endpoint_id = %ep.id(), "created endpoint, now closing");
    ep.close().await;
    // If we get here without panic, shutdown was clean
}

#[tokio::test]
async fn gossip_exchange_peer_announce() {
    init_test_tracing();

    // Create two endpoints
    let ep_a = BiscEndpoint::new()
        .await
        .expect("endpoint A creation failed");
    let ep_b = BiscEndpoint::new()
        .await
        .expect("endpoint B creation failed");

    tracing::info!(a = %ep_a.id(), b = %ep_b.id(), "created two endpoints");

    // Set up gossip on both
    let (gossip_a, _router_a) = GossipHandle::new(ep_a.endpoint());
    let (gossip_b, _router_b) = GossipHandle::new(ep_b.endpoint());

    // Derive a shared topic
    let topic = bisc_protocol::ticket::topic_from_secret(&[0x42; 32]);
    let iroh_topic = iroh_gossip::proto::TopicId::from_bytes(topic.0);

    // Peer A subscribes first (no bootstrap)
    let mut sub_a = gossip_a.subscribe(iroh_topic, vec![]).await.unwrap();

    // We need peer A's address for B to bootstrap
    // Give the endpoint a moment to discover its address
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Peer B subscribes, bootstrapping from A
    let sub_b = gossip_b
        .subscribe_and_join(iroh_topic, vec![ep_a.id()])
        .await
        .unwrap();

    // Wait for neighbors to discover each other
    // Peer A should see NeighborUp for B
    let mut a_saw_neighbor = false;
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while let Some(Ok(event)) = sub_a.recv().await {
            match event {
                GossipEvent::NeighborUp(peer_id) => {
                    tracing::info!(peer = %peer_id, "A saw neighbor up");
                    a_saw_neighbor = true;
                    break;
                }
                other => tracing::debug!(?other, "A received other event"),
            }
        }
    });

    timeout
        .await
        .expect("timed out waiting for A to see neighbor");
    assert!(a_saw_neighbor, "A should have seen B as a neighbor");

    // Now B broadcasts a PeerAnnounce message
    let announce = ChannelMessage::PeerAnnounce {
        endpoint_id: EndpointId(*ep_b.id().as_bytes()),
        display_name: "peer_b".to_string(),
        capabilities: MediaCapabilities {
            audio: true,
            video: false,
            screen_share: false,
        },
    };
    sub_b.broadcast(&announce).await.unwrap();
    tracing::info!("B broadcast PeerAnnounce");

    // A should receive the message
    let mut a_received = false;
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(Ok(event)) = sub_a.recv().await {
            match event {
                GossipEvent::Message { message, .. } => {
                    tracing::info!(?message, "A received message");
                    if let ChannelMessage::PeerAnnounce { display_name, .. } = &message {
                        if display_name == "peer_b" {
                            a_received = true;
                            break;
                        }
                    }
                }
                other => tracing::debug!(?other, "A received other event"),
            }
        }
    });

    timeout
        .await
        .expect("timed out waiting for A to receive message");
    assert!(a_received, "A should have received B's PeerAnnounce");

    // Clean shutdown
    ep_a.close().await;
    ep_b.close().await;
}
