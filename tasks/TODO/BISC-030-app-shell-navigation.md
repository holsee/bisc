# BISC-030: Iced Application Shell & Navigation

**Phase**: 6 — UI & Settings
**Depends on**: BISC-026, BISC-027, BISC-028, BISC-029

## Description

Wire all screens together into the main Iced application with navigation and the top-level Elm state machine.

## Deliverables

- `bisc-app/src/main.rs` — Iced `Application` implementation:
  - `Bisc` struct (the top-level application state from IMPLEMENTATION.md)
  - `enum Screen { Channel, Call, Settings }` — navigation state
  - `update()` dispatches `Message`s to the appropriate handler
  - `view()` renders the current screen
  - `subscription()` wires up async tasks: gossip events, media pipelines, heartbeat timer
  - Tokio runtime integration for async iroh/gossip operations
- Navigation: Channel screen -> Call screen (on join) -> back to Channel (on leave). Settings accessible from both screens.

## Acceptance Criteria

- [ ] Application launches and shows the channel screen — integration test
- [ ] Creating a channel transitions to the call screen — functional requirement
- [ ] Leaving a channel transitions back to the channel screen — functional requirement
- [ ] Settings screen is accessible and changes persist — functional requirement
- [ ] Application shuts down cleanly (no hanging threads, no panics) — integration test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
