# nokhwa Requires libclang and V4L2 Dev Headers on Linux

**Task**: BISC-013
**Date**: 2026-02-14
**Crate/Area**: bisc-media / nokhwa

## Context

Adding camera capture support using the `nokhwa` crate with `input-native` feature for V4L2 on Linux.

## Discovery

The nokhwa `input-native` feature on Linux depends on `v4l2-sys-mit`, which uses `bindgen` at build time to generate V4L2 FFI bindings. This requires:

1. **libclang** — bindgen needs `libclang.so` to parse C headers. Install with `apt install libclang-dev`.
2. **V4L2 kernel headers** — `linux/videodev2.h` must be present. Usually provided by `linux-libc-dev` (often pre-installed) or `libv4l-dev`.

Without `libclang-dev`, the build fails with: "Unable to find libclang: couldn't find any valid shared libraries matching ['libclang.so', ...]"

## Impact

- The `video` feature in `bisc-media` is kept optional (non-default), so `cargo build --workspace` succeeds without these system dependencies
- CI environments that want to compile with `--features video` need `libclang-dev` and `linux-libc-dev` installed
- Same pattern as the `audio` feature requiring `libasound2-dev` for cpal/ALSA
