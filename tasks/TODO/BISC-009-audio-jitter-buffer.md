# BISC-009: Audio Jitter Buffer

**Phase**: 2 — Voice Calls
**Depends on**: BISC-008

## Description

Buffer incoming audio packets to handle network jitter, reorder out-of-order packets, and produce smooth playback.

## Deliverables

- `bisc-media/src/jitter_buffer.rs`:
  - `JitterBuffer` — configurable buffer depth (e.g. 50-200ms)
  - `push(packet: MediaPacket)` — inserts packet, ordered by timestamp
  - `pop() -> Option<MediaPacket>` — returns the next packet if its playout time has arrived, `None` if buffer is empty or too early
  - Handles: packet reordering, duplicate detection, gap detection (missing packets produce silence)
  - Adaptive buffer depth based on observed jitter (stretch when jittery, shrink when stable)
  - Statistics: packets received, packets lost, current buffer depth, average jitter

## Acceptance Criteria

- [ ] Packets inserted in order are returned in order — unit test
- [ ] Packets inserted out of order are reordered correctly — unit test
- [ ] Duplicate packets are dropped — unit test
- [ ] Gap in sequence numbers produces `None` (silence/concealment) at the right time — unit test
- [ ] Buffer depth adapts: simulate high jitter, verify buffer grows; simulate stable network, verify buffer shrinks — unit test
- [ ] Statistics are tracked correctly — unit test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
