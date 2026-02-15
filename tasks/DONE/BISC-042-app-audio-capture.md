# BISC-042: Application Audio Capture — Platform Implementations

**Phase**: 8 — Integration
**Depends on**: BISC-038

## Problem Statement

The `AppAudioCapture` trait exists with a stub implementation and a PipeWire skeleton that doesn't actually capture audio. README requirements 3 and 4 require sharing audio from specific applications (e.g., Spotify, VLC) without sharing the screen. Windows (WASAPI) and macOS (ScreenCaptureKit) implementations are missing entirely.

## Deliverables

### 1. Linux — PipeWire Implementation

Complete the existing `PipeWireAppAudioCapture`:

- Use `pipewire-rs` (loaded dynamically via dlopen) to enumerate application audio nodes
- `list_capturable_apps()` → list PipeWire audio streams with their associated application names
- `start_capture()` → create a virtual sink, link the target app's audio node to it, read samples
- `stop_capture()` → unlink and destroy the virtual sink
- Fallback: PulseAudio via `pa_stream_set_monitor_stream` if PipeWire unavailable

### 2. Windows — WASAPI Implementation

- Use `wasapi` crate for `AudioClient::new_application_loopback_client(pid, include_tree)`
- `list_capturable_apps()` → enumerate audio sessions via `IAudioSessionEnumerator`
- `start_capture()` → start loopback capture for the target process
- `stop_capture()` → stop capture
- Conditional compilation: `#[cfg(target_os = "windows")]`

### 3. macOS — ScreenCaptureKit Implementation

- Use `objc2` bindings to ScreenCaptureKit
- `list_capturable_apps()` → enumerate `SCRunningApplication` instances
- `start_capture()` → create `SCContentFilter` with `capturesAudio = true` targeting the app
- `stop_capture()` → stop the capture session
- Conditional compilation: `#[cfg(target_os = "macos")]`

### 4. UI Integration

- Add "Share App Audio" option in the call screen
- Show list of capturable applications
- User selects an app → start capture → encode with Opus → send as separate audio stream
- Remote peers hear the app audio mixed with (or separate from) voice

### 5. Standalone Audio Sharing

- Support sharing app audio without sharing the screen (README requirement 4)
- Separate stream_id for app audio vs voice vs screen share audio

## Acceptance Criteria

- [x] Linux: PipeWire app audio capture works with a real application (manual test)
- [x] Linux: graceful fallback to stub when PipeWire is unavailable
- [x] Platform implementations are behind `#[cfg(target_os = "...")]` gates
- [x] `create_app_audio_capture()` factory returns the correct implementation per platform
- [x] App audio streams are encoded and sent to peers as a distinct stream
- [x] UI shows list of capturable apps and allows selection
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo fmt --all --check` passes

## Notes

- Windows and macOS implementations may need to be developed on those platforms
- Linux implementation can be validated in the current dev environment if PipeWire is available
- Dynamic loading (`dlopen`) is preferred to avoid hard link-time dependencies on PipeWire/PulseAudio
