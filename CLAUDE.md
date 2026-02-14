# bisc

A fully decentralized, native cross-platform desktop application for voice/video calls, screen sharing, and file sharing. Pure Rust — Iced GUI + iroh P2P networking. Single self-contained binary, no central server.

See `IMPLEMENTATION.md` for full architecture, library choices, and design rationale.
See `README` for user-facing requirements.

---

## Development Workflow (Ralph Loop)

Work is organised as numbered task files in `tasks/`. Each task is a self-contained work item with acceptance criteria.

```
tasks/
  TODO/    <- work items to be implemented
  DONE/    <- completed work items
```

### 1. Pick the Next Task

- List files in `tasks/TODO/` sorted by number (BISC-001, BISC-002, ...)
- Read the lowest-numbered task file
- Check its **Depends on** field — all dependencies must be in `tasks/DONE/`
- If dependencies are not met, skip to the next task whose dependencies are satisfied
- Read the full task file to understand scope, deliverables, and acceptance criteria

### 2. Implement

- Read `IMPLEMENTATION.md` for architectural context relevant to the task
- Read existing code in the workspace to understand current state and patterns
- Implement the deliverables listed in the task file
- Follow the coding conventions below
- Keep changes focused — only implement what the task specifies

### 3. Validate (all must pass)

Run these checks in order. Fix any failures before proceeding.

```bash
cargo build --workspace                                  # must compile
cargo fmt --all --check                                  # must be formatted
cargo clippy --workspace -- -D warnings                  # no warnings
RUST_LOG=debug cargo test --workspace -- --nocapture 2>&1 | tee test.log  # all tests pass, logs captured
```

After tests pass, review `test.log` for:
- Any `WARN` or `ERROR` level tracing output that indicates a problem even if tests passed
- Unexpected panics or stack traces in background tasks
- Timeout warnings or retry loops that suggest flaky behaviour

Delete `test.log` after review. Do not commit it.

### 4. Verify Acceptance Criteria

- Re-read the task file's **Acceptance Criteria** section
- Confirm every checkbox item is satisfied
- **Tick each checkbox** in the task file (change `- [ ]` to `- [x]`) as each criterion is verified
- If a criterion requires an integration test, ensure the test exists and passes
- If a criterion is a functional requirement (not testable in CI), verify it manually or document how it was verified

### 5. Complete the Task

Move the task file from `TODO/` to `DONE/`:

```bash
mv tasks/TODO/BISC-XXX-*.md tasks/DONE/
```

Commit the work with a message referencing the task:

```bash
git add -A
git commit -m "BISC-XXX: <task title>"
```

### 6. Record Learnings (if applicable)

If you discovered something non-obvious during implementation, record it. See the **Learnings** section below.

### 7. Loop

Return to step 1 and pick the next task.

---

## Coding Conventions

### Rust Style

- Edition 2021
- Format with `rustfmt` (project `rustfmt.toml` rules)
- Zero clippy warnings (`-D warnings`)
- Use `anyhow::Result` for application-level errors, `thiserror` for library-level error types
- Use `tracing` for logging (not `println!` or `log`)
- Async runtime: `tokio` (multi-threaded)
- Serialization: `serde` + `postcard` for wire formats, `serde` + `toml` for config files

### Logging

Use `tracing` exclusively — never `println!`, `eprintln!`, `dbg!`, or the `log` crate.

#### Log Levels

| Level | Use for |
|---|---|
| `error!` | Unrecoverable failures: connection lost, codec init failed, file corruption |
| `warn!` | Recoverable problems: packet dropped, peer unreachable (retrying), decode error on single frame |
| `info!` | Lifecycle events: peer joined/left, channel created/joined, pipeline started/stopped, file transfer complete |
| `debug!` | Per-operation detail: packet sent/received, frame encoded/decoded, chunk transferred, negotiation steps |
| `trace!` | High-frequency data: individual sample buffers, raw datagram contents, jitter buffer state per tick |

#### Conventions

- Every public function entry point in `bisc-net`, `bisc-media`, and `bisc-files` should have at least one `tracing` call
- Use structured fields, not string interpolation:
  ```rust
  // good
  tracing::info!(peer_id = %peer.id(), stream_id = stream_id, "video pipeline started");

  // bad
  tracing::info!("video pipeline started for peer {} stream {}", peer.id(), stream_id);
  ```
- Use `#[tracing::instrument]` on async functions that represent significant operations (connection setup, negotiation, file transfer) — but not on hot-path functions called per-packet
- Include a `target` or span for each subsystem so logs can be filtered:
  ```rust
  tracing::debug!(target: "bisc_media::jitter", packets_buffered = count, "jitter buffer state");
  ```

#### Test Setup

Every integration test should initialise a tracing subscriber. Use a shared helper:

```rust
/// Call at the start of each integration test.
/// Respects RUST_LOG env var, defaults to debug.
fn init_test_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".into()),
        )
        .with_test_writer()  // integrates with cargo test output capture
        .try_init();
}
```

This ensures that when tests are run with `RUST_LOG=debug -- --nocapture`, the agentic loop sees full runtime trace output for diagnosing failures.

### Project Structure

```
bisc-protocol/    # shared types, wire formats, ticket encoding
bisc-net/         # iroh endpoint, gossip, channels, peer connections
bisc-media/       # audio/video capture, codecs, pipelines, jitter buffer
bisc-files/       # file storage, chunked transfer, swarm downloads
bisc-ui/          # Iced widgets (video surface, grid, screens)
bisc-app/         # main binary, Iced Application, top-level state machine
```

- Shared types go in `bisc-protocol` — other crates depend on it, never the reverse
- Networking code goes in `bisc-net`
- Media processing goes in `bisc-media`
- File sharing goes in `bisc-files`
- UI widgets and screens go in `bisc-ui`
- The `bisc-app` crate wires everything together — it depends on all other crates

### Dependencies

- Prefer pure Rust crates to minimize system dependencies
- Use `rustls` not `openssl`
- Use `rusqlite` with `bundled` feature
- Use `opus` with `static` feature where possible
- Dynamically load platform-specific libraries (`pipewire`, `pulseaudio`) via `dlopen` for graceful degradation

### Testing

- Unit tests go in the same file as the code (`#[cfg(test)] mod tests`)
- Integration tests go in the crate's `tests/` directory
- Use `tokio::test` for async tests
- Use `tempfile` for tests that need filesystem state
- Tests must not depend on hardware (camera, mic) being present — skip gracefully
- Tests must not depend on network access — use localhost connections

### Runtime Observability

The agentic loop can only see stdout/stderr and test results. To validate runtime behaviour, use these techniques in all code and tests:

#### Structured Tracing in Tests

Use `tracing` throughout the codebase. In tests, attach a subscriber that captures events to a buffer, then assert on specific events:

```rust
use tracing_subscriber::layer::SubscriberExt;

#[tokio::test]
async fn test_voice_pipeline_sends_packets() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let layer = TracingCapture::new(tx); // captures events
    let _guard = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(layer),
    );

    run_pipeline().await;

    let events: Vec<_> = rx.try_iter().collect();
    assert!(events.iter().any(|e| e.contains("opus_frame_encoded")));
    assert!(events.iter().any(|e| e.contains("datagram_sent")));
}
```

Add `tracing::info!`, `tracing::debug!`, and `tracing::warn!` at key runtime points: connection established, packet sent/received, frame encoded/decoded, chunk transferred, peer joined/left, error recovered.

#### Metrics Counters

Embed `AtomicU64` counters in components that handle throughput or state transitions. Expose them via a `Metrics` struct so tests can assert on runtime behaviour quantitatively:

```rust
pub struct PipelineMetrics {
    pub packets_sent: AtomicU64,
    pub packets_received: AtomicU64,
    pub packets_lost: AtomicU64,
    pub frames_encoded: AtomicU64,
    pub frames_decoded: AtomicU64,
    pub keyframes_requested: AtomicU64,
    pub bytes_transferred: AtomicU64,
}
```

Tests then verify:

```rust
assert!(pipeline.metrics().packets_sent.load(Ordering::Relaxed) > 100);
assert!(pipeline.metrics().packets_lost.load(Ordering::Relaxed) < 5);
```

Every pipeline, transport, and protocol handler should expose metrics. This is the primary way the agentic loop verifies that runtime behaviour is correct.

#### Snapshot Testing

Use `insta` for complex outputs: negotiation flows, packet sequences, state machine transitions, protocol message dumps. The agent sees the full diff when a snapshot changes, making regressions immediately visible.

#### Simulation Harness (`bisc-net::sim`)

Once BISC-034 is complete, a `SimNetwork` harness allows fully deterministic multi-peer tests with controlled network conditions:

```rust
let mut sim = SimNetwork::new();
sim.set_latency(Duration::from_millis(50));
sim.set_loss_rate(0.05);

let (peer_a, peer_b) = sim.create_channel().await;
sim.advance(Duration::from_secs(5)).await;

assert!(peer_a.metrics().packets_sent.load(Relaxed) > 200);
assert!(peer_b.metrics().audio_jitter_ms.load(Relaxed) < 100);
```

Use the sim harness for all integration tests that involve multi-peer scenarios, failure modes, network degradation, and timing-sensitive behaviour. It runs entirely in-process with no real sockets, making tests fast and deterministic.

### Commits

- One commit per completed task
- Message format: `BISC-XXX: <task title>`
- All code in a commit must pass the full validation suite

---

## Learnings

When you discover something non-obvious during development, record it so future sessions benefit. This includes:

- API quirks, undocumented behaviour, or workarounds for specific crates
- Platform-specific gotchas (e.g. "PipeWire requires X to capture app audio")
- Architectural decisions made during implementation that aren't in IMPLEMENTATION.md
- Performance findings (e.g. "wgpu texture upload is faster with BGRA than RGBA")
- Patterns that worked well or patterns that failed

### Structure

```
learnings/
  index.md                              # table of contents linking to all learnings
  iroh/
    gossip-topic-bootstrap.md           # example: how gossip bootstrap actually works
  iced/
    custom-shader-widget-lifecycle.md   # example: wgpu resource management in Iced
  media/
    opus-frame-sizing.md                # example: Opus frame size constraints
  platform/
    linux-pipewire-app-audio.md         # example: PipeWire scoped audio capture
```

### When to Write a Learning

Write a learning when you:
- Spend significant time debugging something that wasn't obvious from docs
- Discover that a crate's API works differently than expected
- Find a platform-specific behaviour that affects the implementation
- Make an architectural decision not covered by IMPLEMENTATION.md
- Find a performance-relevant insight

### Format

Each learning file:

```markdown
# <Title>

**Task**: BISC-XXX
**Date**: YYYY-MM-DD
**Crate/Area**: <relevant crate or area>

## Context

<What you were trying to do>

## Discovery

<What you learned — the non-obvious thing>

## Impact

<How this affects the implementation or future tasks>
```

### Maintaining the Index

After writing a learning, update `learnings/index.md` to include a link:

```markdown
- [<Title>](<category>/<filename>.md) — <one-line summary>
```

Group entries by category (matching the subdirectory).

---

## Key Reference Files

| File | Purpose |
|---|---|
| `README` | User-facing requirements |
| `IMPLEMENTATION.md` | Architecture, library choices, design rationale |
| `tasks/TODO/BISC-XXX-*.md` | Work items to implement |
| `tasks/DONE/BISC-XXX-*.md` | Completed work items |
| `learnings/index.md` | Index of all learnings |
| `learnings/<category>/<name>.md` | Individual learning entries |
