# iroh MemoryLookup for Relay-Disabled Endpoints

**Task**: BISC-035
**Date**: 2026-02-14
**Crate/Area**: iroh, bisc-net

## Context

Tests using `BiscEndpoint::for_testing()` with `RelayMode::Disabled` caused all peer discovery to hang indefinitely. Gossip nodes broadcast heartbeats but peers never found each other.

## Discovery

When `RelayMode::Disabled` is set, iroh endpoints have no way to resolve `EndpointId`s to socket addresses. The gossip layer's `subscribe_and_join()` only takes `EndpointId`s, not addresses. Without relay servers performing address exchange, iroh needs an explicit out-of-band mechanism.

The solution is `iroh::address_lookup::memory::MemoryLookup`:

```rust
use iroh::address_lookup::memory::MemoryLookup;

let memory_lookup = MemoryLookup::new();
memory_lookup.add_endpoint_info(iroh::EndpointAddr {
    id: peer_endpoint_id,
    addrs: vec![TransportAddr::Ip(socket_addr)].into_iter().collect(),
});
endpoint.address_lookup().add(memory_lookup);
```

This registers peer addresses directly with iroh's address lookup system, bypassing the need for relay-based discovery.

## Impact

- **Tests**: Peer discovery went from hanging indefinitely to completing in <300ms on localhost
- **Production**: `Channel::join` now registers bootstrap peer addresses from the ticket via `MemoryLookup`, which also benefits self-hosted deployments without relay servers
- **Key insight**: `datagrams_sent` in `SimNetwork` only counts packets that pass through routing (not dropped ones). Dropped packets only increment `datagrams_dropped`. So `sent + dropped = total_attempted`.
