# BISC-008: MediaPacket Framing & Datagram Transport

**Phase**: 2 — Voice Calls
**Depends on**: BISC-002, BISC-005

## Description

Send and receive `MediaPacket`s over QUIC datagrams, handling serialization and the send/receive pipeline.

## Deliverables

- `bisc-media/src/transport.rs`:
  - `MediaTransport` — wraps a `PeerConnection`, provides:
    - `send_media_packet(packet: &MediaPacket)` — serializes and sends as datagram
    - `recv_media_packet() -> MediaPacket` — receives and deserializes
  - Handles `postcard` serialization of `MediaPacket`
  - Drops packets that fail deserialization (lossy transport — expected)
  - Tracks send/receive sequence numbers per stream_id for loss detection

## Acceptance Criteria

- [ ] `MediaPacket` can be serialized, sent as datagram, received, and deserialized — integration test with two connected peers
- [ ] Audio-sized packets (~100-200 bytes) are sent and received correctly — integration test
- [ ] Malformed datagrams are dropped without panic — unit test (feed garbage bytes to deserialize)
- [ ] Sequence numbers increment correctly per stream_id — unit test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
