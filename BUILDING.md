# Building bisc

## Prerequisites

All platforms require:

- **Rust** (stable, edition 2021) — install via [rustup](https://rustup.rs/)
- **CMake** — required by `openh264` (video-codec feature)
- **NASM** — required by `openh264` (video-codec feature)

### Linux

```bash
# Debian/Ubuntu
sudo apt-get install -y \
  libasound2-dev \
  libclang-dev \
  cmake \
  nasm

# Fedora
sudo dnf install -y \
  alsa-lib-devel \
  clang-devel \
  cmake \
  nasm
```

- `libasound2-dev` / `alsa-lib-devel` — required by `cpal` (audio feature)
- `libclang-dev` / `clang-devel` — required by `nokhwa` (video feature)
- PipeWire headers are optional; `cpal` will use PipeWire if available, otherwise falls back to ALSA

### Windows

- **MSVC toolchain** — install via Visual Studio Build Tools or Visual Studio
- CMake and NASM must be on PATH (install via `winget`, `choco`, or manually)
- No additional system libraries needed; all dependencies are statically linked

### macOS

- **Xcode Command Line Tools** — `xcode-select --install`
- CMake and NASM — `brew install cmake nasm`
- No additional system libraries needed; macOS frameworks (CoreAudio, AVFoundation) are used automatically

## Building

```bash
# Default build (no hardware features — voice pipeline, networking, files, UI)
cargo build --workspace

# Full build with all hardware features
cargo build --workspace --all-features
```

## Feature Flags

Features are opt-in. The default build compiles without hardware-specific dependencies.

| Feature | Crate | Description | System dependency |
|---|---|---|---|
| `audio` | bisc-media | Microphone/speaker via cpal | libasound2-dev (Linux) |
| `video` | bisc-media | Camera capture via nokhwa | libclang-dev (Linux) |
| `video-codec` | bisc-media | H.264 encode/decode via openh264 | cmake, nasm |
| `screen-capture` | bisc-media | Screen/window capture via scap | None (uses platform APIs) |

Enable features on `bisc-app`:

```bash
cargo build -p bisc-app --features audio,video,video-codec,screen-capture
```

## Running Tests

```bash
# Run all tests (default features)
cargo test --workspace

# Run with full logging
RUST_LOG=debug cargo test --workspace -- --nocapture

# Run all tests including hardware-dependent ones
cargo test --workspace --all-features
```

## Cross-Compilation

Cross-compiling from Linux to Windows (`x86_64-pc-windows-gnu`) is blocked by
platform-specific dependencies in `iced` (wgpu/winit) and `cpal`. Use native
builds or CI matrix builds for each target platform.
