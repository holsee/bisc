# BISC-056: Instance Data Directory Override & File Exchange Location

**Phase**: 9 — Validation & Fixes
**Depends on**: BISC-055

## Problem Statement

Two problems, one root cause — file storage paths aren't configurable:

1. **Multi-instance conflicts**: Running two bisc instances on the same machine is a first-class use case (peer-to-peer testing, demo mode, development). Currently all instances share the same config dir (`~/.config/bisc/`) and data dir (`~/.local/share/bisc/`), causing SQLite lock conflicts, settings overwrites, and iroh endpoint collisions.

2. **No control over file exchange location**: Users cannot choose where downloaded files are saved. The `storage_dir` is shown read-only in Settings and `export_blob()` exists but is dead code — downloaded blobs stay in iroh's in-memory `MemStore` and are never written to disk. Users should be able to set their file exchange directory before joining a channel, and downloaded files should actually appear there.

## Deliverables

### 1. Add `--data-dir <PATH>` CLI argument

Use `clap` to parse a single optional argument:

```
bisc --data-dir /tmp/bisc-instance-2
```

When `--data-dir` is provided:
- Config dir → `<data-dir>/config/` (settings.toml lives here)
- Storage dir default → `<data-dir>/data/` (SQLite DB, downloaded files)

When omitted, behaviour is unchanged — platform defaults via the `directories` crate.

### 2. Thread the override through initialization

- Pass the optional data dir from `main()` into `Settings::load()` / `Settings::save()`
- The `storage_dir` in `Settings` should default to `<data-dir>/data/` when a custom data dir is provided, but can still be overridden in the UI or settings.toml
- `FileStore`, `FileSharingState`, and any other components using paths should continue reading from `settings.storage_dir` — no changes needed downstream

### 3. Make storage_dir editable in the Settings screen

Currently `storage_dir` is displayed as read-only text in the Settings screen. Change it to:

- An editable text input showing the current path
- A "Browse" button that opens a native folder picker (`rfd::AsyncFileDialog::pick_folder()`)
- Saved via the existing Save button alongside other settings
- The updated path should be reflected immediately for new file downloads

Add a `StorageDirChanged(String)` message variant to the settings screen and wire it through to `AppAction::SaveSettings`.

### 4. Show storage_dir on the Channel screen

Before creating/joining a channel, the user should see where file exchanges will be saved. Add a read-only display of the current storage directory below the display name field on the channel screen, with a small "Change in Settings" link/button that navigates to Settings.

### 5. Export downloaded blobs to the file exchange directory

Currently `export_blob()` in `file_sharing.rs` is dead code. Wire it into the download flow so that after a blob is downloaded via iroh-blobs, it is exported (written) to `storage_dir/<filename>`. The `FileDownloadComplete` message should carry the destination path so the UI can inform the user where the file was saved.

### 6. Add `clap` dependency

Add `clap` with `derive` feature to `bisc-app/Cargo.toml`.

### 7. Log the active directories at startup

At `info` level, log the resolved config dir and storage dir so operators can verify which instance is using which paths.

## Usage Example

```bash
# Instance A — uses platform defaults, files saved to ~/.local/share/bisc/
bisc

# Instance B — isolated data directory, files saved to /tmp/bisc-b/data/
bisc --data-dir /tmp/bisc-b
```

## Acceptance Criteria

- [ ] `bisc --data-dir /tmp/test-instance` uses `/tmp/test-instance/config/` for settings and `/tmp/test-instance/data/` as default storage dir
- [ ] Running without `--data-dir` uses platform defaults (unchanged behaviour)
- [ ] Config and storage directories are logged at `info` level on startup
- [ ] Two instances with different `--data-dir` values can run simultaneously without conflict
- [ ] Storage directory is editable in Settings screen (text input + folder picker)
- [ ] Channel screen shows the current file exchange directory
- [ ] Downloaded files are exported to `storage_dir` on disk after blob download completes
- [ ] `cargo build --workspace` compiles
- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
