# BISC-023: File Chunking & Transfer Protocol

**Phase**: 5 — File Sharing
**Depends on**: BISC-005, BISC-022, BISC-002

## Description

Split files into chunks, transfer over iroh reliable streams, and reassemble.

## Deliverables

- `bisc-files/src/transfer.rs`:
  - `FileSender`:
    - `send_file(path: PathBuf, connection: &PeerConnection) -> Result<FileManifest>` — reads file, computes SHA-256 hash, splits into 256KB chunks, sends chunk-by-chunk over a reliable stream with progress tracking
  - `FileReceiver`:
    - `receive_file(connection: &PeerConnection, manifest: &FileManifest, store: &FileStore) -> Result<()>` — receives chunks, writes to storage, updates `FileStore`, verifies SHA-256 on completion
  - Wire format: each chunk is sent as a length-prefixed `FileChunk` message on the stream
  - Integrity: full file SHA-256 verification after all chunks received

## Acceptance Criteria

- [ ] A file can be sent and received between two peers — integration test (use a temp file)
- [ ] Received file content matches the original (SHA-256 verified) — integration test
- [ ] Large file (> 1MB, multiple chunks) transfers correctly — integration test
- [ ] Small file (< 256KB, single chunk) transfers correctly — integration test
- [ ] Transfer handles connection drop gracefully (returns error, doesn't panic) — integration test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
