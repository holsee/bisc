# BISC-040: Screen Share Integration — Capture to Pipeline to Viewers

**Phase**: 8 — Integration
**Depends on**: BISC-039

## Problem Statement

Screen capture (`scap`), the share pipeline, and the video surface exist but are not connected to the application. Users need to be able to share their screen or a specific window, and other peers should see it as a video stream.

## Deliverables

### 1. Screen Share Lifecycle

- `AppAction::SetScreenShare(true)` → prompt user for display/window selection → start `scap` capture
- Feed captured frames to `SharePipeline` (encode → fragment → send)
- `AppAction::SetScreenShare(false)` → stop capture, notify peers
- Broadcast `MediaStateUpdate` with `screen_sharing` change via gossip

### 2. Display/Window Selection

- Enumerate available displays and windows via `scap`
- Present selection UI (modal or dropdown) before starting capture
- Support sharing full screen or a specific window

### 3. Viewer Side

- When a peer's `MediaStateUpdate` indicates `screen_sharing: true`, prepare to receive screen share stream
- Screen share uses a separate `stream_id` (e.g., 2) from camera video (stream_id 1)
- Decode and render in a `VideoSurface` in the video grid
- Screen shares should be visually distinct from camera feeds (label or border)

### 4. Multi-Stream Support

- A user can share their screen while also having their camera on (two outbound video streams)
- Viewers can see multiple screen shares from different peers simultaneously

## Acceptance Criteria

- [ ] User can start and stop screen sharing from the call screen
- [ ] Remote peers see the shared screen in their video grid
- [ ] Screen share and camera video can run simultaneously
- [ ] UI indicates which peers are sharing their screen
- [ ] Stopping screen share removes the stream from all viewers
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --all --check` passes
