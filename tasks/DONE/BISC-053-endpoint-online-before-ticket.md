# BISC-053: Wait for Endpoint Online Before Generating Ticket

**Phase**: 10 — Improvements
**Depends on**: BISC-049

## Problem Statement

When creating a channel, bisc generates the invite ticket immediately after binding the endpoint. The ticket contains the endpoint's addressing info (node ID, direct addresses, relay URLs). If the endpoint hasn't yet connected to the relay server or discovered its public addresses, the ticket may contain incomplete or stale addressing info, making it unreachable for the joining peer.

sendme addresses this by calling `endpoint.online().await` (with a timeout) before generating the ticket, ensuring relay connectivity is established and addresses are populated.

## Deliverables

### 1. Wait for endpoint to be online

- After `BiscEndpoint::new()` and before generating the ticket, await `endpoint.online()` with a reasonable timeout (e.g. 10 seconds)
- If relay mode is disabled (testing), skip the wait
- Log the endpoint's addresses after coming online for diagnostics

### 2. Refresh ticket addresses

- Ensure `BiscTicket` includes the endpoint's relay URL and any discovered direct addresses
- Call `endpoint.addr()` after online to get the most current addressing info

## Acceptance Criteria

- [x] `endpoint.online()` is awaited before ticket generation in `Channel::create()`
- [x] Timeout prevents indefinite hang if relay is unreachable
- [x] Ticket contains relay URLs when available
- [x] Channel creation still works when relay is unavailable (graceful degradation)
- [x] `cargo build --workspace` compiles
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo test --workspace` — all tests pass
