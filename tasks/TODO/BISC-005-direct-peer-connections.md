# BISC-005: Direct Peer Connections

**Phase**: 1 — Foundation & Channel Infrastructure
**Depends on**: BISC-004

## Description

When gossip discovers a new peer, establish a direct QUIC connection for media and file transfer.

## Deliverables

- `bisc-net/src/connection.rs` — `PeerConnection` struct wrapping an `iroh::endpoint::Connection`:
  - `connect(endpoint, peer_addr, alpn) -> PeerConnection` — initiates connection
  - `accept(incoming) -> PeerConnection` — accepts incoming connection
  - `open_control_stream() -> (SendStream, RecvStream)` — opens a reliable bidirectional stream for media control
  - `send_datagram(data: Bytes)` — sends unreliable datagram
  - `recv_datagram() -> Bytes` — receives datagram
  - `close()` — graceful shutdown
- Integration into `Channel`: when `PeerAnnounce` is received, automatically establish a direct `PeerConnection` to the new peer. When `PeerLeave` is received, close the connection.
- Connection state tracking: `Connecting`, `Connected`, `Disconnected`

## Acceptance Criteria

- [ ] Two peers in a channel automatically establish a direct QUIC connection after gossip discovery — integration test
- [ ] Datagrams can be sent and received between connected peers — integration test
- [ ] A reliable stream can be opened and data exchanged — integration test
- [ ] When a peer leaves the channel, the connection is closed on both sides — integration test
- [ ] Connection handles the case where the remote peer is unreachable (relay fallback or error) — integration test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
