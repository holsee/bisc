# BISC-058: Wire Camera Frame Fan-Out to Video Pipelines

**Phase**: 9 — Validation & Fixes
**Depends on**: BISC-057

## Problem Statement

Camera capture works for local preview, but captured frames are **never sent to remote peers**. In `bisc-app/src/video.rs:208`:

```rust
drop(frame_in_tx); // TODO: wire camera fan-out in future
```

The `frame_in_tx` — the channel that feeds camera frames into a peer's `VideoPipeline` — is immediately dropped. The pipeline's send loop receives no input and never transmits video.

This means: enabling camera shows a local preview, but the remote peer's screen remains blank.

## Root Cause

The camera task writes frames to `camera_tx`, but there is no fan-out mechanism distributing those frames to each per-peer `VideoPipeline`'s `frame_in_tx`. The architecture needs a hub (similar to the voice audio hub) that:

1. Receives frames from the camera task via `camera_tx`
2. Clones each frame to every active pipeline's `frame_in_tx`
3. Handles pipeline add/remove dynamically

## Deliverables

### 1. Implement camera frame fan-out in `VideoState`

Add a fan-out mechanism that distributes camera frames to all active video pipeline inputs. Options:

- **Option A (hub task)**: Similar to `audio_hub_task` in `voice.rs` — a spawned task that receives camera frames and forwards to all pipeline inputs.
- **Option B (inline)**: Store pipeline `frame_in_tx` senders in a `Vec` and fan-out in the camera capture task itself.

The chosen approach must:
- Forward every camera frame to every active pipeline's `frame_in_tx`
- Handle pipelines being added/removed while the camera is running
- Not block the camera capture loop if a pipeline is slow (use `try_send` or unbounded channels)

### 2. Connect `frame_in_tx` to the fan-out in `add_peer`

Replace `drop(frame_in_tx)` with registration into the fan-out mechanism so the pipeline receives camera frames.

### 3. Disconnect on `remove_peer`

Ensure removing a peer also removes its `frame_in_tx` from the fan-out.

### 4. Handle camera start/stop with existing pipelines

- If the camera starts while pipelines already exist, they should begin receiving frames
- If the camera stops, all pipelines should see the channel close gracefully

## Acceptance Criteria

- [x] `drop(frame_in_tx)` removed from `VideoState::add_peer`
- [x] Camera frames are distributed to all active video pipeline inputs
- [x] Adding/removing pipelines while camera is running works correctly
- [x] Starting camera while pipelines exist begins frame delivery
- [x] Stopping camera while pipelines exist ends frame delivery cleanly
- [x] `cargo build --workspace` compiles
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo test --workspace` passes
- [x] Unit test verifies fan-out delivers frames to multiple pipeline inputs
