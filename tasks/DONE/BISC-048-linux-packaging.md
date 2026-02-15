# BISC-048: Linux Packaging — Static Binary & AppImage

**Phase**: 9 — Platform Support
**Depends on**: BISC-045

## Problem Statement

The `.cargo/config.toml` links `lstdc++` on Linux for openh264, but there is no verified static binary, no AppImage, and no distribution workflow. GPU drivers (Vulkan, Wayland, X11) must remain dynamically linked as they are system-provided.

## Deliverables

### 1. Static Binary Verification

- Build with `cargo build --release`
- Audit dynamic dependencies with `ldd target/release/bisc`
- Expected dynamic: `libc.so`, `libm.so`, `libpthread.so`, `libvulkan.so` (or Mesa), `libwayland-*.so`, `libX11.so`
- Everything else should be statically linked (SQLite, opus, openh264, rustls)

### 2. musl Static Build (Optional)

- Attempt `cargo build --release --target x86_64-unknown-linux-musl` for fully static binary
- Document if GPU/audio deps prevent this (they likely do — wgpu needs Vulkan, cpal needs ALSA)
- If musl doesn't work, document the minimal dynamic deps clearly

### 3. AppImage

- Create an AppImage bundle for desktop integration:
  - `.desktop` file with icon and categories
  - `AppRun` script or direct binary
  - Bundle any non-system shared libraries if needed
- Use `linuxdeploy` or `appimagetool`
- Test on Ubuntu 22.04+ and Fedora 38+

### 4. Linux-Specific Considerations

- PipeWire/PulseAudio loaded via `dlopen` — verify graceful fallback when neither is present
- Wayland vs X11: verify iced/wgpu works on both (wgpu should handle this)
- Verify `directories` crate uses `~/.config/bisc/` and `~/.local/share/bisc/`
- Flatpak/Snap: document feasibility but don't implement (future work)

## Acceptance Criteria

- [x] Release binary runs on Ubuntu 22.04 and Fedora 38+
- [x] `ldd` output shows only expected system libraries
- [x] AppImage created and runs on a clean Linux install
- [x] `.desktop` file included with appropriate metadata
- [x] Settings persist at `~/.config/bisc/settings.toml`
- [x] Graceful behavior when PipeWire is unavailable (falls back to stub)
- [x] Release binary size documented
