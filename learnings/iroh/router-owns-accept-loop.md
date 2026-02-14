# iroh Router Owns the Accept Loop

**Task**: BISC-005
**Date**: 2026-02-14
**Crate/Area**: bisc-net / iroh

## Context

Needed to accept incoming QUIC connections for direct peer media connections alongside the gossip protocol. Initially tried calling `endpoint.accept()` directly in a background loop.

## Discovery

When an iroh `Router` is active (which is required for gossip), it owns the accept loop on the endpoint. Calling `endpoint.accept()` directly either races with the Router or never receives connections.

To accept connections with a custom ALPN alongside gossip, you must implement `iroh::protocol::ProtocolHandler` and register it with the Router via `Router::builder().accept(alpn, handler)`.

The `ProtocolHandler` trait requires:
```rust
async fn accept(&self, connection: Connection) -> Result<(), AcceptError>
```

A practical pattern is to forward accepted connections through an `mpsc::UnboundedSender<Connection>`, then process them in the main event loop.

## Impact

- Cannot use standalone accept loops when a Router is running
- All custom protocols must be registered as `ProtocolHandler` implementations
- The `GossipHandle::with_protocols()` constructor was added to register both gossip and media protocols on the same Router
- Channel tests that don't need media connections pass `None` for the incoming receiver
