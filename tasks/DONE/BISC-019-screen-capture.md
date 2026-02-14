# BISC-019: Screen Capture

**Phase**: 4 — Screen & App Sharing
**Depends on**: BISC-001

## Description

Capture full screen or specific application windows using `scap`.

## Deliverables

- `bisc-media/src/screen_capture.rs`:
  - `ScreenCapture`:
    - `list_displays() -> Vec<DisplayInfo>` — enumerate available displays
    - `list_windows() -> Vec<WindowInfo>` — enumerate capturable application windows
    - `capture_display(display_id) -> ScreenCaptureStream` — capture full screen
    - `capture_window(window_id) -> ScreenCaptureStream` — capture specific window
  - `ScreenCaptureStream` — async stream of `RawFrame`s from the capture source
  - Permission handling: detect and surface permission errors (macOS screen recording permission, etc.)

## Acceptance Criteria

- [x] `list_displays()` runs without panic — unit test
- [x] `list_windows()` runs without panic — unit test
- [x] Display capture starts and produces frames with correct dimensions (or returns permission error) — integration test
- [x] Window capture starts and produces frames — integration test
- [x] Capture stream can be stopped and the resources freed — integration test
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
