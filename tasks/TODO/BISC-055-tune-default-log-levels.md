# BISC-055: Tune Default Log Levels for Usability

**Phase**: 9 — Validation & Fixes
**Depends on**: BISC-036

## Problem Statement

When running the application, the console is flooded with hundreds of lines of HLSL/WGSL shader source code logged at `INFO` level by `wgpu_hal::dx12::device`. This drowns out the actual application logs (`bisc`, `bisc_net`, `bisc_media`, etc.) making it impossible to observe application behavior during development and testing.

Similarly, other noisy subsystems like `iced_wgpu`, `iced_winit`, and `fontdb` log at levels that obscure the bisc application logs.

## Deliverables

### 1. Tune the default EnvFilter

Update the default `tracing_subscriber` filter in `main()` to suppress noisy third-party crates while keeping bisc subsystem logs visible:

```
info,wgpu_hal=warn,wgpu_core=warn,iced_wgpu=warn,iced_winit=warn,naga=warn,fontdb=warn
```

This keeps `info` for bisc crates while suppressing third-party noise. Users can still override via `RUST_LOG` environment variable.

### 2. Preserve RUST_LOG override

Ensure `EnvFilter::try_from_default_env()` is still checked first, so users can set `RUST_LOG=debug` or `RUST_LOG=wgpu_hal=info` when they need to diagnose GPU issues.

## Acceptance Criteria

- [ ] Default log output shows bisc application logs without wgpu shader dumps
- [ ] `RUST_LOG` environment variable still overrides the defaults
- [ ] `cargo build --workspace` compiles
- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
