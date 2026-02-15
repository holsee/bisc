# Learnings Index

Discoveries, workarounds, and architectural insights captured during development.

---

<!-- Entries grouped by category. Add new learnings here as they are discovered. -->

## iroh

- [Gossip Peer Discovery Requires Re-Announce on NeighborUp](iroh/gossip-peer-discovery-reannounce.md) — Peers must re-broadcast PeerAnnounce on NeighborUp events for reliable discovery; also n0-future version must match iroh's transitive dep
- [iroh Router Owns the Accept Loop](iroh/router-owns-accept-loop.md) — Cannot call endpoint.accept() when a Router is active; must implement ProtocolHandler and register with the Router
- [Spawn Connection Attempts to Avoid Blocking the Event Loop](iroh/spawn-connection-attempts.md) — QUIC connection setup can block a tokio::select! event loop; spawn as background tasks instead
- [MemoryLookup for Relay-Disabled Endpoints](iroh/memorylookup-for-relay-disabled.md) — Use MemoryLookup to register peer addresses when RelayMode::Disabled; without it, iroh cannot resolve EndpointIds to socket addresses
- [Plumbing Exists Does Not Mean Plumbing Is Connected](iroh/plumbing-not-connected.md) — Test helpers may correctly wire subsystems while production code does not; integration tests should test the real initialization path
- [iroh-blobs as Alternative to Custom File Transfer](iroh/iroh-blobs-for-file-transfer.md) — iroh-blobs provides BLAKE3 verified streaming, resumable downloads, and BlobsProtocol out of the box; replaced ~1300 lines of custom code
- [endpoint.online() Before Ticket Generation](iroh/endpoint-online-before-ticket.md) — Call endpoint.online() with timeout before generating tickets to ensure relay URLs are included
- [Gossip Leave Detection Is Too Slow for Test Timeouts](iroh/gossip-leave-detection-timing.md) — Don't rely on wait_for_peer_left in time-sensitive tests; use sleep after endpoint close instead
- [FileAvailable Gossip Pattern for Peer-Assisted Downloads](iroh/file-availability-gossip-pattern.md) — 5-step pattern for adding new gossip message types; iroh-blobs serves automatically, only discovery was missing

## Media

- [Opus Codec Startup Distortion](media/opus-codec-startup-distortion.md) — First few Opus frames have high distortion; VoIP mode filters pure tones; audiopus_sys static feature builds from source
- [nokhwa Requires libclang and V4L2 Dev Headers](media/nokhwa-system-deps.md) — nokhwa input-native uses bindgen needing libclang-dev; made optional behind `video` feature flag
- [scap Requires D-Bus and PipeWire on Linux](media/scap-system-deps.md) — scap depends on libdbus-sys needing libdbus-1-dev; made optional behind `screen-capture` feature flag

## Iced

- [Verify Application Actually Launches](iced/verify-app-launches.md) — Acceptance criteria saying "app launches" must be verified by running the binary, not just unit tests

## Platform

- [cpal Requires ALSA Dev Headers on Linux](platform/cpal-alsa-dev-headers.md) — cpal depends on alsa-sys which needs libasound2-dev; made optional behind `audio` feature flag
- [Windows wgpu Backend Selection](platform/windows-wgpu-backend.md) — Force DX12 backend on Windows to avoid Vulkan driver crashes; set WGPU_BACKEND before Iced initialization
- [scap 0.1.0-beta.1 Linux API Bugs](platform/scap-linux-api-bugs.md) — Vendored scap to fix Linux Frame enum paths, display_time types, and nokhwa Camera non-Send issue
