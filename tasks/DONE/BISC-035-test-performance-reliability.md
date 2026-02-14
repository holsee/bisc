# BISC-035: Test Performance & Reliability — Self-Contained Test Infrastructure

**Phase**: 7 — Packaging & Hardening
**Depends on**: BISC-034

## Problem Statement

Tests are unreliable and slow. Root causes:

1. **External relay dependency**: `BiscEndpoint::new()` calls `Endpoint::builder().alpns(...).bind()` with default relay configuration, which connects every test endpoint to iroh's public relay servers (`https://relay.iroh.network` or similar). This adds latency to endpoint creation, can fail when relays are unreachable, and is fundamentally non-deterministic.

2. **Excessive timeouts masking slowness**: Tests use 20-second timeouts for peer discovery, 25 seconds for heartbeat detection, 10 seconds for negotiation. These are set high to accommodate relay-dependent variance, but they also hide genuine performance issues and make the test suite slow even when things work (a failing test wastes the full timeout duration before reporting failure).

3. **No timing instrumentation**: There is no visibility into where time is actually spent during tests — is it endpoint creation? Relay connection? Gossip bootstrap? QUIC handshake? Without measurement, optimization is guesswork.

4. **SimNetwork exists but is unused in integration tests**: BISC-034 delivered a comprehensive `SimNetwork` harness with deterministic latency/loss/jitter simulation, but all integration tests (bisc-net/tests/, bisc-app/tests/) still use real iroh endpoints on localhost hitting external relays.

5. **Duplicated test helpers**: `init_test_tracing()`, `setup_peer()`, `wait_for_peers()`, and `wait_for_connection()` are copy-pasted across bisc-net/tests/ and bisc-app/tests/integration/helpers.rs with slight variations.

## Deliverables

### 1. Timing Instrumentation Layer

Add a `TestTimer` utility to measure and report where time is spent in tests:

- `bisc-net/src/testing.rs` (behind `#[cfg(test)]` or a `test-util` feature):
  - `TestTimer` — wraps phases of test setup with `tracing::info!` timing spans
  - Instrument: endpoint creation, gossip subscription, peer discovery, connection establishment, media negotiation, pipeline start
  - Each phase logs wall-clock duration on completion
  - Summary report at test end showing cumulative time per phase

Example output:
```
[2.3s] endpoint_create: 450ms
[2.8s] gossip_subscribe: 320ms
[3.1s] peer_discovery: 1200ms
[3.2s] connection_setup: 85ms
[3.2s] negotiation: 40ms
TOTAL: 2095ms (of 3200ms wall clock)
```

### 2. Local Relay Server for Tests

Implement or integrate a self-contained iroh relay for test use:

- `bisc-net/src/testing.rs` or `bisc-net/src/test_relay.rs`:
  - `TestRelay` — spins up a local `iroh-relay` server on localhost (ephemeral port)
  - Provides the relay URL for endpoint configuration
  - Starts/stops with the test lifecycle
  - Zero external network dependency
- Modify `BiscEndpoint` to accept configuration:
  - `BiscEndpoint::new()` — production default (keeps current behaviour)
  - `BiscEndpoint::with_config(EndpointConfig)` — accepts relay URL override, discovery mode, etc.
  - `BiscEndpoint::for_testing()` — convenience constructor: local relay only, no DNS discovery, no external network
- Investigate `iroh::endpoint::Builder::relay_mode(RelayMode::Disabled)` as a simpler alternative if tests work without relay entirely (localhost direct connections may not need relay at all)

### 3. Optimised Test Helpers (Shared Crate or Module)

Consolidate and improve test helpers:

- Single canonical location for test helpers (either `bisc-net/src/testing.rs` with `test-util` feature, or a shared `bisc-test-util` crate)
- `init_test_tracing()` — single implementation, used everywhere
- `setup_peer()` / `setup_peer_with_media()` — uses `BiscEndpoint::for_testing()` instead of `BiscEndpoint::new()`
- `wait_for_peers()` — add timing instrumentation, adaptive timeout (fast-fail if no progress)
- `wait_for_connection()` — add timing instrumentation
- Remove duplicated helpers from bisc-net/tests/ files
- All bisc-app/tests/ and bisc-net/tests/ import from the shared location

### 4. Timeout Tuning

With external relay removed and timing instrumented:

- Profile actual test timings with local-only endpoints
- Reduce peer discovery timeout from 20s to an appropriate value (likely 3-5s for localhost)
- Reduce connection timeout from 20s to an appropriate value
- Reduce heartbeat detection timeout from 25s proportionally if heartbeat interval is configurable
- Add per-test `#[tokio::test(start_paused = true)]` where SimNetwork is used (instant deterministic time)
- Keep a safety margin (2-3x observed p99) but not the current 10-20x margins

### 5. Integration Test Migration to SimNetwork

For tests that don't specifically need real iroh networking:

- Identify which integration tests can use `SimNetwork` instead of real endpoints
- Migrate applicable tests (media transport, file transfer, reconnection scenarios)
- Keep a small set of "smoke tests" using real local iroh endpoints to catch iroh-level regressions
- SimNetwork tests should complete in <1 second each (no real I/O, deterministic time)

### 6. CI-Friendly Test Runner Configuration

- Add a `tests/run_tests.sh` or equivalent that:
  - Sets `RUST_LOG=info` (not debug — reduce log noise for CI)
  - Captures timing per test binary
  - Reports slowest tests
  - Fails fast on timeout rather than waiting full duration
- Document expected test suite total runtime in this task file after optimization

## Acceptance Criteria

- [x] `TestTimer` instrumentation exists and every integration test logs phase timings
- [x] Tests do NOT connect to external relay servers — `RelayMode::Disabled` with `MemoryLookup` for address resolution
- [x] Either a local `TestRelay` is available OR relay is disabled for tests — relay disabled, `MemoryLookup` used for direct address exchange
- [x] `BiscEndpoint::for_testing()` (or equivalent) exists and is used by all test helpers
- [x] Test helpers are consolidated in one location — `bisc-net/src/testing.rs` behind `test-util` feature, re-exported by all test crates
- [x] Peer discovery timeout reduced from 20s to ≤5s (measured: peer discovery takes <300ms on localhost)
- [x] At least 2 integration tests migrated to use `SimNetwork` with `start_paused = true` — `sim_file_transfer_over_reliable_stream` and `sim_disconnect_reconnect_with_latency` in `bisc-net/tests/sim_integration.rs`
- [x] Full `cargo test --workspace` completes in under 60 seconds (excluding compile time) on localhost — measured ~30s total
- [x] No flaky test failures over 5 consecutive runs: `for i in $(seq 5); do cargo test --workspace || exit 1; done` — verified 5/5 clean passes
- [x] Timing summary logged at end of each integration test showing phase breakdown
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo fmt --all --check` passes

## Investigation Notes

### iroh Relay Situation

`Endpoint::builder().bind()` with no configuration uses iroh's default relay map, which includes public relay servers. Options:

1. **`RelayMode::Disabled`**: Simplest. Tests on localhost may work without relay since QUIC can connect directly. Risk: if iroh uses relay for initial address discovery even on localhost, this breaks.
2. **Local relay**: `iroh-relay` crate provides a server binary. We could embed a lightweight version. More complex but guarantees relay-dependent features work in tests.
3. **Custom relay map**: Point endpoints at a local relay using `RelayMode::Custom(RelayMap)`. Middle ground.

**Recommendation**: Start with `RelayMode::Disabled` — if tests pass, this is the simplest fix and eliminates the external dependency entirely. If some tests require relay (e.g., for address exchange), add a local relay.

### SimNetwork vs Real Endpoints

Not all tests should use SimNetwork:

| Test Type | Transport | Rationale |
|-----------|-----------|-----------|
| Channel lifecycle (gossip) | Real local iroh | Tests actual gossip protocol |
| Peer connection | Real local iroh | Tests actual QUIC connections |
| Media transport (datagram flow) | SimNetwork | Only needs Transport trait |
| File transfer | SimNetwork | Only needs Transport trait |
| Reconnection | SimNetwork | Needs disconnect/reconnect simulation |
| Voice/video pipeline | SimNetwork | Tests codec + pipeline, not network |
| Heartbeat timeout | SimNetwork | Deterministic time control needed |

### Estimated Impact

| Metric | Before | Target |
|--------|--------|--------|
| Single test (peer discovery) | 3-20s | <2s (local) or <100ms (sim) |
| Single test (heartbeat timeout) | 25s | <1s (sim with paused time) |
| Full workspace test suite | 2-5 minutes | <60 seconds |
| Flaky failure rate | Unknown | 0% over 5 runs |
| External network dependency | Yes (relay servers) | None |
