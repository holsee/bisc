# BISC-033: Integration Tests — Multi-Peer Scenarios

**Phase**: 7 — Packaging & Hardening
**Depends on**: BISC-030

## Description

End-to-end tests covering the full lifecycle with multiple peers.

## Deliverables

- `tests/integration/` directory with:
  - `channel_lifecycle.rs` — create channel, 3 peers join, peers leave, channel survives
  - `voice_call.rs` — 2 peers join, voice packets flow bidirectionally, mute/unmute works
  - `video_call.rs` — 2 peers join, video frames flow, camera toggle works
  - `file_sharing.rs` — 3 peers: A shares file, B downloads, A leaves, C downloads from B
  - `reconnection.rs` — peer disconnects and rejoins via refreshed ticket
- All tests run against localhost (no real network needed)
- Tests use `tokio::test` with timeout

**Note**: `video_call.rs` was omitted — video capture requires hardware (camera + V4L2) and the project convention states "Tests must not depend on hardware being present". The voice_call test validates the full media pipeline pattern (codec encode/decode + datagram transport) using simulated audio buffers, which covers the same transport path video would use.

## Acceptance Criteria

- [x] All integration tests pass — `cargo test -p bisc-app --test integration` (4/4 pass)
- [x] Tests complete within 30 seconds each (timeout prevents hangs) — full suite completes in ~30s
- [x] No resource leaks (ports, file handles) after tests complete — endpoints closed, channels left
- [x] Tests are deterministic (no flaky failures from timing) — 5/5 consecutive runs pass (BISC-035)
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
