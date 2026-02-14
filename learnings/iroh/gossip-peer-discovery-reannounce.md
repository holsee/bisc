# Gossip Peer Discovery Requires Re-Announce on NeighborUp

**Task**: BISC-004
**Date**: 2026-02-14
**Crate/Area**: bisc-net, iroh-gossip

## Context

When peer A creates a channel and broadcasts PeerAnnounce, then peer B joins later via ticket, B needs to discover A. B broadcasts its own PeerAnnounce which A receives, but A's initial broadcast happened before B subscribed, so B never learns about A.

## Discovery

The iroh-gossip `Event::NeighborUp` event fires when a new peer connects on a gossip topic. By re-broadcasting our PeerAnnounce in response to NeighborUp, we ensure that newly connected peers learn about us immediately, regardless of when they joined.

Without this, the peer that created the channel first would be invisible to later joiners until a heartbeat or other message is sent, and heartbeats don't carry display_name/capabilities.

Additionally, `n0-future` version must match what iroh-gossip uses internally (0.3, not 0.1). Using the wrong version causes `StreamExt::try_next` to silently not work with `GossipReceiver` because the `Stream` trait comes from a different crate version.

## Impact

- All channel implementations must re-announce on NeighborUp for reliable peer discovery
- The `event_loop` function needs access to `display_name` and the gossip `sender` to perform re-announces
- Always ensure `n0-future` workspace version matches iroh's transitive dependency
