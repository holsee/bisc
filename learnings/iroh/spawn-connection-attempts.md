# Spawn Connection Attempts to Avoid Blocking the Event Loop

**Task**: BISC-005
**Date**: 2026-02-14
**Crate/Area**: bisc-net / channel

## Context

After integrating `PeerConnection::connect()` into the channel event loop, the `heartbeat_timeout_removes_peer` test started failing. The test timed out after 25 seconds waiting for the peer timeout (15s) to fire.

## Discovery

`PeerConnection::connect()` calls `endpoint.connect(remote_id, alpn).await`, which can take a long time if the remote peer is unreachable (QUIC retries, relay attempts). When this is awaited inline inside `handle_gossip_event()`, which is itself awaited inline in the `tokio::select!` event loop, it blocks all other select branches — including the timeout check interval that removes stale peers.

The fix is to `tokio::spawn()` the connection attempt as a background task instead of awaiting it inline. This way the event loop continues processing heartbeats, timeouts, and other events while the connection is being established.

## Impact

- Any long-running async operation in a `tokio::select!` event loop must be spawned, not awaited inline
- This applies broadly: DNS resolution, TLS handshakes, and QUIC connection setup can all take seconds
- The pattern is: clone the needed handles (`Arc`, `Sender`, `Endpoint`), then `tokio::spawn(async move { ... })`
