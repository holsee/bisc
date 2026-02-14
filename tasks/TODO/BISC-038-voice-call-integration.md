# BISC-038: Voice Call Integration — Audio I/O to Pipeline to Network

**Phase**: 8 — Integration
**Depends on**: BISC-037

## Problem Statement

`VoicePipeline`, `AudioInput`, `AudioOutput`, and `OpusEncoder`/`OpusDecoder` all work independently but are never connected. When a peer joins a channel and both have audio enabled, voice should flow automatically.

## Deliverables

### 1. Audio Device Setup

On channel join (when `PeerConnected` fires for a peer):

- Create `AudioInput` (cpal microphone capture) → feeds `mpsc` channel
- Create `AudioOutput` (cpal speaker playback) → reads `mpsc` channel
- Handle device enumeration failures gracefully (no mic = no send, no speaker = no receive)
- Log selected device names at `info` level

### 2. Voice Pipeline Lifecycle

- When a peer connects (`ChannelEvent::PeerConnected`), create a `VoicePipeline` for that peer's QUIC connection
- Call `pipeline.start()` to begin send/receive loops
- When peer disconnects (`ChannelEvent::PeerLeft`), call `pipeline.stop()` and clean up
- When user leaves channel, stop all pipelines

### 3. Mute/Unmute Wiring

- `AppAction::SetMic(false)` → `VoicePipeline::set_muted(true)` for all active pipelines
- `AppAction::SetMic(true)` → `VoicePipeline::set_muted(false)`
- Broadcast `MediaStateUpdate` via gossip so peers see mute state

### 4. Multi-Peer Audio Mixing

- Each peer gets its own `VoicePipeline` and decoded output channel
- Mix all peer audio streams before sending to `AudioOutput`
- Simple additive mixing with clipping prevention

## Acceptance Criteria

- [ ] Two peers in a channel can hear each other's voice (manual test on localhost)
- [ ] Muting stops audio transmission (verify via `PipelineMetrics::packets_sent` not incrementing)
- [ ] Unmuting resumes audio transmission
- [ ] Peer disconnect tears down the pipeline cleanly (no orphaned tasks)
- [ ] Graceful handling when no audio device is available (app doesn't crash, logs warning)
- [ ] Multiple peers: audio from all peers is mixed and played back
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --all --check` passes
