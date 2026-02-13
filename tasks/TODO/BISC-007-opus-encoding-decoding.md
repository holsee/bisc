# BISC-007: Opus Encoding & Decoding

**Phase**: 2 — Voice Calls
**Depends on**: BISC-006

## Description

Encode raw audio samples to Opus and decode back.

## Deliverables

- `bisc-media/src/opus_codec.rs`:
  - `OpusEncoder` — wraps the `opus` crate encoder, configurable bitrate (6-510 kbps), sample rate 48kHz, mono or stereo
  - `encode(samples: &[f32]) -> Vec<u8>` — encodes a frame (20ms = 960 samples at 48kHz) to Opus bytes
  - `OpusDecoder` — wraps the `opus` crate decoder
  - `decode(data: &[u8]) -> Vec<f32>` — decodes Opus bytes back to samples
  - `set_bitrate(bps: u32)` — runtime bitrate adjustment for adaptive quality

## Acceptance Criteria

- [ ] Encode a 20ms frame of silence (960 zeros) and get valid Opus bytes — unit test
- [ ] Decode the encoded bytes back and get 960 samples — unit test
- [ ] Round-trip: encode a known waveform (sine wave), decode, verify output is similar (not bit-exact due to lossy compression, but within tolerance) — unit test
- [ ] Bitrate change takes effect (encode at 16kbps produces smaller output than 128kbps) — unit test
- [ ] Encoder and decoder handle stereo — unit test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
