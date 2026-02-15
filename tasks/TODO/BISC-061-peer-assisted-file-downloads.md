# BISC-061: Peer-Assisted File Downloads

**Phase**: 9 — Validation & Fixes
**Depends on**: BISC-060

## Problem Statement

The README requires: "Peer-assisted downloads: if the original sender is offline, files can be downloaded from any other online user who has already downloaded them."

Currently, only the original file sender is tracked as a source. After a peer downloads a file, they have the blob in their local `MemStore` and the `BlobsProtocol` is registered on their router, so they *can* serve it — but **no one knows to ask them**. The download flow tries peers from the `available_from` list, which only contains the original sender.

## Deliverables

### 1. Broadcast file availability after download

After a peer successfully downloads a file (in the `DownloadFile` action handler), broadcast a `ChannelMessage::FileAvailable` (or re-use `FileAnnounce` with the downloader's endpoint ID) via gossip to inform other peers that this file is now available from an additional source.

### 2. Track additional sources in the files panel

When a `FileAvailable` / `FileAnnounce` message is received for a file already in the store, add the new peer's endpoint ID to the `available_from` list on the `FileEntry` in the files panel.

### 3. Try all known sources on download

The existing `download_file` function already iterates over `peer_node_ids`. Ensure the list includes all peers who have announced availability, not just the original sender.

### 4. Handle peer departure

When a peer leaves the channel, remove them from all `available_from` lists. This prevents download attempts to offline peers (which would time out before trying the next source).

## Acceptance Criteria

- [ ] After downloading a file, the peer broadcasts availability via gossip
- [ ] Other peers add the new source to their `available_from` lists
- [ ] Download attempts try all known sources, not just the original sender
- [ ] Peers removed from `available_from` on channel departure
- [ ] `cargo build --workspace` compiles
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] Integration test: peer A shares file, peer B downloads, peer A goes offline, peer C downloads from peer B
