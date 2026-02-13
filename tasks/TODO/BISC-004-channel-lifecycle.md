# BISC-004: Channel Lifecycle (Create, Join, Leave)

**Phase**: 1 — Foundation & Channel Infrastructure
**Depends on**: BISC-003

## Description

Implement the full channel lifecycle: creating a channel, generating a ticket, joining via ticket, peer discovery via gossip, and leaving.

## Deliverables

- `bisc-net/src/channel.rs` — `Channel` struct with:
  - `create(endpoint, display_name) -> (Channel, BiscTicket)` — generates secret, derives topic, subscribes to gossip, returns ticket
  - `join(endpoint, ticket, display_name) -> Channel` — parses ticket, derives topic, subscribes with bootstrap peers
  - `leave(&mut self)` — broadcasts `PeerLeave`, unsubscribes from gossip, closes connections
  - `refresh_ticket(&self) -> BiscTicket` — generates new ticket with same secret, this peer's address
  - `peers(&self) -> &HashMap<EndpointId, PeerInfo>` — current peer list
  - Internal gossip event loop that processes `PeerAnnounce`, `PeerLeave`, `Heartbeat` and maintains the peer map
- `bisc-net/src/channel.rs` — `PeerInfo` struct (endpoint_id, display_name, capabilities, last_heartbeat)
- Heartbeat ticker (sends `Heartbeat` every 5 seconds, marks peers as timed out after 15 seconds of silence)

## Acceptance Criteria

- [ ] Create channel produces a valid `BiscTicket` that round-trips — unit test
- [ ] Two peers: A creates channel, B joins via ticket, both see each other in peer list — integration test
- [ ] Three peers: A creates, B joins, C joins — all three see each other — integration test
- [ ] Peer leave: B leaves, A and C see B removed from peer list — integration test
- [ ] Ticket refresh: A leaves, C generates new ticket, D joins via C's ticket — integration test
- [ ] Heartbeat timeout: peer that stops sending heartbeats is removed after timeout — integration test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
