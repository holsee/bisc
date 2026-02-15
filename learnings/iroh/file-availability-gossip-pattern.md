# FileAvailable Gossip Pattern for Peer-Assisted Downloads

**Task**: BISC-061
**Date**: 2026-02-15
**Crate/Area**: bisc-protocol, bisc-net, bisc-app

## Context

Implementing peer-assisted file downloads: after peer B downloads a file from peer A, peer B should announce that it also has the file so that peer C can download from either A or B.

## Discovery

The pattern for extending the gossip protocol with new message types is straightforward:

1. Add the variant to `ChannelMessage` in `bisc-protocol/src/channel.rs` (serde-serializable)
2. Add a corresponding `ChannelEvent` variant in `bisc-net/src/channel.rs`
3. Handle the `ChannelMessage` in `handle_gossip_event` to emit the `ChannelEvent`
4. Map `ChannelEvent` to `AppMessage` in `channel_event_to_message` in `bisc-app/src/main.rs`
5. Handle the `AppMessage` in `app.rs` to update UI state

The broadcast happens in the `AppMessage` pre-processing phase in `main.rs` (before calling `app.update()`), using `gossip.broadcast()` to send the `ChannelMessage` to all peers.

Key insight: iroh-blobs automatically serves blobs via the `BlobsProtocol` router handler — any peer that has downloaded a blob can serve it without additional code. The only missing piece was *discovery*: peers need to know who has the file. The `FileAvailable` gossip message solves this by letting peers announce their blob availability.

## Impact

- Adding new gossip message types follows a well-defined 5-step pattern across 4 crates
- iroh-blobs' router-based serving means peer-assisted downloads "just work" once peers know where to look
- The `available_from` list in the UI tracks all known sources, enabling the download function to try multiple peers
- Peer departure cleanup (`remove_peer` on `PeerLeft`) prevents stale source entries
