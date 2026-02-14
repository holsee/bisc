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

- [x] Screen share produces `MediaPacket`s with stream_id=2 — integration test
- [x] Camera and screen share can run simultaneously (different stream_ids) — integration test
- [x] Starting a share broadcasts `MediaStateUpdate` via gossip — integration test
- [x] Stopping a share broadcasts updated `MediaStateUpdate` — integration test
- [x] If scoped audio is available, audio packets are sent on a separate stream_id — integration test
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
