# BISC-054: Record Validation Session Learnings

**Phase**: 10 — Improvements
**Depends on**: BISC-049

## Problem Statement

During the first end-to-end validation session, several non-obvious lessons were discovered. Some extend existing learnings; others are entirely new. These should be captured in `learnings/` so future development sessions benefit.

**Note**: The existing learnings `iroh/router-owns-accept-loop.md` and `iced/verify-app-launches.md` already document the general patterns. The new learnings below capture what was discovered beyond those.

## Deliverables

### 1. Learning: Plumbing exists does not mean plumbing is connected (iroh/)

`MediaProtocol`, `GossipHandle::with_protocols()`, and `Channel::create(incoming_rx)` were all correctly implemented in `bisc-net`. But the app-level wiring in `main.rs` used `GossipHandle::new()` (no MediaProtocol) and passed `None` for `incoming_rx`. Unit tests passed because they tested components in isolation. Integration tests passed because they used `setup_peer_with_media()` which correctly wires everything.

**Key insight**: When test helpers correctly wire subsystems but the production code does not, tests provide false confidence. Integration tests should share the same initialization path as the real application, or explicitly test the real path.

### 2. Learning: iroh-blobs as alternative to custom file transfer (iroh/)

sendme (n0-computer/sendme) uses `iroh-blobs` for file transfer, getting BLAKE3 verified streaming, resumable downloads, content-addressed storage, and `BlobsProtocol` (a `ProtocolHandler` for the Router) out of the box. bisc reimplements all of this with SHA-256 chunking, bitfield tracking, and a hand-rolled wire protocol. `iroh-blobs` is the recommended approach for iroh-based file transfer applications.

### 3. Learning: endpoint.online() before ticket generation (iroh/)

sendme calls `endpoint.online().await` (with timeout) before generating tickets, ensuring relay connectivity is established and the ticket contains reachable addresses. Without this, tickets may contain incomplete addressing info, causing the joining peer's connection attempt to fail or require fallback to DNS discovery.

### 4. Learning: Windows wgpu backend selection (platform/)

On Windows, wgpu's automatic backend selection may pick Vulkan, which crashes with `STATUS_ACCESS_VIOLATION` on certain GPU drivers. Forcing `WGPU_BACKEND=dx12` via environment variable before Iced/wgpu initialization resolves this. DX12 is the native, most reliable backend on Windows. The env var must be set before `iced::application().run()` is called.

### 5. Update learnings index

- Add entries to `learnings/index.md` grouped by category

## Acceptance Criteria

- [x] Learning files created in appropriate subdirectories (`iroh/`, `platform/`)
- [x] No duplication with existing learnings — new files extend, not repeat
- [x] `learnings/index.md` updated with links to new entries
- [x] Each learning follows the standard format (Context, Discovery, Impact)
