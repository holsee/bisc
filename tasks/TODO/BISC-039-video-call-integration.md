# BISC-039: Video Call Integration — Camera to Pipeline to GPU Surface

**Phase**: 8 — Integration
**Depends on**: BISC-038

## Problem Statement

`VideoPipeline`, `Camera`, `VideoSurface`, and `VideoGrid` all work independently but are never connected. When a user enables their camera, the video should encode, send over the network, decode on the remote side, and render in the video grid.

## Deliverables

### 1. Camera Lifecycle

- `AppAction::SetCamera(true)` → start camera capture via `nokhwa`, feed frames to `VideoPipeline`
- `AppAction::SetCamera(false)` → stop camera capture, send keyframe-less stream end
- Handle no camera gracefully (log warning, disable camera button)
- Broadcast `MediaStateUpdate` with `video_enabled` change via gossip

### 2. Video Pipeline Wiring

- On peer connect + peer has video enabled: create `VideoPipeline` for that connection
- Send loop: camera frames → H.264 encode → fragment → QUIC datagrams
- Receive loop: QUIC datagrams → reassemble → H.264 decode → `VideoSurface::update_frame()`

### 3. VideoSurface ↔ Iced Integration

- Each remote peer's decoded video feeds a `VideoSurface` instance
- `VideoGrid` arranges all active `VideoSurface` widgets in the call screen
- Frame updates trigger Iced view redraws (via message or subscription tick)
- Local camera preview: render own camera feed in a smaller surface

### 4. Media Negotiation

- On peer connect, exchange `MediaOffer`/`MediaAnswer` over the reliable control stream
- Select codec (H.264), resolution, framerate based on negotiation
- Respect negotiated parameters in encoder configuration

## Acceptance Criteria

- [ ] Enabling camera shows local preview in the call screen
- [ ] Remote peer sees the video stream in their video grid
- [ ] Disabling camera stops the video stream and removes the surface
- [ ] Video grid rearranges when peers join/leave or toggle video
- [ ] Media negotiation completes before video flows
- [ ] No camera available: camera toggle is disabled or shows helpful message
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --all --check` passes
