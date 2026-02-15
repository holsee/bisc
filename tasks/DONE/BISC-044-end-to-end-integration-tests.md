# BISC-044: End-to-End Integration Tests

**Phase**: 8 — Integration
**Depends on**: BISC-038, BISC-041

## Problem Statement

The existing integration tests in `bisc-app/tests/integration/` test channel lifecycle and networking but do not verify the full path: UI action → networking → media/file → UI update. With the wiring tasks complete, we need tests that exercise the integrated system.

## Deliverables

### 1. Voice Call End-to-End Test

- Two peers create/join a channel
- Both start voice pipelines
- Verify audio packets flow bidirectionally (via `PipelineMetrics`)
- Verify mute stops packet flow, unmute resumes
- Verify peer disconnect tears down pipeline cleanly

### 2. File Share End-to-End Test

- Peer A shares a file (hash, chunk, announce via gossip)
- Peer B receives the announcement
- Peer B downloads the file
- Verify file contents match original (SHA-256)
- Test peer-assisted download: A disconnects, C (who already downloaded) serves B

### 3. Multi-Peer Scenario

- 3+ peers in a channel
- Voice flowing between all pairs
- One peer shares a file, all others can download
- One peer leaves, remaining peers unaffected

### 4. Graceful Degradation

- Test with no audio device: voice pipeline skipped, app still works
- Test with no camera: video disabled, voice still works
- Test channel join with invalid ticket: error shown, app recoverable

## Acceptance Criteria

- [x] Voice call e2e test passes with bidirectional audio verified via metrics
- [x] File share e2e test passes with content verification
- [x] Multi-peer test passes with 3 peers
- [x] Graceful degradation tests pass (no crash on missing hardware)
- [x] All tests complete in under 30 seconds
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo fmt --all --check` passes
