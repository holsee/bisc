# BISC-017: Video Grid Layout

**Phase**: 3 — Video Calls
**Depends on**: BISC-016

## Description

Arrange multiple `VideoSurface` widgets in a responsive grid based on peer count.

## Deliverables

- `bisc-ui/src/video_grid.rs`:
  - `VideoGrid` — Iced widget/layout that arranges N video surfaces:
    - 1 peer: full area
    - 2 peers: side by side (50/50 split)
    - 3-4 peers: 2x2 grid
    - 5-6 peers: 2x3 or 3x2 grid
    - 7-9 peers: 3x3 grid
  - `grid_layout(count: usize, bounds: Size) -> Vec<Rectangle>` — computes the position and size of each cell
  - Handles dynamic add/remove of streams

## Acceptance Criteria

- [ ] `grid_layout(1, ...)` returns 1 rectangle filling the bounds — unit test
- [ ] `grid_layout(2, ...)` returns 2 equal rectangles side by side — unit test
- [ ] `grid_layout(4, ...)` returns 4 equal rectangles in a 2x2 grid — unit test
- [ ] `grid_layout(5, ...)` arranges in a 2x3 or 3x2 grid — unit test
- [ ] All rectangles fit within bounds (no overflow) — unit test for N=1..9
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
