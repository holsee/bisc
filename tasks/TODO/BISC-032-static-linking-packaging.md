# BISC-032: Static Linking & Cross-Platform Packaging

**Phase**: 7 — Packaging & Hardening
**Depends on**: BISC-030

## Description

Configure builds for self-contained portable binaries on all platforms.

## Deliverables

- `.cargo/config.toml` — platform-specific linker flags:
  - Windows: `-C target-feature=+crt-static`
  - Linux: musl target or static linking config
- `Cargo.toml` feature flags: `rustls` (not openssl), `rusqlite/bundled`, `opus/static`
- CI-ready build scripts or documentation for:
  - Windows: portable `.exe`
  - macOS: `.app` bundle structure
  - Linux: static binary
- Verify binary has no unexpected dynamic library dependencies (`ldd` on Linux, `otool` on macOS, `dumpbin` on Windows)

## Acceptance Criteria

- [ ] Linux build produces a binary with minimal dynamic dependencies (libc, libm, GPU drivers only) — verified with `ldd`
- [ ] Binary runs on a clean system without installing additional packages — manual test
- [ ] `cargo build --release` succeeds with all static features enabled
- [ ] Binary size is under 50 MB
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
