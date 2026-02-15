# BISC-052: Adopt iroh-blobs for File Transfer

**Phase**: 10 — Improvements
**Depends on**: BISC-049

## Problem Statement

`bisc-files` implements a custom file transfer protocol: SHA-256 chunking, bitfield-based availability tracking, a hand-rolled request/response wire format over raw QUIC bi-streams, and a custom `SwarmDownloader`. This is a significant amount of code to maintain and get right.

The `iroh-blobs` crate (used by [sendme](https://github.com/n0-computer/sendme)) provides all of this out of the box:

- **BLAKE3 verified streaming** — content integrity at every byte, not just per-chunk
- **Resumable downloads** — partial data is tracked automatically; only missing ranges are fetched
- **Content-addressed storage** — deduplication for free
- **`BlobsProtocol`** — a `ProtocolHandler` that plugs directly into `iroh::protocol::Router`
- **Collection support** — `HashSeq` for multi-file transfers
- **Proven protocol** — battle-tested in iroh's ecosystem

Adopting `iroh-blobs` would replace the custom chunking, transfer, and swarm download code with a well-tested library, reducing our maintenance burden and improving reliability.

## Deliverables

### 1. Add iroh-blobs dependency

- Add `iroh-blobs` to the workspace dependencies
- Use `FsStore` (or `MemStore` for testing) for blob storage

### 2. Register BlobsProtocol with Router

- In `bisc-net`, register `BlobsProtocol` with the Router alongside gossip and media protocols
- This replaces the manual `spawn_chunk_server` approach

### 3. Replace file sharing flow

- **Share**: import file into blob store via `store.add_path()`, get BLAKE3 hash, announce hash+name via gossip
- **Download**: connect to peer using `iroh_blobs::protocol::ALPN`, use `db.remote().execute_get()` to fetch
- **Announce**: gossip message carries BLAKE3 hash instead of SHA-256

### 4. Simplify bisc-files

- Remove custom `ChunkServer`, `SwarmDownloader`, `FileSender`, bitfield tracking
- `FileStore` becomes a thin wrapper around iroh-blobs `FsStore`
- Keep the SQLite metadata layer if needed for UI (file names, sender info)

### 5. Update bisc-app integration

- Remove `spawn_chunk_server` from `PeerConnected` handler
- Update `DownloadFile` handler to use iroh-blobs API
- Update `FileAnnounced` to work with BLAKE3 hashes

### 6. Remove dead code

- Remove `FILES_ALPN` (`b"bisc/files/0"`) from `bisc-net/src/endpoint.rs` — it is defined and advertised in `alpns()` but never registered with a handler, so no connection on it would ever be accepted. It is replaced by `iroh_blobs::protocol::ALPN`.
- Remove it from the `alpns()` call in both `BiscEndpoint::new()` and `BiscEndpoint::for_testing()`
- Remove any now-unused imports or references across the workspace

## Acceptance Criteria

- [x] `iroh-blobs` is used for all file storage and transfer
- [x] Files shared by one peer can be downloaded by another
- [x] Downloads are resumable (partial progress survives restart) — iroh-blobs handles this natively; MemStore used for now (FsStore for persistence in production)
- [x] Content integrity is verified via BLAKE3
- [x] Custom chunking/transfer code is removed from `bisc-files`
- [x] `FILES_ALPN` removed from `bisc-net` — no dead/misleading ALPN advertisement
- [x] `BlobsProtocol` is registered with the Router
- [x] Existing file sharing UI (announce, download, progress) still works
- [x] `cargo build --workspace` compiles
- [x] `cargo fmt --all --check` passes
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo test --workspace` — all tests pass
