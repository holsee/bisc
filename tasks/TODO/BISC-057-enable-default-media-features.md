# BISC-057: Enable Default Media Feature Flags

**Phase**: 9 — Validation & Fixes
**Depends on**: None

## Problem Statement

Running `cargo run --bin bisc` produces a binary with **no audio, video, or screen capture support**. The `bisc-app` crate defines feature flags (`audio`, `video`, `video-codec`, `screen-capture`) but sets `default = []`. This means every media subsystem silently degrades to a no-op stub:

- Voice: "audio feature not enabled, voice disabled"
- Camera: "video feature not enabled, camera disabled"
- Video codec: "video-codec feature not enabled, skipping video pipeline"
- Screen capture: "screen-capture feature not enabled, screen sharing disabled"

A user building and running the app gets a binary that can create/join channels and see peers, but **none of the core features work**.

## Deliverables

### 1. Enable all media features by default

In `bisc-app/Cargo.toml`, change:

```toml
[features]
default = []
```

to:

```toml
[features]
default = ["audio", "video", "video-codec", "screen-capture"]
```

### 2. Verify build with all features

Run `cargo build -p bisc-app` (which now includes all features) and confirm it compiles. Fix any compilation issues that arise from the newly-enabled feature code paths.

### 3. Verify features can be individually disabled

Ensure `cargo build -p bisc-app --no-default-features` still compiles — the graceful degradation stubs must remain functional.

## Acceptance Criteria

- [ ] `default = ["audio", "video", "video-codec", "screen-capture"]` in `bisc-app/Cargo.toml`
- [ ] `cargo build -p bisc-app` compiles with all features enabled
- [ ] `cargo build -p bisc-app --no-default-features` compiles without features
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo test --workspace` passes
