# BISC-018: End-to-End Video Call Pipeline

**Phase**: 3 — Video Calls
**Depends on**: BISC-012, BISC-013, BISC-014, BISC-015, BISC-016, BISC-017

## Description

Wire together camera capture, H264 encode, fragmentation, datagram transport, reassembly, decode, and VideoSurface rendering.

## Deliverables

- `bisc-media/src/video_pipeline.rs`:
  - `VideoPipeline` — orchestrates the full video pipeline:
    - Captures frames from `Camera` at configured framerate
    - Encodes with `VideoEncoder`
    - Fragments via `Fragmenter`
    - Sends fragments as `MediaPacket`s via `MediaTransport` (stream_id=1)
    - Receives fragments from remote
    - Reassembles via `Reassembler`
    - Pushes through video `JitterBuffer`
    - Decodes with `VideoDecoder`
    - Delivers decoded `RawFrame` to `VideoSurface` for rendering
  - Camera on/off toggle
  - Quality adjustment: changes encoder bitrate/resolution based on `QualityPreset` or `MediaControl::QualityChange`
  - Keyframe request handling: when `MediaControl::RequestKeyframe` is received, calls `force_keyframe()` on encoder

## Acceptance Criteria

- [ ] Two peers exchange video frames (verify fragment packet count > 0 on both sides after 3 seconds) — integration test
- [ ] Turning camera off stops packet transmission — integration test
- [ ] Turning camera on resumes transmission — integration test
- [ ] Keyframe request triggers an IDR frame — integration test
- [ ] Quality change adjusts encoder bitrate — integration test
- [ ] Pipeline shuts down cleanly when peer disconnects — integration test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
