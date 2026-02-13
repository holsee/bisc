# BISC-012: End-to-End Voice Call Pipeline

**Phase**: 2 — Voice Calls
**Depends on**: BISC-006, BISC-007, BISC-008, BISC-009, BISC-010, BISC-011

## Description

Wire together audio capture, Opus encode, MediaPacket send, receive, jitter buffer, Opus decode, and audio playback into a working voice call between two peers.

## Deliverables

- `bisc-media/src/voice_pipeline.rs`:
  - `VoicePipeline` — orchestrates the full pipeline:
    - Captures audio from `AudioInput` (48kHz, mono, 20ms frames)
    - Encodes with `OpusEncoder`
    - Wraps in `MediaPacket` (stream_id=0)
    - Sends via `MediaTransport`
    - Receives `MediaPacket` from remote
    - Pushes through `JitterBuffer`
    - Decodes with `OpusDecoder`
    - Plays on `AudioOutput`
  - Mute/unmute: when muted, stop sending packets (don't encode silence)
  - Volume: apply gain to decoded samples before playback
- Integration into `Channel`/`PeerConnection`: when a direct connection is established and media is negotiated, start the voice pipeline

## Acceptance Criteria

- [ ] Two peers establish a channel, voice pipeline starts automatically after negotiation — integration test
- [ ] Audio packets flow in both directions (verify packet count > 0 on both sides after 2 seconds) — integration test
- [ ] Muting stops packet transmission (verify packet count stops increasing) — integration test
- [ ] Unmuting resumes packet transmission — integration test
- [ ] Pipeline shuts down cleanly when peer disconnects — integration test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
