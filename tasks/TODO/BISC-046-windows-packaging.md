# BISC-046: Windows Packaging — Portable .exe

**Phase**: 9 — Platform Support
**Depends on**: BISC-045

## Problem Statement

The `.cargo/config.toml` already configures `+crt-static` for Windows MSVC targets, but there is no build script, no verified portable `.exe`, and no distribution workflow. The README promises "single self-contained binary" — this needs to be validated on Windows.

## Deliverables

### 1. Windows Release Build Verification

- Build with `cargo build --release --target x86_64-pc-windows-msvc`
- Verify the resulting `bisc.exe` runs on a clean Windows machine (no MSVC redistributable)
- Verify no dynamic DLL dependencies beyond system libraries (kernel32, user32, etc.)
- Use `dumpbin /dependents bisc.exe` or `Dependencies` tool to audit

### 2. Binary Size Optimization

- Strip debug symbols: `strip = true` in `[profile.release]`
- LTO: `lto = "thin"` or `lto = true` in release profile
- Document final binary size

### 3. Windows-Specific Considerations

- Verify `directories` crate uses correct Windows paths (`%APPDATA%\bisc\`)
- Verify `rusqlite` bundled SQLite works on Windows
- Test that `settings.toml` is read/written correctly with Windows path separators

### 4. Optional: Windows Installer

- Consider `wix` or `nsis` for optional `.msi` installer
- Not required — portable `.exe` is the primary distribution method
- If pursued, include start menu shortcut and uninstaller

## Acceptance Criteria

- [ ] `bisc.exe` built with `--release` runs on a clean Windows 10/11 machine
- [ ] No MSVC redistributable or other runtime required
- [ ] `dumpbin /dependents` shows only system DLLs
- [ ] Settings persist correctly at `%APPDATA%\bisc\settings.toml`
- [ ] Release binary size documented
- [ ] Release profile optimizations configured in workspace `Cargo.toml`
