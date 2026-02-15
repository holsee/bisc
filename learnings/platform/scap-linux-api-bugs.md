# scap 0.1.0-beta.1 Linux API Bugs

**Task**: BISC-057
**Date**: 2026-02-15
**Crate/Area**: scap / platform / screen-capture

## Context

Enabling the `screen-capture` feature by default required the `scap` crate to compile on Linux. The `scap` crate version `0.1.0-beta.1` has bugs in its Linux engine that prevent compilation.

## Discovery

The Linux engine code in `scap/src/capturer/engine/linux/mod.rs` has two classes of bugs:

1. **Wrong Frame enum paths**: The code references `Frame::RGBx(...)`, `Frame::RGB(...)`, `Frame::XBGR(...)`, `Frame::BGRx(...)` but the actual `Frame` enum only has `Frame::Audio(AudioFrame)` and `Frame::Video(VideoFrame)`. The correct paths are `Frame::Video(VideoFrame::RGBx(...))` etc.

2. **Wrong display_time type**: The code passes `timestamp as u64` for `display_time` fields, but all frame structs expect `SystemTime`, not `u64`. The fix is to convert the PipeWire nanosecond timestamp to `SystemTime` via `UNIX_EPOCH + Duration::from_nanos(timestamp as u64)`.

Additionally, the `scap::get_all_targets()` function returns `Vec<Target>` (not `Result<Vec<Target>>`), and `Display`/`Window` structs don't have `width`/`height` or `app_name` fields respectively — these were assumed by our screen_capture wrapper.

## Impact

We vendored the `scap` crate under `vendor/scap/` with a `[patch.crates-io]` entry in the workspace `Cargo.toml` to fix these issues. This patch must be maintained until scap releases a version that fixes the Linux engine. When upgrading scap, the patch can be removed and the vendor directory deleted.

The `nokhwa` camera crate also has a non-Send `Camera` type that cannot be used in `tokio::spawn`. Camera capture must run on a dedicated `std::thread::spawn` thread, with the camera opened on that same thread (not opened first and moved).
