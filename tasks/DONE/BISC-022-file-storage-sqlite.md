# BISC-022: File Storage & SQLite Metadata

**Phase**: 5 — File Sharing
**Depends on**: BISC-001

## Description

Manage local file storage directory and track file metadata in SQLite.

## Deliverables

- `bisc-files/src/store.rs`:
  - `FileStore` — manages the storage directory and SQLite database:
    - `new(storage_dir: PathBuf) -> FileStore` — creates directory if needed, opens/creates SQLite DB
    - `add_file(manifest: FileManifest) -> FileId` — records a file's metadata
    - `get_file(file_hash: &[u8; 32]) -> Option<FileManifest>`
    - `set_chunk_received(file_hash, chunk_index)` — marks a chunk as downloaded
    - `get_chunk_bitfield(file_hash) -> ChunkBitfield` — returns which chunks we have
    - `is_complete(file_hash) -> bool`
    - `list_files() -> Vec<FileManifest>` — all known files
    - `file_path(file_hash) -> PathBuf` — `<storage_dir>/bisc/<hex_hash>/<filename>`
  - SQLite schema: `files` table (hash, name, size, chunk_count, complete), `chunks` table (file_hash, chunk_index, received)
  - Dependencies: `rusqlite` with `bundled` feature

## Acceptance Criteria

- [x] `FileStore::new()` creates directory and database — unit test (in temp dir)
- [x] `add_file()` + `get_file()` round-trip — unit test
- [x] `set_chunk_received()` + `get_chunk_bitfield()` correctly tracks chunks — unit test
- [x] `is_complete()` returns true only when all chunks are received — unit test
- [x] `list_files()` returns all added files — unit test
- [x] Database persists across `FileStore` instances (close and reopen) — unit test
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
