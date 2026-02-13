# BISC-026: Iced UI — Channel Screen (Create & Join)

**Phase**: 6 — UI & Settings
**Depends on**: BISC-004, BISC-025

## Description

The initial screen: enter a display name, create a new channel, or join an existing one by pasting a ticket.

## Deliverables

- `bisc-ui/src/screens/channel.rs`:
  - `ChannelScreen` — Iced view with:
    - Text input for display name
    - "Create Channel" button — creates channel, displays ticket (with copy button)
    - Text input for ticket + "Join" button — joins channel via pasted ticket
    - Error display (invalid ticket, connection failed)
  - Emits messages: `Message::CreateChannel`, `Message::JoinChannel(String)`
  - Transitions to call screen on successful join

## Acceptance Criteria

- [ ] Screen renders without panic — integration test
- [ ] Create button triggers `Message::CreateChannel` — unit test on message handling
- [ ] Join with empty ticket shows validation error — unit test
- [ ] Join with invalid ticket shows error message — unit test
- [ ] Ticket is displayed and copyable after channel creation — functional requirement
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
