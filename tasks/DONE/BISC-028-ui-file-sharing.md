# BISC-028: Iced UI — File Sharing View

**Phase**: 6 — UI & Settings
**Depends on**: BISC-024, BISC-027

## Description

File sharing panel: view shared files, click to download, see download progress.

## Deliverables

- `bisc-ui/src/screens/files.rs`:
  - `FilesPanel` — Iced view (panel/overlay within call screen) with:
    - List of shared files (name, size, sender, download status)
    - "Download" button per file (only for files not yet downloaded)
    - Download progress bar (chunks received / total)
    - "Share File" button that opens a file picker
    - Indicator showing which peers have each file
  - Emits messages: `ShareFile(PathBuf)`, `RequestDownload(FileHash)`

## Acceptance Criteria

- [x] Panel renders with empty file list — integration test
- [x] `FileAnnounce` adds file to the list — unit test on state
- [x] Download button emits `RequestDownload` — unit test
- [x] Progress updates as chunks arrive — unit test on state
- [x] Completed files show as "Downloaded" — unit test on state
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
