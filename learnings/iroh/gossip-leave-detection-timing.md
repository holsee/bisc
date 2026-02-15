# Gossip Leave Detection Is Too Slow for Test Timeouts

**Task**: BISC-061
**Date**: 2026-02-15
**Crate/Area**: bisc-net / iroh-gossip

## Context

The `peer_assisted_download` integration test needed to verify that after peer A leaves and goes offline, peer C can still download a file from peer B (peer-assisted download). Initially, the test used `wait_for_peer_left` with a 5-second timeout to detect A's departure.

## Discovery

Gossip-based leave detection in iroh is unreliable within short timeouts in localhost tests. The `NeighborDown` event that triggers `PeerLeft` depends on the gossip protocol's internal heartbeat and failure detection, which can take longer than 5 seconds even on localhost. The test consistently timed out waiting for the `PeerLeft` event.

However, for the test's purpose we don't actually need to detect the leave event — we only need A to be offline so C's download attempt goes to B instead. A simple `tokio::time::sleep(Duration::from_millis(500))` after calling `channel_a.leave()` and `ep_a.close()` is sufficient: A's endpoint is closed, so any connection attempt to A will fail immediately, and the 500ms gives gossip time to propagate enough for the test to proceed.

## Impact

When writing integration tests that involve peer departure:
- Don't rely on `wait_for_peer_left` for time-sensitive tests — gossip leave detection is inherently slow
- If the test only needs the peer to be *offline* (not to detect the leave event), use a simple sleep after closing the endpoint
- Reserve `wait_for_peer_left` for tests that specifically verify leave detection behavior, with generous timeouts (10s+)
