# BISC-010: Media Negotiation (Offer/Answer)

**Phase**: 2 — Voice Calls
**Depends on**: BISC-005, BISC-002

## Description

When two peers connect, exchange `MediaOffer`/`MediaAnswer` over a reliable stream to agree on codecs and quality.

## Deliverables

- `bisc-media/src/negotiation.rs`:
  - `negotiate_as_offerer(stream: &mut BiStream) -> MediaAnswer` — sends `MediaOffer` with local capabilities, receives `MediaAnswer`
  - `negotiate_as_answerer(stream: &mut BiStream) -> MediaAnswer` — receives `MediaOffer`, selects best matching codecs/quality, sends `MediaAnswer`
  - Codec selection logic: prefer Opus for audio, prefer H264 then VP8 for video, negotiate to the lower of the two peers' max resolution/framerate
- Messages serialized with `postcard` over the reliable stream, length-prefixed

## Acceptance Criteria

- [x] Two peers negotiate successfully with matching capabilities — integration test
- [x] Negotiation picks the lower resolution when peers differ — unit test
- [x] Negotiation fails gracefully when no common codec exists — unit test (returns error, doesn't panic)
- [x] Length-prefixed messages are correctly framed over the reliable stream — unit test
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
