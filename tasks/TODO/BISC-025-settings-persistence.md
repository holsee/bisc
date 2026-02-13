# BISC-025: Settings Persistence

**Phase**: 6 — UI & Settings
**Depends on**: BISC-001

## Description

Load and save user settings (display name, storage directory, quality preferences, audio devices).

## Deliverables

- `bisc-app/src/settings.rs`:
  - `Settings` struct with fields: `display_name`, `storage_dir`, `video_quality`, `audio_quality`, `input_device`, `output_device`, `theme` (light/dark)
  - `Settings::load() -> Settings` — loads from `<config_dir>/bisc/settings.toml`, returns defaults if file doesn't exist
  - `Settings::save(&self)` — writes to TOML file
  - `Settings::default()` — sensible defaults (storage_dir = `<data_dir>/bisc`, quality = medium)
  - Dependencies: `serde`, `toml`, `directories`

## Acceptance Criteria

- [ ] `Settings::default()` produces valid settings — unit test
- [ ] `Settings::save()` + `Settings::load()` round-trip — unit test (in temp dir)
- [ ] Missing config file returns defaults without error — unit test
- [ ] Corrupted config file returns defaults without panic (log warning) — unit test
- [ ] All fields serialize/deserialize correctly — unit test
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
