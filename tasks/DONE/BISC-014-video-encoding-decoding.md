# BISC-014: Video Encoding & Decoding (H264)

**Phase**: 3 — Video Calls
**Depends on**: BISC-013

## Description

Encode raw video frames to H.264 and decode back.

## Deliverables

- `bisc-media/src/video_codec.rs`:
  - `VideoEncoder` — wraps `openh264` encoder
    - `new(width, height, framerate, bitrate) -> VideoEncoder`
    - `encode(frame: &RawFrame) -> Vec<Vec<u8>>` — encodes a frame, returns one or more NAL units
    - `set_bitrate(bps: u32)` — runtime bitrate adjustment
    - `force_keyframe()` — force next frame to be an IDR keyframe
  - `VideoDecoder` — wraps `openh264` decoder
    - `decode(nal_units: &[u8]) -> Option<RawFrame>` — decodes NAL units to a raw frame
  - `RawFrame` conversions between RGBA and YUV (I420) as needed by the codec

## Acceptance Criteria

- [x] Encode a synthetic frame (solid color, known dimensions) and get valid H.264 NAL units — unit test
- [x] Decode the encoded NAL units back to a frame with correct dimensions — unit test
- [x] First frame is a keyframe (IDR) — unit test
- [x] `force_keyframe()` produces an IDR at the next encode call — unit test
- [x] Bitrate change affects encoded size (lower bitrate = smaller output over several frames) — unit test
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
