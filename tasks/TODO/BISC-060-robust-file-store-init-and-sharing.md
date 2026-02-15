# BISC-060: Robust File Store Initialization and Sharing

**Phase**: 9 — Validation & Fixes
**Depends on**: None

## Problem Statement

File sharing fails with `ERROR bisc::app: file sharing failed error=failed to insert file` when a user tries to share a file. Two issues contribute:

### 1. Race condition between init and file operations

In `bisc-app/src/main.rs`, `file_sharing.init()` is called in a `tokio::spawn` when `ChannelCreated` or `ChannelJoined` is received (line 322-328). The `OpenFilePicker` action can fire before init completes. While the code checks for `None` store (returning "file store not initialized"), the race window is real.

### 2. No storage directory validation

The `FileStore::new()` creates the directory with `create_dir_all` but does not verify it is writable. If the storage directory path in settings points to a read-only location, or if the path is malformed, the SQLite `INSERT` fails at runtime with an opaque "failed to insert file" error.

### 3. Init errors are logged but not surfaced

When `file_sharing.init()` fails, it logs an `ERROR` but the user sees no UI feedback. They can then try to share a file and get a confusing error.

## Deliverables

### 1. Eagerly initialize file store

Initialize the file store synchronously during `BiscApp::new()` (or immediately after settings are loaded), not asynchronously in a spawned task. This eliminates the race condition. The store can be re-initialized if the storage directory changes in settings.

### 2. Validate storage directory on settings save

When the user changes the storage directory in settings and saves, validate that the directory exists and is writable (or can be created). Show an error in the settings screen if not.

### 3. Surface file store errors to the user

If file store initialization fails, surface the error via the UI (e.g., a status message on the channel/call screen or an error in the files panel). Don't silently fail.

### 4. Better error context

Add more context to the "failed to insert file" error to include the storage directory path and file name, making it easier to diagnose.

## Acceptance Criteria

- [ ] File store is initialized before any file operations can be attempted
- [ ] No race condition between init and file sharing
- [ ] Storage directory validation on settings save
- [ ] File store init errors are visible in the UI
- [ ] `cargo build --workspace` compiles
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] Unit test: sharing a file with an initialized store succeeds
- [ ] Unit test: sharing a file with an unwritable storage dir produces a clear error
