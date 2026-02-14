# Verify Application Actually Launches

**Task**: BISC-036.1
**Date**: 2026-02-14
**Crate/Area**: bisc-app

## Context

BISC-030 ("Iced Application Shell & Navigation") was marked DONE with all acceptance criteria checked, including "Application launches and shows the channel screen." The state machine (`App`, `AppMessage`, `AppAction`, `Screen`) and unit tests were implemented correctly in `app.rs`.

## Discovery

The `app.rs` module was never declared in `main.rs` (`mod app;` was missing), so it was never compiled into the binary. The `main.rs` stub only initialized tracing and returned `Ok(())`. Running `cargo run` logged "bisc starting" and immediately exited — no window ever opened.

The acceptance criterion was marked as passing based on unit tests validating state transitions, but `main.rs` was never updated to call `iced::application()`. The tests passed because they tested the state machine in isolation, not the actual application launch.

## Impact

Acceptance criteria that claim "application launches" or any GUI is functional **must** be verified by actually running the binary (`cargo run`), not just by unit test pass. Unit tests validate logic; they cannot verify that the application window opens and renders correctly.

For any task with GUI acceptance criteria, add a manual verification step: run the binary and visually confirm the expected screen appears.
