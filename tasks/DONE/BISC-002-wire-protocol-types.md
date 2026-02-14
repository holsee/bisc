# BISC-002: Wire Protocol Types (`bisc-protocol`)

**Phase**: 1 — Foundation & Channel Infrastructure
**Depends on**: BISC-001

## Description

Define all shared message types used across crates for gossip, media, file transfer, and control messages.

## Deliverables

- `bisc-protocol/src/lib.rs` re-exports all protocol modules
- `bisc-protocol/src/channel.rs` — `ChannelMessage` enum (`PeerAnnounce`, `PeerLeave`, `Heartbeat`, `MediaStateUpdate`, `FileAnnounce`, `TicketRefresh`)
- `bisc-protocol/src/media.rs` — `MediaPacket`, `MediaOffer`, `MediaAnswer`, `MediaControl`, `CodecInfo`, `QualityPreset`
- `bisc-protocol/src/file.rs` — `FileRequest`, `FileChunk`, `FileManifest`, `ChunkBitfield`
- `bisc-protocol/src/ticket.rs` — `BiscTicket` struct with `to_string()` / `from_str()` (base32 serialization with `bisc1` prefix), `topic_from_secret()` function
- All types derive `Serialize`, `Deserialize` (via `serde`), `Debug`, `Clone`
- Serialization uses `postcard` for compact binary encoding
- Dependencies: `serde`, `postcard`, `sha2`, `data-encoding` (base32), `bytes`

## Acceptance Criteria

- [x] All types compile and derive required traits
- [x] `BiscTicket` round-trips through `to_string()` / `from_str()` — unit test
- [x] `topic_from_secret()` is deterministic — same secret always produces same `TopicId` — unit test
- [x] All `ChannelMessage` variants round-trip through `postcard` serialize/deserialize — unit test
- [x] All `MediaPacket` variants round-trip through `postcard` — unit test
- [x] All `FileRequest`/`FileChunk` types round-trip — unit test
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
