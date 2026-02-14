# BISC-027: Iced UI — Call View (Peer List, Controls, Video Grid)

**Phase**: 6 — UI & Settings
**Depends on**: BISC-017, BISC-026

## Description

The main call screen showing connected peers, video grid, and media controls.

## Deliverables

- `bisc-ui/src/screens/call.rs`:
  - `CallScreen` — Iced view with:
    - `VideoGrid` showing remote video streams
    - Peer list sidebar (names, audio/video/sharing status indicators)
    - Control bar: mute/unmute mic, enable/disable camera, share screen button, share file button, leave channel button
    - Ticket display/copy for inviting more peers
  - Emits messages: `ToggleCamera`, `ToggleMic`, `StartScreenShare`, `ShareFile`, `LeaveChannel`
  - Updates reactively when peers join/leave or media state changes

## Acceptance Criteria

- [x] Screen renders without panic with 0 peers — integration test
- [x] Peer list updates when `PeerAnnounced` / `PeerLeft` messages arrive — unit test on state
- [x] Mute button toggles mic state — unit test on message handling
- [x] Camera button toggles video state — unit test on message handling
- [x] Leave button triggers `LeaveChannel` — unit test
- [x] Video grid shows correct layout for 1, 2, 4 peers — functional requirement
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
