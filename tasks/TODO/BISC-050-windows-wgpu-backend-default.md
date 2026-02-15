# BISC-050: Windows wgpu Backend Default (DX12)

**Phase**: 9 — Validation & Fixes
**Depends on**: BISC-036

## Problem Statement

On Windows, the Iced/wgpu application crashes with `STATUS_ACCESS_VIOLATION` (0xc0000005) on startup. The crash occurs during GPU surface creation when wgpu's automatic backend selection picks Vulkan, which fails on certain Windows configurations.

Setting `WGPU_BACKEND=dx12` resolves the crash. DX12 is the native, most reliable GPU backend on Windows.

## Deliverables

### 1. Default to DX12 on Windows

- At the top of `main()`, before any Iced/wgpu initialization, set `WGPU_BACKEND=dx12` if the environment variable is not already set.
- Guard with `#[cfg(target_os = "windows")]` so it only applies on Windows.
- Users can still override by setting `WGPU_BACKEND` explicitly.

**Note**: This fix has already been applied in the current working tree. This task formalizes it and ensures it passes validation.

## Acceptance Criteria

- [x] `WGPU_BACKEND` defaults to `dx12` on Windows when not explicitly set
- [x] Application launches without crash on Windows
- [x] Users can override via `WGPU_BACKEND` environment variable
- [ ] `cargo build --workspace` compiles
- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo test --workspace` — all existing tests pass
