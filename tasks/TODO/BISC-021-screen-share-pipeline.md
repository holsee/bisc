# BISC-021: Screen Share Pipeline

**Phase**: 4 — Screen & App Sharing
**Depends on**: BISC-018, BISC-019, BISC-020

## Description

Integrate screen/window capture into the video pipeline so a user can share their screen to peers.

## Deliverables

- `bisc-media/src/share_pipeline.rs`:
  - `SharePipeline` — similar to `VideoPipeline` but sources from `ScreenCapture` instead of `Camera`
    - Uses stream_id=2 (or higher) to distinguish from camera video
    - Optionally includes scoped audio from the captured app (via `AppAudioCapture`), sent as a separate audio stream (stream_id=3)
  - A user can share multiple streams simultaneously (screen + camera, or multiple screens)
  - `MediaStateUpdate` gossip message is broadcast when sharing starts/stops

## Acceptance Criteria

- [ ] Screen share produces `MediaPacket`s with stream_id=2 — integration test
- [ ] Camera and screen share can run simultaneously (different stream_ids) — integration test
- [ ] Starting a share broadcasts `MediaStateUpdate` via gossip — integration test
- [ ] Stopping a share broadcasts updated `MediaStateUpdate` — integration test
- [ ] If scoped audio is available, audio packets are sent on a separate stream_id — integration test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
