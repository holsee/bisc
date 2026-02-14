# Opus Codec Startup Distortion

**Task**: BISC-007
**Date**: 2026-02-14
**Crate/Area**: bisc-media / opus

## Context

Writing unit tests to verify Opus encode/decode round-trip quality with a 440Hz sine wave at 128kbps.

## Discovery

The first few frames encoded by Opus have significantly higher distortion than converged frames. A single-frame round-trip test showed MSE of ~0.13, while a 5-frame test (measuring the last frame) showed MSE of ~0.09.

Additionally, Opus in `Application::Voip` mode applies filtering optimized for speech, which increases distortion for pure tones. The MSE even after convergence was ~0.09 for a simple sine wave at 128kbps — higher than one might expect for such a high bitrate.

For round-trip quality tests:
- Encode multiple frames (5+) before measuring quality
- Use generous MSE thresholds (0.15 for VoIP mode)
- `Application::Audio` mode gives better fidelity for non-speech signals

## Impact

- Quality tests must account for codec warmup
- The voice pipeline should prime the encoder with a few silent frames at startup
- VoIP mode is correct for our use case (voice calls) but pure tones will have higher distortion than speech
- `audiopus_sys` with `static` feature compiles libopus from source via cmake, avoiding the need for `libopus-dev` system package
