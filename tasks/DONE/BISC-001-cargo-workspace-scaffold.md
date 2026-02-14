# BISC-001: Cargo Workspace Scaffold

**Phase**: 1 — Foundation & Channel Infrastructure
**Depends on**: none

## Description

Set up the multi-crate workspace with shared dependencies and build configuration.

## Deliverables

- Root `Cargo.toml` workspace with members: `bisc-app`, `bisc-net`, `bisc-media`, `bisc-files`, `bisc-protocol`
- Each crate has a `lib.rs` (or `main.rs` for `bisc-app`) with a placeholder module structure
- `bisc-app` depends on all other crates
- Shared dependencies pinned in `[workspace.dependencies]`: `serde`, `tokio`, `tracing`, `tracing-subscriber`, `anyhow`, `bytes`
- `.gitignore` for Rust projects (include `test.log`)
- `rustfmt.toml` with project formatting rules
- `clippy.toml` or workspace-level clippy config in `Cargo.toml`

## Acceptance Criteria

- [ ] `cargo build` succeeds for the entire workspace
- [ ] `cargo test` runs (even if no tests yet) without errors
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] Each crate can be built independently (`cargo build -p bisc-net`, etc.)
- [ ] `bisc-app` binary runs and exits cleanly
