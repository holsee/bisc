# BISC-031: Congestion Estimation & Adaptive Bitrate

**Phase**: 7 — Packaging & Hardening
**Depends on**: BISC-011, BISC-018

## Description

Measure network conditions and automatically adjust media quality.

## Deliverables

- `bisc-media/src/congestion.rs`:
  - `CongestionEstimator`:
    - Consumes `ReceiverReport`s from the control channel
    - Tracks: packet loss rate (%), RTT (ms), jitter (ms)
    - Produces `QualityRecommendation`: target bitrate, resolution, framerate
    - Simple algorithm: if loss > 5%, reduce bitrate by 20%. If loss < 1% for 10 seconds, increase bitrate by 10%. Clamp to min/max.
  - Integration: `VideoPipeline` and `VoicePipeline` subscribe to quality recommendations and adjust encoder settings

## Acceptance Criteria

- [ ] High loss rate produces lower bitrate recommendation — unit test
- [ ] Sustained low loss rate gradually increases bitrate — unit test
- [ ] Bitrate stays within min/max bounds — unit test
- [ ] RTT and jitter are tracked correctly from `ReceiverReport` data — unit test
- [ ] Encoder bitrate actually changes when recommendation is applied — integration test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
