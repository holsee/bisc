# BISC-020: Scoped Audio Capture (Platform Abstraction)

**Phase**: 4 — Screen & App Sharing
**Depends on**: BISC-006

## Description

Capture audio from a specific application. Platform-specific implementations behind a common trait.

## Deliverables

- `bisc-media/src/app_audio.rs` — the `AppAudioCapture` trait:
  - `list_capturable_apps() -> Vec<AppAudioSource>`
  - `start_capture(source: &AppAudioSource) -> AudioStream`
  - `stop_capture()`
- `bisc-media/src/app_audio_linux.rs` — Linux implementation using `pipewire-rs` (loaded via `dlopen`, graceful fallback if unavailable)
- `bisc-media/src/app_audio_stub.rs` — stub implementation for platforms not yet implemented (returns empty list, errors on capture)
- Conditional compilation: `#[cfg(target_os = "linux")]`, etc.

## Acceptance Criteria

- [ ] `AppAudioCapture` trait compiles on all platforms — build test
- [ ] On Linux with PipeWire: `list_capturable_apps()` returns a list (may be empty if no apps playing audio) — integration test
- [ ] On platforms without implementation: stub returns empty list and `start_capture()` returns an error — unit test
- [ ] If PipeWire is not available on Linux, fallback gracefully (no panic, return error) — unit test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
