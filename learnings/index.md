# Learnings Index

Discoveries, workarounds, and architectural insights captured during development.

---

<!-- Entries grouped by category. Add new learnings here as they are discovered. -->

## iroh

- [Gossip Peer Discovery Requires Re-Announce on NeighborUp](iroh/gossip-peer-discovery-reannounce.md) — Peers must re-broadcast PeerAnnounce on NeighborUp events for reliable discovery; also n0-future version must match iroh's transitive dep
- [iroh Router Owns the Accept Loop](iroh/router-owns-accept-loop.md) — Cannot call endpoint.accept() when a Router is active; must implement ProtocolHandler and register with the Router
- [Spawn Connection Attempts to Avoid Blocking the Event Loop](iroh/spawn-connection-attempts.md) — QUIC connection setup can block a tokio::select! event loop; spawn as background tasks instead

## Platform

- [cpal Requires ALSA Dev Headers on Linux](platform/cpal-alsa-dev-headers.md) — cpal depends on alsa-sys which needs libasound2-dev; made optional behind `audio` feature flag
