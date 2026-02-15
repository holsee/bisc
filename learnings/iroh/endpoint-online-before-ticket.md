# endpoint.online() Before Ticket Generation

**Task**: BISC-053
**Date**: 2026-02-15
**Crate/Area**: iroh

## Context

When creating a channel, bisc generates the invite ticket immediately after binding the endpoint. The ticket contains the endpoint's addressing info (node ID, direct addresses, relay URLs).

## Discovery

`endpoint.online()` waits for the home relay connection to be established. Without calling it, the ticket may contain incomplete addressing info — specifically missing the relay URL. The joining peer would then need to rely on DNS discovery (slower) or direct addresses (may not work through NAT).

sendme (n0-computer/sendme) calls `endpoint.online().await` with a timeout before generating tickets.

Key details:
- `online()` internally waits for `home_relay().initialized()`
- With `RelayMode::Disabled` (testing), `online()` never resolves — a timeout is essential
- Relay connections typically establish in < 200ms, so a 500ms timeout works well
- Log at `debug` level on timeout (expected in tests) rather than `warn`

## Impact

Added `endpoint.online()` with a 500ms timeout in `Channel::create()` before ticket generation. This ensures production tickets contain relay URLs for reliable connectivity while keeping test overhead minimal.
