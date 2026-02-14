//! Deterministic in-process network simulation harness for testing.
//!
//! Provides [`SimNetwork`] to create simulated peers that communicate via
//! in-process channels with configurable latency, packet loss, jitter,
//! and disconnection. All tests run without real sockets or hardware.
//!
//! # Example
//!
//! ```ignore
//! let mut sim = SimNetwork::new();
//! let (peer_a, peer_b, _ticket) = sim.create_channel().await;
//! sim.set_latency(Duration::from_millis(50)).await;
//! sim.set_loss_rate(0.05).await;
//!
//! peer_a.send_datagram(Bytes::from("hello")).await?;
//! let data = peer_b.recv_datagram().await?;
//! ```

mod network;
mod peer;
mod transport;

pub use network::{SimNetwork, SimPeerId};
pub use peer::{PeerMetrics, SimPeer};
pub use transport::SimTransport;

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use bytes::Bytes;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::transport::Transport;

    use super::*;

    fn init_test_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "debug".into()),
            )
            .with_test_writer()
            .try_init();
    }

    // --- SimNetwork can create 2+ peers and exchange datagrams ---

    #[tokio::test]
    async fn two_peers_exchange_datagrams() {
        init_test_tracing();

        let mut sim = SimNetwork::new();
        let (peer_a, peer_b, _ticket) = sim.create_channel().await;

        peer_a.send_datagram(Bytes::from("hello")).await.unwrap();
        let data = peer_b.recv_datagram().await.unwrap();
        assert_eq!(data, Bytes::from("hello"));

        peer_b.send_datagram(Bytes::from("world")).await.unwrap();
        let data = peer_a.recv_datagram().await.unwrap();
        assert_eq!(data, Bytes::from("world"));
    }

    #[tokio::test]
    async fn three_peers_exchange_datagrams() {
        init_test_tracing();

        let mut sim = SimNetwork::new();
        let a = sim.create_peer("alice");
        let b = sim.create_peer("bob");
        let c = sim.create_peer("carol");

        let (peer_ab, peer_ba) = sim.connect(a, b).await;
        let (peer_ac, peer_ca) = sim.connect(a, c).await;

        // A → B
        peer_ab.send_datagram(Bytes::from("a-to-b")).await.unwrap();
        let data = peer_ba.recv_datagram().await.unwrap();
        assert_eq!(data, Bytes::from("a-to-b"));

        // A → C
        peer_ac.send_datagram(Bytes::from("a-to-c")).await.unwrap();
        let data = peer_ca.recv_datagram().await.unwrap();
        assert_eq!(data, Bytes::from("a-to-c"));

        // C → A
        peer_ca.send_datagram(Bytes::from("c-to-a")).await.unwrap();
        let data = peer_ac.recv_datagram().await.unwrap();
        assert_eq!(data, Bytes::from("c-to-a"));
    }

    // --- set_latency() causes measurable delay ---

    #[tokio::test]
    async fn latency_causes_measurable_delay() {
        init_test_tracing();
        tokio::time::pause();

        let mut sim = SimNetwork::new();
        let (peer_a, peer_b, _) = sim.create_channel().await;
        sim.set_latency(Duration::from_millis(100)).await;

        let start = tokio::time::Instant::now();
        peer_a.send_datagram(Bytes::from("delayed")).await.unwrap();

        let data = peer_b.recv_datagram().await.unwrap();
        let elapsed = start.elapsed();

        assert_eq!(data, Bytes::from("delayed"));
        assert!(
            elapsed >= Duration::from_millis(100),
            "expected >= 100ms delay, got {elapsed:?}"
        );
    }

    // --- set_loss_rate(0.5) drops ~50% ---

    #[tokio::test]
    async fn loss_rate_drops_approximately_half() {
        init_test_tracing();

        let mut sim = SimNetwork::new();
        let (peer_a, peer_b, _) = sim.create_channel().await;
        sim.set_loss_rate(0.5).await;

        let total = 1000u64;
        for i in 0..total {
            peer_a
                .send_datagram(Bytes::from(format!("pkt-{i}")))
                .await
                .unwrap();
        }

        let mut received = 0u64;
        loop {
            match tokio::time::timeout(Duration::from_millis(50), peer_b.recv_datagram()).await {
                Ok(Ok(_)) => received += 1,
                _ => break,
            }
        }

        let dropped = peer_a.datagrams_dropped();
        tracing::info!(total, received, dropped, "loss rate test results");

        assert!(
            received > 300 && received < 700,
            "received {received}, expected ~500 (tolerance 300-700)"
        );
        assert_eq!(
            received + dropped,
            total,
            "sent({total}) should equal received({received}) + dropped({dropped})"
        );
    }

    // --- set_loss_rate(0.0) causes zero loss ---

    #[tokio::test]
    async fn zero_loss_rate_delivers_all() {
        init_test_tracing();

        let mut sim = SimNetwork::new();
        let (peer_a, peer_b, _) = sim.create_channel().await;
        sim.set_loss_rate(0.0).await;

        let total = 100;
        for i in 0..total {
            peer_a
                .send_datagram(Bytes::from(format!("pkt-{i}")))
                .await
                .unwrap();
        }

        for _ in 0..total {
            let _data = peer_b.recv_datagram().await.unwrap();
        }

        assert_eq!(peer_a.datagrams_dropped(), 0);
        assert_eq!(peer_a.datagrams_sent(), total);
        assert_eq!(peer_b.datagrams_received(), total as u64);
    }

    // --- disconnect() stops delivery ---

    #[tokio::test]
    async fn disconnect_stops_delivery() {
        init_test_tracing();

        let mut sim = SimNetwork::new();
        let a_id = sim.create_peer("alice");
        let b_id = sim.create_peer("bob");
        let (peer_a, peer_b) = sim.connect(a_id, b_id).await;

        // Before disconnect
        peer_a.send_datagram(Bytes::from("before")).await.unwrap();
        let data = peer_b.recv_datagram().await.unwrap();
        assert_eq!(data, Bytes::from("before"));

        // Disconnect
        sim.disconnect(a_id).await;

        for _ in 0..10 {
            peer_a.send_datagram(Bytes::from("dropped")).await.unwrap();
        }

        let result = tokio::time::timeout(Duration::from_millis(50), peer_b.recv_datagram()).await;
        assert!(
            result.is_err(),
            "should not receive anything while disconnected"
        );
        assert_eq!(peer_a.datagrams_dropped(), 10);
    }

    // --- reconnect() resumes delivery ---

    #[tokio::test]
    async fn reconnect_resumes_delivery() {
        init_test_tracing();

        let mut sim = SimNetwork::new();
        let a_id = sim.create_peer("alice");
        let b_id = sim.create_peer("bob");
        let (peer_a, peer_b) = sim.connect(a_id, b_id).await;

        sim.disconnect(a_id).await;

        peer_a.send_datagram(Bytes::from("dropped")).await.unwrap();
        let result = tokio::time::timeout(Duration::from_millis(50), peer_b.recv_datagram()).await;
        assert!(result.is_err());

        sim.reconnect(a_id).await;

        peer_a.send_datagram(Bytes::from("after")).await.unwrap();
        let data = peer_b.recv_datagram().await.unwrap();
        assert_eq!(data, Bytes::from("after"));
    }

    // --- Reliable streams deliver in order regardless of loss/latency ---

    #[tokio::test]
    async fn reliable_streams_deliver_in_order() {
        init_test_tracing();

        let mut sim = SimNetwork::new();
        let (peer_a, peer_b, _) = sim.create_channel().await;

        // Loss/latency should NOT affect reliable streams
        sim.set_loss_rate(0.5).await;
        sim.set_latency(Duration::from_millis(10)).await;

        let (mut a_send, mut a_recv) = peer_a.open_bi_stream().await.unwrap();
        let (mut b_send, mut b_recv) = peer_b.accept_bi_stream().await.unwrap();

        // A → B
        let message = b"hello from A to B stream";
        a_send.write_all(message).await.unwrap();
        a_send.shutdown().await.unwrap();

        let mut buf = vec![0u8; message.len()];
        b_recv.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, message);

        // B → A
        let reply = b"reply from B to A stream";
        b_send.write_all(reply).await.unwrap();
        b_send.shutdown().await.unwrap();

        let mut buf = vec![0u8; reply.len()];
        a_recv.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, reply);
    }

    // --- PeerMetrics accurately tracks ---

    #[tokio::test]
    async fn peer_metrics_track_sent_received_lost() {
        init_test_tracing();

        let mut sim = SimNetwork::new();
        let (peer_a, peer_b, _) = sim.create_channel().await;

        for i in 0..10u64 {
            peer_a
                .send_datagram(Bytes::from(format!("pkt-{i}")))
                .await
                .unwrap();
        }

        for _ in 0..10 {
            let _ = peer_b.recv_datagram().await.unwrap();
        }

        assert_eq!(peer_a.datagrams_sent(), 10);
        assert_eq!(peer_a.datagrams_dropped(), 0);
        assert_eq!(peer_b.datagrams_received(), 10);

        assert!(
            peer_a.metrics().bytes_sent.load(Ordering::Relaxed) > 0,
            "bytes_sent should be non-zero"
        );
    }

    // --- Transport trait compile tests ---

    #[tokio::test]
    async fn sim_transport_implements_transport_trait() {
        init_test_tracing();

        let mut sim = SimNetwork::new();
        let (peer_a, _peer_b, _) = sim.create_channel().await;

        let transport = peer_a.transport();
        transport
            .send_datagram(Bytes::from("via trait"))
            .await
            .unwrap();
        let metrics = transport.transport_metrics();
        assert_eq!(metrics.datagrams_sent.load(Ordering::Relaxed), 1);
    }

    fn _assert_peer_connection_is_transport() {
        fn _require_transport<T: Transport>() {}
        _require_transport::<crate::PeerConnection>();
    }

    fn _assert_sim_transport_is_transport() {
        fn _require_transport<T: Transport>() {}
        _require_transport::<SimTransport>();
    }

    // --- Seeded RNG produces deterministic results ---

    #[tokio::test]
    async fn seeded_rng_is_deterministic() {
        init_test_tracing();

        let seed = 12345u64;
        let total = 200;

        // Run 1
        let mut sim1 = SimNetwork::with_seed(seed);
        let (peer_a1, peer_b1, _) = sim1.create_channel().await;
        sim1.set_loss_rate(0.3).await;

        for i in 0..total {
            peer_a1
                .send_datagram(Bytes::from(format!("pkt-{i}")))
                .await
                .unwrap();
        }

        let mut received1 = 0u64;
        loop {
            match tokio::time::timeout(Duration::from_millis(50), peer_b1.recv_datagram()).await {
                Ok(Ok(_)) => received1 += 1,
                _ => break,
            }
        }

        // Run 2 with same seed
        let mut sim2 = SimNetwork::with_seed(seed);
        let (peer_a2, peer_b2, _) = sim2.create_channel().await;
        sim2.set_loss_rate(0.3).await;

        for i in 0..total {
            peer_a2
                .send_datagram(Bytes::from(format!("pkt-{i}")))
                .await
                .unwrap();
        }

        let mut received2 = 0u64;
        loop {
            match tokio::time::timeout(Duration::from_millis(50), peer_b2.recv_datagram()).await {
                Ok(Ok(_)) => received2 += 1,
                _ => break,
            }
        }

        assert_eq!(
            received1, received2,
            "same seed should produce same loss pattern: run1={received1}, run2={received2}"
        );
    }
}
