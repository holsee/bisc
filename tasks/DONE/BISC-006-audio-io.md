# BISC-006: Audio I/O (`cpal` Integration)

**Phase**: 2 — Voice Calls
**Depends on**: BISC-001

## Description

Capture audio from the microphone and play audio to the speakers using `cpal`.

## Deliverables

- `bisc-media/src/audio_io.rs`:
  - `AudioInput` — wraps a `cpal::Stream` for microphone capture, produces `Vec<f32>` sample buffers via a channel
  - `AudioOutput` — wraps a `cpal::Stream` for speaker playback, consumes `Vec<f32>` sample buffers via a channel
  - `list_input_devices()` / `list_output_devices()` — enumerate available audio devices
  - `AudioConfig` — sample rate (48000), channels (mono/stereo), buffer size
- Error handling for device-not-found, permission denied
- Dependencies: `cpal`

## Acceptance Criteria

- [x] `list_input_devices()` returns at least one device (or an empty list without panicking) — unit test
- [x] `list_output_devices()` returns at least one device — unit test
- [x] `AudioInput` can be created, started, and stopped without panic — integration test
- [x] `AudioOutput` can be created, started, and stopped without panic — integration test
- [x] Audio loopback test: capture from input, feed to output (manual verification, but test that the pipeline runs for 1 second without errors) — integration test
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
