# BISC-024: Peer-Assisted Downloads

**Phase**: 5 — File Sharing
**Depends on**: BISC-023

## Description

Download file chunks from any peer that has them, not just the original sender.

## Deliverables

- `bisc-files/src/swarm_download.rs`:
  - `SwarmDownloader`:
    - `download(file_hash, peers: &[PeerConnection], store: &FileStore) -> Result<()>` — downloads missing chunks from available peers
    - Queries each peer's `ChunkBitfield` for the file
    - Distributes chunk requests across peers that have the needed chunks
    - Downloads chunks in parallel from multiple peers
    - Falls back to other peers if one disconnects
  - `ChunkServer`:
    - Handles incoming chunk requests from peers
    - Reads chunks from local `FileStore` and sends them
    - Only serves chunks that have been fully received and verified
  - Integration: `FileAnnounce` gossip message triggers peers to record file availability. When a user clicks "Download", `SwarmDownloader` checks all connected peers for chunks.

## Acceptance Criteria

- [ ] Download works when original sender is the only source — integration test (3 peers: A shares, B downloads from A)
- [ ] Download works when original sender is offline but another peer has the file — integration test (A shares to B, A disconnects, C downloads from B)
- [ ] Chunks are downloaded from multiple peers in parallel when available — integration test (A and B both have the file, C downloads chunks from both)
- [ ] Download resumes if a peer disconnects mid-transfer — integration test
- [ ] `ChunkServer` only serves complete, verified chunks — unit test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
