# BISC-059: Wire Screen Capture Fan-Out to Share Pipelines

**Phase**: 9 — Validation & Fixes
**Depends on**: BISC-057

## Problem Statement

Screen capture works locally, but captured frames are **never sent to remote peers**. In `bisc-app/src/screen_share.rs:140`:

```rust
// TODO: wire screen capture fan-out to pipeline's frame_in_tx
drop(frame_in_tx);
```

The `frame_in_tx` — the channel that feeds screen capture frames into a peer's `SharePipeline` — is immediately dropped. The pipeline's send loop receives no input and never transmits screen share video.

This is the same class of bug as BISC-058 (camera fan-out), but for screen sharing.

## Deliverables

### 1. Implement screen capture frame fan-out in `ScreenShareState`

Add a fan-out mechanism that distributes screen capture frames to all active share pipeline inputs. Follow the same pattern chosen in BISC-058 for consistency.

### 2. Connect `frame_in_tx` to the fan-out in `add_peer`

Replace `drop(frame_in_tx)` with registration into the fan-out mechanism.

### 3. Disconnect on `remove_peer`

Ensure removing a peer also removes its `frame_in_tx` from the fan-out.

### 4. Handle capture start/stop with existing pipelines

- If screen capture starts while pipelines already exist, they should begin receiving frames
- If screen capture stops, all pipelines should see the channel close gracefully

## Acceptance Criteria

- [ ] `drop(frame_in_tx)` removed from `ScreenShareState::add_peer`
- [ ] Screen capture frames are distributed to all active share pipeline inputs
- [ ] Adding/removing pipelines while capture is running works correctly
- [ ] Starting capture while pipelines exist begins frame delivery
- [ ] Stopping capture while pipelines exist ends frame delivery cleanly
- [ ] `cargo build --workspace` compiles
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] Unit test verifies fan-out delivers frames to multiple pipeline inputs
