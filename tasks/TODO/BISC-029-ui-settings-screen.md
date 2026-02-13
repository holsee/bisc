# BISC-029: Iced UI — Settings Screen

**Phase**: 6 — UI & Settings
**Depends on**: BISC-025, BISC-006

## Description

Settings screen for configuring preferences.

## Deliverables

- `bisc-ui/src/screens/settings.rs`:
  - `SettingsScreen` — Iced view with:
    - Display name text input
    - Storage directory path display + "Browse" button (native file dialog via `rfd`)
    - Audio input device dropdown (from `list_input_devices()`)
    - Audio output device dropdown (from `list_output_devices()`)
    - Video quality selector (Low / Medium / High)
    - Audio quality selector (Low / Medium / High)
    - Theme toggle (Light / Dark)
    - "Save" button — persists settings
  - Emits messages for each setting change

## Acceptance Criteria

- [ ] Screen renders with current settings loaded — integration test
- [ ] Changing display name and saving persists the change — unit test
- [ ] Audio device dropdowns populate from available devices — functional requirement
- [ ] Quality selectors update the settings — unit test on state
- [ ] Save button writes settings to disk — unit test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
