# BISC-015: Frame Fragmentation & Reassembly

**Phase**: 3 — Video Calls
**Depends on**: BISC-008, BISC-014

## Description

Fragment large encoded video frames into datagram-sized `MediaPacket`s and reassemble on the receive side.

## Deliverables

- `bisc-media/src/fragmentation.rs`:
  - `Fragmenter` — splits encoded frame data into `MediaPacket` fragments:
    - `fragment(stream_id, timestamp, is_keyframe, data: &[u8], max_payload_size: usize) -> Vec<MediaPacket>` — returns multiple packets with fragment_index/fragment_count set
  - `Reassembler` — collects fragments and produces complete frames:
    - `push(packet: MediaPacket) -> Option<ReassembledFrame>` — returns complete frame when all fragments for a timestamp arrive
    - Timeout: discard incomplete frames after 100ms
    - Out-of-order fragment handling
  - `ReassembledFrame` — struct with complete encoded data, timestamp, is_keyframe flag

## Acceptance Criteria

- [ ] Small frame (< max_payload_size) produces exactly 1 fragment — unit test
- [ ] Large frame produces correct number of fragments (ceil(data_len / max_payload)) — unit test
- [ ] Fragments reassemble into the original data — unit test
- [ ] Fragments arriving out of order still reassemble correctly — unit test
- [ ] Incomplete frame (missing fragment) is discarded after timeout — unit test
- [ ] Duplicate fragments don't corrupt the output — unit test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
