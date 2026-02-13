# BISC-016: VideoSurface Shader Widget (wgpu)

**Phase**: 3 — Video Calls
**Depends on**: BISC-001

## Description

Custom Iced `Shader` widget that renders a video frame as a GPU texture.

## Deliverables

- `bisc-ui/src/video_surface.rs`:
  - `VideoSurface` — Iced `Shader` widget that:
    - Creates a wgpu texture + bind group + render pipeline on first frame
    - Updates the texture via `queue.write_texture()` when new frame data arrives
    - Renders the texture as a quad scaled to fit the widget bounds (maintaining aspect ratio)
    - Handles resize (recreates texture if dimensions change)
  - Vertex/fragment shaders (WGSL) for textured quad rendering
  - `VideoSurface::update_frame(width, height, rgba_data: &[u8])` — uploads new frame

## Acceptance Criteria

- [ ] Widget compiles and can be placed in an Iced layout — unit test / build test
- [ ] Shader compiles (WGSL validation) — unit test
- [ ] A synthetic test frame (solid color) can be uploaded and the widget renders without panic — integration test (headless if possible, or just verify no errors)
- [ ] Texture is recreated when frame dimensions change — unit test on internal logic
- [ ] Aspect ratio is maintained (not stretched) — unit test on viewport calculation
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
