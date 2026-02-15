# iroh-blobs as Alternative to Custom File Transfer

**Task**: BISC-052
**Date**: 2026-02-15
**Crate/Area**: iroh-blobs, bisc-files

## Context

bisc implemented a custom file transfer protocol with SHA-256 chunking, bitfield-based availability tracking, a hand-rolled request/response wire format over raw QUIC bi-streams, and a custom `SwarmDownloader`. This was ~1300 lines of code.

## Discovery

The `iroh-blobs` crate provides all of this out of the box:
- **BLAKE3 verified streaming** — content integrity at every byte, not just per-chunk
- **Resumable downloads** — partial data is tracked automatically
- **Content-addressed storage** — deduplication for free
- **`BlobsProtocol`** — a `ProtocolHandler` that plugs into `iroh::protocol::Router`
- **`MemStore` / `FsStore`** — in-memory or filesystem-backed blob storage

Key API patterns:
- `blob_store.blobs().add_path(file_path)` to import a file (returns BLAKE3 hash)
- `endpoint.connect(peer_id, iroh_blobs::ALPN)` to connect to a peer's blob server
- `blob_store.remote().execute_get(connection, GetRequest::blob(hash))` to download
- `blob_store.blobs().export(hash, destination)` to export to filesystem

Version compatibility: `iroh-blobs 0.98` depends on `iroh ^0.96`. Earlier versions (0.96) depend on `iroh ^0.94` and are incompatible with `iroh 0.96` due to the `ProtocolHandler` trait being from different crate versions. Always check the iroh dependency version in iroh-blobs' Cargo.toml.

`BlobsProtocol::new()` in 0.98 takes an `events: Option<EventSender>` parameter (pass `None` if events aren't needed).

## Impact

Replacing the custom implementation with iroh-blobs reduced the codebase by ~1300 lines (2038 deleted, 733 added). The remaining `FileStore` in bisc-files is now a thin SQLite metadata layer (file names, sizes, completion status) while iroh-blobs handles all actual storage and transfer.
