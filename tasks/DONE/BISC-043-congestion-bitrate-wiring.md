# BISC-043: Congestion & Adaptive Bitrate Wiring

**Phase**: 8 — Integration
**Depends on**: BISC-039

## Problem Statement

`CongestionEstimator` and the adaptive bitrate logic in `bisc-media/src/congestion.rs` exist and are tested, but they are not connected to the live pipelines. `MediaControl::ReceiverReport` messages are defined but never sent during actual calls. Encoder bitrates never adapt to network conditions.

## Deliverables

### 1. Receiver Report Loop

- Each receive pipeline periodically (every 1-2 seconds) sends a `MediaControl::ReceiverReport` to the sender over the reliable control stream
- Report includes: packets received, packets lost, jitter estimate, RTT estimate
- Derive stats from `JitterBuffer` metrics and transport counters

### 2. Sender-Side Bitrate Adaptation

- Sender receives `ReceiverReport` messages from each peer
- Feed reports into `CongestionEstimator`
- `CongestionEstimator` outputs a target bitrate
- Apply to `OpusEncoder::set_bitrate()` for voice
- Apply to `VideoCodec` encoder for video (adjust resolution/framerate/quantizer)
- Log bitrate changes at `info` level

### 3. Keyframe Request Wiring

- When the video decoder loses sync or a peer joins mid-stream, send `MediaControl::RequestKeyframe`
- Sender receives the request and forces the next frame to be a keyframe via the H.264 encoder
- Wire this through the `VideoPipeline`

### 4. Quality Presets

- Wire `AppAction::SetVideoQuality` / `AppAction::SetAudioQuality` (from settings) to encoder configuration
- Presets: Low (360p/24fps/128kbps), Medium (720p/30fps/512kbps), High (1080p/30fps/2Mbps)
- Adaptive bitrate adjusts within the bounds of the selected preset

## Acceptance Criteria

- [x] Receiver reports are sent periodically during active voice/video calls
- [x] Sender adjusts Opus bitrate based on receiver reports
- [x] Sender adjusts video quality based on receiver reports
- [x] Keyframe requests trigger IDR frames from the sender
- [x] Quality presets from settings are respected as upper bounds
- [x] Bitrate changes are logged with before/after values
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo fmt --all --check` passes
