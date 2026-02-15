# BISC-041: File Sharing Integration — UI to Store to Transfer

**Phase**: 8 — Integration
**Depends on**: BISC-037

## Problem Statement

`FileStore`, `FileTransfer`, and `SwarmDownload` work independently, and `FilesPanel` renders a file list, but nothing connects them. Users cannot share or download files.

## Deliverables

### 1. File Sharing Flow (Sender)

- `AppAction::OpenFilePicker` → open native file dialog
- User selects a file → hash it, chunk it, store in `FileStore`
- Broadcast `FileAnnounce` via gossip with file metadata
- Serve chunk requests from peers via `FileTransfer` over reliable QUIC streams

### 2. File Download Flow (Receiver)

- `AppAction::DownloadFile(hash)` → initiate download from available peers
- Use `SwarmDownload` if multiple peers have the file
- Track download progress, update `FilesPanel` with percentage
- On completion, verify SHA-256 hash, mark complete in `FileStore`

### 3. File Picker Integration

- Use `rfd` (Rust File Dialog) or equivalent for native file picker dialogs
- Return selected file path to the app for processing

### 4. Storage Directory

- Use configured storage directory from `Settings`
- Files stored at `<storage_dir>/bisc/<hex_hash>/<filename>`
- `FileStore` initialised with this path on app startup

### 5. Progress and Status

- Show upload/download progress in `FilesPanel`
- Show file availability (which peers have it)
- Show completed files with option to open containing folder

## Acceptance Criteria

- [x] User can select and share a file via the file picker
- [x] Shared file appears in all peers' file panels
- [x] User can download a shared file by clicking download
- [x] Download progress is shown in the UI
- [x] Completed files are stored in the configured storage directory
- [x] Peer-assisted downloads work when original sender is offline but another peer has the file
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo fmt --all --check` passes
