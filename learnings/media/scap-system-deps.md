# scap Screen Capture System Dependencies

**Task**: BISC-019
**Date**: 2026-02-14
**Crate/Area**: bisc-media / screen capture

## Context

Implementing screen capture using the `scap` crate (0.1.0-beta.1) for cross-platform display and window capture.

## Discovery

On Linux, `scap` depends on PipeWire and D-Bus via the `libdbus-sys` crate. Building requires:
- `libdbus-1-dev` (D-Bus development headers)
- `pkg-config`
- PipeWire runtime for actual capture functionality

Without these system packages, `cargo build --features screen-capture` fails at the `libdbus-sys` build script.

The `scap` API is synchronous — `Capturer::get_next_frame()` blocks until a frame is available. The implementation wraps this in a `std::thread::spawn` to provide async frame delivery.

## Impact

- The `screen-capture` feature is gated behind `dep:scap` and won't compile without system dependencies
- CI/build environments need `libdbus-1-dev` and `pkg-config` installed
- WSL2 environments typically lack PipeWire, so screen capture won't function at runtime even if it compiles
- Frame format from scap is BGRA on most platforms — needs channel swap to RGBA for the video pipeline
