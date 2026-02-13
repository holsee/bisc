# BISC-034: Network Simulation Harness

**Phase**: 1 — Foundation & Channel Infrastructure
**Depends on**: BISC-005

## Description

Build a deterministic in-process simulation harness that allows multi-peer integration tests to run without real sockets, real network, or real hardware. The harness simulates network conditions (latency, packet loss, reordering, disconnection) so the agentic loop can test failure modes, timing-sensitive behaviour, and multi-peer scenarios with fully reproducible results.

This is foundational infrastructure — once complete, all subsequent integration tests (BISC-012, BISC-018, BISC-021, BISC-024, BISC-033) should use the sim harness instead of real iroh connections on localhost.

## Deliverables

- `bisc-net/src/sim/mod.rs` — module root, re-exports
- `bisc-net/src/sim/network.rs`:
  - `SimNetwork` — the simulation controller:
    - `new() -> SimNetwork` — creates a simulated network
    - `create_peer(display_name) -> SimPeer` — creates a simulated peer with an in-process transport
    - `create_channel() -> (SimPeer, SimPeer, BiscTicket)` — shorthand: creates two peers already joined to a channel
    - `set_latency(duration: Duration)` — sets one-way latency for all links
    - `set_latency_between(peer_a, peer_b, duration)` — sets latency for a specific link
    - `set_loss_rate(rate: f64)` — sets packet loss rate (0.0-1.0) for all links
    - `set_loss_rate_between(peer_a, peer_b, rate)` — per-link loss rate
    - `set_jitter(max_jitter: Duration)` — random jitter added to latency
    - `disconnect(peer)` — simulates a peer going offline
    - `reconnect(peer)` — simulates a peer coming back online
    - `advance(duration: Duration)` — advances simulated time (for deterministic timing)
  - Uses `tokio::sync::mpsc` channels internally to shuttle datagrams and stream data between peers, applying latency/loss/jitter before delivery
- `bisc-net/src/sim/peer.rs`:
  - `SimPeer` — wraps a simulated peer endpoint:
    - Implements the same interface as `PeerConnection` (send_datagram, recv_datagram, open_control_stream)
    - Exposes `metrics() -> PeerMetrics` for test assertions
    - Tracks: datagrams sent/received/lost, streams opened, connection state
  - `PeerMetrics` — struct with `AtomicU64` counters for all observable events
- `bisc-net/src/sim/transport.rs`:
  - `SimTransport` — trait-compatible replacement for the real iroh transport
    - Datagrams go through the `SimNetwork` which applies latency, loss, reordering
    - Reliable streams are simulated as `mpsc` channels with ordering guarantees
    - Supports both unreliable (media) and reliable (control, file transfer) semantics
- `bisc-net/src/transport.rs` (or equivalent):
  - Extract a `Transport` trait from `PeerConnection` so that both real iroh connections and `SimTransport` can be used interchangeably
  - `trait Transport: Send + Sync`:
    - `async fn send_datagram(&self, data: Bytes) -> Result<()>`
    - `async fn recv_datagram(&self) -> Result<Bytes>`
    - `async fn open_bi_stream(&self) -> Result<(SendStream, RecvStream)>`
    - `async fn accept_bi_stream(&self) -> Result<(SendStream, RecvStream)>`
    - `fn metrics(&self) -> &TransportMetrics`

## Design Notes

- The `Transport` trait abstraction is key — it allows all higher-level code (MediaTransport, VoicePipeline, VideoPipeline, FileTransfer) to be tested against the simulation without code changes.
- Latency is implemented by delaying delivery via `tokio::time::sleep` (or simulated time if using `tokio::time::pause()`).
- Packet loss is implemented by randomly dropping datagrams based on the configured loss rate using a seeded RNG for reproducibility.
- Reordering is implemented by randomizing delivery order within a jitter window.
- `advance()` uses `tokio::time::advance()` when time is paused, enabling fully deterministic tests.
- The sim harness does NOT simulate iroh's NAT traversal or relay — it assumes all peers can reach each other directly. The value is in testing application-layer behaviour under various network conditions.

## Acceptance Criteria

- [ ] `SimNetwork` can create 2+ peers and they can exchange datagrams — unit test
- [ ] `set_latency()` causes measurable delay in datagram delivery — unit test (measure round-trip time)
- [ ] `set_loss_rate(0.5)` causes approximately 50% of datagrams to be dropped (within tolerance) — unit test (send 1000 packets, verify ~500 received)
- [ ] `set_loss_rate(0.0)` causes zero packet loss — unit test
- [ ] `disconnect()` stops all datagram delivery to/from that peer — unit test
- [ ] `reconnect()` resumes delivery — unit test
- [ ] Reliable streams deliver all data in order regardless of loss/latency settings — unit test
- [ ] `PeerMetrics` accurately tracks packets sent, received, and lost — unit test
- [ ] `Transport` trait is implemented by both `PeerConnection` (real) and `SimTransport` (simulated) — build test
- [ ] Existing `PeerConnection` code compiles against the `Transport` trait without functional changes — build test
- [ ] A seeded RNG produces deterministic loss/jitter patterns — unit test (same seed = same results)
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
