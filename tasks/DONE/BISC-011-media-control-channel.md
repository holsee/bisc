# BISC-011: Media Control Channel

**Phase**: 2 — Voice Calls
**Depends on**: BISC-010

## Description

Send and receive `MediaControl` messages (keyframe requests, receiver reports, quality changes) over a reliable stream.

## Deliverables

- `bisc-media/src/control.rs`:
  - `MediaControlSender` — sends `MediaControl` messages on a reliable stream
  - `MediaControlReceiver` — receives `MediaControl` messages as an async stream
  - `ReceiverReportGenerator` — periodically (every 1 second) generates `ReceiverReport` from packet statistics (packets received, lost, jitter, RTT estimate)
  - RTT estimation: timestamp in report, measured on round-trip

## Acceptance Criteria

- [ ] `MediaControl::RequestKeyframe` can be sent and received — integration test
- [ ] `ReceiverReport` is generated with correct packet counts after receiving N packets — unit test
- [ ] RTT estimation produces a reasonable value in a localhost test — integration test
- [ ] Multiple control message types can be interleaved on the same stream — integration test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
