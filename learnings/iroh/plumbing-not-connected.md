# Plumbing Exists Does Not Mean Plumbing Is Connected

**Task**: BISC-049
**Date**: 2026-02-15
**Crate/Area**: iroh, bisc-net, bisc-app

## Context

`MediaProtocol`, `GossipHandle::with_protocols()`, and `Channel::create(incoming_rx)` were all correctly implemented in `bisc-net`. The app-level wiring in `main.rs` used `GossipHandle::new()` (no MediaProtocol registered) and passed `None` for `incoming_rx`.

## Discovery

Unit tests passed because they tested components in isolation. Integration tests passed because they used `setup_peer_with_media()` which correctly wires everything. The production code path was never exercised by automated tests.

The bug was invisible until manual testing: peers could discover each other via gossip but could never establish direct QUIC connections because:
1. `MediaProtocol` was not registered with the Router, so incoming connections on the `bisc/media/0` ALPN were rejected
2. `incoming_rx` was `None`, so accepted connections were silently dropped

## Impact

When test helpers correctly wire subsystems but the production code does not, tests provide false confidence. Integration tests should share the same initialization path as the real application, or explicitly test the real path. Consider extracting the production initialization into a shared function used by both `main.rs` and integration test setup.
