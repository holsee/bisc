# BISC-049: Fix Peer-to-Peer QUIC Connection Establishment

**Phase**: 9 — Validation & Fixes
**Depends on**: BISC-038, BISC-041, BISC-044

## Problem Statement

Direct QUIC connections between peers are never established. Gossip works (peers discover each other via `PeerAnnounce`), but the data-plane connections for voice, video, screen share, and file transfer all silently fail. File downloads report "no peers available for download"; voice/video/screen share pipelines never start.

**Root cause**: Two wiring bugs in `bisc-app/src/main.rs`:

1. `ensure_net_initialized()` calls `GossipHandle::new()` which only registers the gossip ALPN with the Router. `MediaProtocol` is never registered, so incoming QUIC connections on `MEDIA_ALPN` (`b"bisc/media/0"`) are rejected. The correct call is `GossipHandle::with_protocols()`.

2. `Channel::create()` and `Channel::join()` are both called with `None` for the `incoming_rx` parameter. The Channel event loop (channel.rs line 399-426) pends forever on the incoming connection branch and never processes accepted connections.

**Discovery**: Comparison with [n0-computer/sendme](https://github.com/n0-computer/sendme) confirmed that iroh-based apps must register protocol handlers with `Router::builder().accept(ALPN, handler)` for incoming connections to be dispatched.

## Deliverables

### 1. Register MediaProtocol with Router

- In `ensure_net_initialized()`: call `MediaProtocol::new()` to get `(protocol, incoming_rx)`, then `GossipHandle::with_protocols(ep.endpoint(), protocol)` to register it with the Router.
- Add `MediaProtocol` to the `bisc_net` import.

### 2. Store and pass `incoming_rx` to Channel

- Add `incoming_rx: Option<mpsc::UnboundedReceiver<iroh::endpoint::Connection>>` field to the `Net` struct.
- Store the `incoming_rx` from `MediaProtocol::new()` in `Net.incoming_rx`.
- In `CreateChannel`: `.take()` the `incoming_rx` from `Net` and pass it to `Channel::create()`.
- In `JoinChannel`: `.take()` the `incoming_rx` from `Net` and pass it to `Channel::join()`.

### 3. Handle rejoin after leave

- The `incoming_rx` is consumed by the Channel when created/joined. On `LeaveChannel`, tear down `Net` entirely (`self.net.lock().unwrap().take()`) so the next create/join gets a fresh endpoint, router, and `MediaProtocol`.
- Endpoint identity is ephemeral (no stored keys), so teardown is safe.

## Scope

All changes are in a single file: `bisc-app/src/main.rs`. No changes to `bisc-net` or other crates — `MediaProtocol` and `GossipHandle::with_protocols()` already exist and are exported.

## Acceptance Criteria

- [x] `MediaProtocol` is registered with the Router via `GossipHandle::with_protocols()`
- [x] `incoming_rx` from `MediaProtocol::new()` is passed to `Channel::create()` and `Channel::join()`
- [x] `PeerConnected` events fire when two peers join the same channel (visible in logs at `info` level)
- [x] `LeaveChannel` tears down `Net` so rejoining works cleanly
- [x] Two instances on localhost can create/join a channel and establish a direct QUIC connection
- [x] File sharing works: share a file on instance A, download on instance B — no "no peers available" error
- [x] `cargo build --workspace` compiles
- [x] `cargo fmt --all --check` passes
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo test --workspace` — all existing tests pass
