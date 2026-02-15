# BISC-045: Cross-Platform CI — Build & Test on Windows, macOS, Linux

**Phase**: 9 — Platform Support
**Depends on**: BISC-036

## Problem Statement

All development and testing has been done on Linux. The codebase *should* compile and work on Windows and macOS thanks to cross-platform crate abstractions, but there is zero evidence of this — no CI pipeline, no manual test reports, no cross-compilation verification. Platform-specific bugs (cpal backend differences, nokhwa API variations, wgpu driver issues, path handling) could be lurking undetected.

## Deliverables

### 1. GitHub Actions CI Workflow

Create `.github/workflows/ci.yml` with a matrix build:

```yaml
strategy:
  matrix:
    os: [ubuntu-latest, windows-latest, macos-latest]
```

Each platform runs:
- `cargo build --workspace` (default features — no audio/video/screen-capture hardware deps)
- `cargo build --workspace --all-features` (verify feature-gated code compiles)
- `cargo clippy --workspace -- -D warnings`
- `cargo fmt --all --check`
- `cargo test --workspace` (tests that don't need hardware)

### 2. System Dependencies Documentation

Document per-platform build prerequisites:

- **Linux**: `libasound2-dev` (cpal), `libclang-dev` (nokhwa), `libdbus-1-dev` (scap), PipeWire headers
- **Windows**: MSVC toolchain, no extra system deps (everything static/bundled)
- **macOS**: Xcode command line tools, no extra system deps

Add a `BUILDING.md` or section in README with setup instructions per platform.

### 3. Cross-Compilation Smoke Test

Verify the binary can be cross-compiled (even if not run):

- Linux → Windows: `cargo build --target x86_64-pc-windows-gnu` (if feasible)
- Document any cross-compilation blockers

### 4. Platform-Specific Test Annotations

- Tag tests that require hardware with `#[ignore]` and a comment explaining why
- Ensure all non-hardware tests pass on all three platforms
- Add `#[cfg(target_os = "...")]` test blocks for platform-specific code paths

## Acceptance Criteria

- [x] CI workflow exists and runs on push/PR for all three platforms
- [x] `cargo build --workspace` succeeds on Windows, macOS, and Linux in CI
- [x] `cargo test --workspace` passes on all three platforms (non-hardware tests)
- [x] `cargo clippy --workspace -- -D warnings` passes on all three platforms
- [x] Build prerequisites documented per platform
- [x] CI badge in README showing build status
