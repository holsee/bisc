# Windows wgpu Backend Selection

**Task**: BISC-050
**Date**: 2026-02-15
**Crate/Area**: wgpu, iced, platform/windows

## Context

On Windows, wgpu's automatic backend selection may pick Vulkan, which crashes with `STATUS_ACCESS_VIOLATION` on certain GPU drivers.

## Discovery

Forcing `WGPU_BACKEND=dx12` via environment variable before Iced/wgpu initialization resolves this. DX12 is the native, most reliable backend on Windows.

Key details:
- The env var must be set before `iced::application().run()` is called
- Use `#[cfg(target_os = "windows")]` to only set it on Windows
- Check `std::env::var("WGPU_BACKEND").is_err()` first so users can still override
- Use `unsafe { std::env::set_var(...) }` (unsafe in Rust 2024 edition due to thread safety)

## Impact

Added DX12 default in `main()` with user override capability. This prevents GPU driver crashes on Windows while allowing advanced users to select a different backend if needed.
