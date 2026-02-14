# BISC-036.1: Fix — Wire Iced Application Entrypoint

**Type**: Bug fix
**Origin**: BISC-030 (Iced Application Shell & Navigation)
**Parent**: BISC-036 (Application Lifecycle & Networking Integration)
**Depends on**: BISC-030 (DONE)

## Problem

`cargo run` logs "bisc starting" and immediately exits — no window opens.

`bisc-app/src/main.rs` initializes tracing and returns `Ok(())`. The Iced `Application` was never wired up. The `app.rs` module defining `App`, `AppMessage`, `AppAction`, and `Screen` exists but is not declared as a module in `main.rs`, so it is never compiled into the binary.

## Deliverables

1. Add `mod app;` and `iced` dependency so `app.rs` is compiled
2. Rewrite `main.rs` to launch Iced via `iced::application()`
3. Implement `BiscApp` wrapper that:
   - Initializes `App::default()` and loads `Settings`
   - Delegates `update()` to `App::update()` and dispatches `AppAction`
   - Delegates `view()` to the appropriate screen based on `App::screen`
   - Returns `iced::Theme::Dark`
4. Wire `CopyToClipboard` → `iced::clipboard::write()`
5. Wire `SaveSettings` → `Settings::save()`
6. Stub remaining `AppAction` variants with `tracing::info!` logs

## Acceptance Criteria

- [x] `cargo build --workspace` succeeds
- [x] `cargo run --bin bisc` opens a window showing the channel screen
- [x] Channel screen renders: title, display name input, create/join buttons
- [x] Settings screen accessible (via future navigation wiring)
- [x] All existing tests pass
- [x] No clippy warnings
