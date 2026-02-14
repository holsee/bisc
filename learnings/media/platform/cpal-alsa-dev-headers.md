# cpal Requires ALSA Dev Headers on Linux

**Task**: BISC-006
**Date**: 2026-02-14
**Crate/Area**: bisc-media / cpal

## Context

Adding `cpal` as a dependency for audio I/O. Building on Linux (WSL2).

## Discovery

`cpal` on Linux depends on `alsa-sys`, which requires `libasound2-dev` (ALSA development headers) to compile. Without this system package, the build fails with:

```
The system library `alsa` required by crate `alsa-sys` was not found.
```

Since not all build environments have ALSA headers (CI, WSL2, headless servers), the `cpal` dependency must be made optional.

## Impact

- Made `cpal` an optional dependency behind an `audio` feature flag in `bisc-media`
- The `audio` feature is NOT in `default` features — must be explicitly enabled
- All audio-related code is gated with `#[cfg(feature = "audio")]`
- Integration tests use `#![cfg(feature = "audio")]` at the crate level
- Tests skip gracefully on systems without audio support (using `match` on errors instead of `unwrap`)
- On a system with ALSA headers: `cargo build -p bisc-media --features audio`
- On a system without: `cargo build -p bisc-media` (audio module simply not compiled)
