# BISC-003: iroh Endpoint & Gossip Setup (`bisc-net`)

**Phase**: 1 — Foundation & Channel Infrastructure
**Depends on**: BISC-001, BISC-002

## Description

Initialize iroh networking with endpoint creation, gossip protocol, and the ability to create/subscribe to topics.

## Deliverables

- `bisc-net/src/endpoint.rs` — `BiscEndpoint` wrapper that creates an `iroh::Endpoint` with ALPN protocols for media (`bisc/media/0`) and files (`bisc/files/0`), configures relay servers
- `bisc-net/src/gossip.rs` — `GossipHandle` wrapping `iroh_gossip::Gossip`, methods to subscribe/unsubscribe to topics, broadcast `ChannelMessage`, receive `ChannelMessage` as an async stream
- `bisc-net/src/lib.rs` — re-exports, `BiscNet` struct combining endpoint + gossip
- Dependencies: `iroh`, `iroh-gossip`, `tokio`, `bisc-protocol`

## Acceptance Criteria

- [x] `BiscEndpoint` creates a working iroh endpoint — integration test that creates an endpoint and reads its `NodeId`
- [x] Gossip subscribe/unsubscribe works — integration test with two endpoints on localhost that exchange a `PeerAnnounce` message via gossip on a shared topic
- [x] `ChannelMessage` broadcast and receive works end-to-end — integration test
- [x] Endpoint shuts down cleanly without panics
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
