# BISC-013: Camera Capture

**Phase**: 3 — Video Calls
**Depends on**: BISC-001

## Description

Capture video frames from the system camera.

## Deliverables

- `bisc-media/src/camera.rs`:
  - `Camera` — wraps `nokhwa` for camera access
  - `list_cameras() -> Vec<CameraInfo>` — enumerate available cameras
  - `open(camera_id, config) -> Camera` — open a camera with resolution/framerate
  - `capture_frame() -> RawFrame` — capture a single frame (RGBA or NV12)
  - `CameraConfig` — resolution (640x480, 1280x720, 1920x1080), framerate (15, 30, 60)
  - `RawFrame` — struct holding pixel data, width, height, format

## Acceptance Criteria

- [ ] `list_cameras()` runs without panic (returns empty list if no camera) — unit test
- [ ] `Camera` can be opened and closed without panic (skip test if no camera available) — integration test
- [ ] Captured `RawFrame` has correct width/height matching the config — integration test
- [ ] Camera can be stopped and restarted — integration test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
