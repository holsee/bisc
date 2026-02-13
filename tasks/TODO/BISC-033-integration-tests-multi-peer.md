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

## Acceptance Criteria

- [ ] All integration tests pass — `cargo test --test integration`
- [ ] Tests complete within 30 seconds each (timeout prevents hangs)
- [ ] No resource leaks (ports, file handles) after tests complete
- [ ] Tests are deterministic (no flaky failures from timing)
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
