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

- [x] Screen renders with current settings loaded — integration test
- [x] Changing display name and saving persists the change — unit test
- [x] Audio device dropdowns populate from available devices — functional requirement
- [x] Quality selectors update the settings — unit test on state
- [x] Save button writes settings to disk — unit test
- [x] `cargo clippy -- -D warnings` passes
- [x] `cargo fmt --check` passes
