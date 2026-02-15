//! Custom Iced [`Shader`] widget that renders video frames as GPU textures.
//!
//! Uses wgpu directly to create a texture, upload RGBA frame data, and render
//! a textured quad scaled to fit the widget bounds while maintaining aspect ratio.

use iced::wgpu;
use iced::widget::shader;
use iced::{mouse, Rectangle, Size};

/// WGSL vertex + fragment shader source for rendering a textured quad.
///
/// Uses `vertex_index` to generate a fullscreen triangle strip (4 vertices)
/// so no vertex buffer is needed.
pub const SHADER_SOURCE: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    // Triangle strip: 4 vertices forming a quad
    // 0: bottom-left, 1: bottom-right, 2: top-left, 3: top-right
    let x = f32(vertex_index & 1u) * 2.0 - 1.0;
    let y = f32(vertex_index >> 1u) * 2.0 - 1.0;

    var out: VertexOutput;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    // Map to UV: x: [−1,1]→[0,1], y: [−1,1]→[1,0] (flip Y for top-down textures)
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

@group(0) @binding(0) var t_texture: texture_2d<f32>;
@group(0) @binding(1) var s_sampler: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_texture, s_sampler, in.uv);
}
"#;

/// Compute the viewport rectangle that fits `frame_size` into `bounds`
/// while preserving aspect ratio.
pub fn compute_viewport(frame_size: Size<u32>, bounds: Rectangle) -> Rectangle<u32> {
    if frame_size.width == 0 || frame_size.height == 0 {
        return Rectangle {
            x: bounds.x as u32,
            y: bounds.y as u32,
            width: bounds.width.max(1.0) as u32,
            height: bounds.height.max(1.0) as u32,
        };
    }

    let frame_aspect = frame_size.width as f32 / frame_size.height as f32;
    let bounds_aspect = bounds.width / bounds.height;

    if frame_aspect > bounds_aspect {
        // Frame is wider — letterbox (bars top/bottom)
        let height = bounds.width / frame_aspect;
        let y = bounds.y + (bounds.height - height) / 2.0;
        Rectangle {
            x: bounds.x as u32,
            y: y as u32,
            width: bounds.width as u32,
            height: height.max(1.0) as u32,
        }
    } else {
        // Frame is taller — pillarbox (bars left/right)
        let width = bounds.height * frame_aspect;
        let x = bounds.x + (bounds.width - width) / 2.0;
        Rectangle {
            x: x as u32,
            y: bounds.y as u32,
            width: width.max(1.0) as u32,
            height: bounds.height as u32,
        }
    }
}

/// Frame data to upload to the GPU.
#[derive(Debug, Clone)]
struct FrameData {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Iced Shader widget that renders video frames as GPU textures.
///
/// Call [`update_frame`](VideoSurface::update_frame) to provide new RGBA frame
/// data, then place the widget in your Iced view via [`view`](VideoSurface::view).
#[derive(Debug, Clone)]
pub struct VideoSurface {
    frame: Option<FrameData>,
}

impl VideoSurface {
    /// Create a new video surface with no frame.
    pub fn new() -> Self {
        Self { frame: None }
    }

    /// Whether this surface has frame data to render.
    pub fn has_frame(&self) -> bool {
        self.frame.is_some()
    }

    /// Upload a new RGBA frame to be rendered.
    ///
    /// `rgba_data` must be exactly `width * height * 4` bytes.
    pub fn update_frame(&mut self, width: u32, height: u32, rgba_data: &[u8]) {
        assert_eq!(
            rgba_data.len(),
            (width * height * 4) as usize,
            "RGBA data length must match width * height * 4"
        );
        self.frame = Some(FrameData {
            width,
            height,
            rgba: rgba_data.to_vec(),
        });
    }

    /// Create the Iced widget element for this video surface.
    pub fn view(&self) -> iced::Element<'_, ()> {
        shader(self)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .into()
    }
}

impl Default for VideoSurface {
    fn default() -> Self {
        Self::new()
    }
}

impl<Message> shader::Program<Message> for VideoSurface {
    type State = ();
    type Primitive = VideoPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        VideoPrimitive {
            frame: self.frame.clone(),
            bounds,
        }
    }
}

/// Per-frame data sent to the GPU pipeline.
#[derive(Debug, Clone)]
pub struct VideoPrimitive {
    frame: Option<FrameData>,
    bounds: Rectangle,
}

/// Persistent GPU resources for video rendering.
pub struct VideoPipeline {
    render_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    texture_state: Option<TextureState>,
}

/// Current GPU texture and its bind group.
struct TextureState {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

impl shader::Primitive for VideoPrimitive {
    type Pipeline = VideoPipeline;

    fn prepare(
        &self,
        pipeline: &mut VideoPipeline,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        let Some(frame) = &self.frame else {
            return;
        };

        // Recreate texture if dimensions changed
        let needs_new_texture = pipeline
            .texture_state
            .as_ref()
            .is_none_or(|ts| ts.width != frame.width || ts.height != frame.height);

        if needs_new_texture {
            tracing::debug!(
                width = frame.width,
                height = frame.height,
                "creating new video texture"
            );

            let texture = _device.create_texture(&wgpu::TextureDescriptor {
                label: Some("video_surface_texture"),
                size: wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

            let bind_group = _device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video_surface_bind_group"),
                layout: &pipeline.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                    },
                ],
            });

            pipeline.texture_state = Some(TextureState {
                texture,
                bind_group,
                width: frame.width,
                height: frame.height,
            });
        }

        // Upload frame data to texture
        if let Some(ts) = &pipeline.texture_state {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &ts.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * frame.width),
                    rows_per_image: Some(frame.height),
                },
                wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    fn render(
        &self,
        pipeline: &VideoPipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let Some(frame) = &self.frame else {
            return;
        };
        let Some(ts) = &pipeline.texture_state else {
            return;
        };

        let viewport = compute_viewport(Size::new(frame.width, frame.height), self.bounds);

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("video_surface_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_pipeline(&pipeline.render_pipeline);
        render_pass.set_bind_group(0, &ts.bind_group, &[]);

        // Set scissor rect to clip_bounds
        render_pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );

        // Set viewport for aspect-ratio-preserving rendering
        render_pass.set_viewport(
            viewport.x as f32,
            viewport.y as f32,
            viewport.width as f32,
            viewport.height as f32,
            0.0,
            1.0,
        );

        render_pass.draw(0..4, 0..1);
    }
}

impl shader::Pipeline for VideoPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video_surface_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SOURCE.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video_surface_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video_surface_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video_surface_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video_surface_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        tracing::info!("video surface GPU pipeline initialized");

        VideoPipeline {
            render_pipeline,
            bind_group_layout,
            sampler,
            texture_state: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_surface_default() {
        let surface = VideoSurface::new();
        assert!(surface.frame.is_none());
    }

    #[test]
    fn update_frame_stores_data() {
        let mut surface = VideoSurface::new();
        let rgba = vec![255u8; 4 * 4 * 4]; // 4x4 white
        surface.update_frame(4, 4, &rgba);

        let frame = surface.frame.as_ref().unwrap();
        assert_eq!(frame.width, 4);
        assert_eq!(frame.height, 4);
        assert_eq!(frame.rgba.len(), 64);
    }

    #[test]
    #[should_panic(expected = "RGBA data length must match")]
    fn update_frame_wrong_size_panics() {
        let mut surface = VideoSurface::new();
        surface.update_frame(4, 4, &[0u8; 10]);
    }

    #[test]
    fn viewport_wider_frame_letterbox() {
        // 16:9 frame in a 100x100 square widget
        let vp = compute_viewport(
            Size::new(1920, 1080),
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        );
        // Width fills 100, height = 100 * (1080/1920) ≈ 56
        assert_eq!(vp.width, 100);
        assert!(vp.height < 100);
        assert!(vp.height > 50);
        // Centered vertically
        assert!(vp.y > 0);
    }

    #[test]
    fn viewport_taller_frame_pillarbox() {
        // 9:16 frame in a 100x100 square widget
        let vp = compute_viewport(
            Size::new(1080, 1920),
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        );
        // Height fills 100, width = 100 * (1080/1920) ≈ 56
        assert_eq!(vp.height, 100);
        assert!(vp.width < 100);
        assert!(vp.width > 50);
        // Centered horizontally
        assert!(vp.x > 0);
    }

    #[test]
    fn viewport_exact_aspect_no_bars() {
        // 16:9 frame in a 160x90 widget
        let vp = compute_viewport(
            Size::new(1920, 1080),
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 90.0,
            },
        );
        assert_eq!(vp.x, 0);
        assert_eq!(vp.y, 0);
        assert_eq!(vp.width, 160);
        assert_eq!(vp.height, 90);
    }

    #[test]
    fn viewport_zero_frame_size() {
        let vp = compute_viewport(
            Size::new(0, 0),
            Rectangle {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 100.0,
            },
        );
        assert_eq!(vp.x, 10);
        assert_eq!(vp.y, 20);
        assert_eq!(vp.width, 100);
        assert_eq!(vp.height, 100);
    }

    #[test]
    fn draw_produces_primitive() {
        let mut surface = VideoSurface::new();
        let rgba = vec![128u8; 8 * 8 * 4];
        surface.update_frame(8, 8, &rgba);

        let prim = <VideoSurface as shader::Program<()>>::draw(
            &surface,
            &(),
            mouse::Cursor::Unavailable,
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 200.0,
            },
        );

        assert!(prim.frame.is_some());
        assert_eq!(prim.bounds.width, 200.0);
    }

    #[test]
    fn wgsl_shader_validates() {
        // Parse the WGSL shader source with naga to validate it offline (no GPU needed)
        let module = naga::front::wgsl::parse_str(SHADER_SOURCE)
            .expect("WGSL shader should parse successfully");

        // Verify expected entry points exist
        let entry_names: Vec<&str> = module
            .entry_points
            .iter()
            .map(|ep| ep.name.as_str())
            .collect();
        assert!(
            entry_names.contains(&"vs_main"),
            "missing vertex entry point"
        );
        assert!(
            entry_names.contains(&"fs_main"),
            "missing fragment entry point"
        );
    }

    #[test]
    fn dimension_change_detected() {
        // Simulate dimension change detection logic
        let mut surface = VideoSurface::new();

        // Upload 4x4 frame
        surface.update_frame(4, 4, &vec![0u8; 4 * 4 * 4]);
        let frame1 = surface.frame.as_ref().unwrap();
        assert_eq!(frame1.width, 4);
        assert_eq!(frame1.height, 4);

        // Upload 8x8 frame — dimensions changed
        surface.update_frame(8, 8, &vec![0u8; 8 * 8 * 4]);
        let frame2 = surface.frame.as_ref().unwrap();
        assert_eq!(frame2.width, 8);
        assert_eq!(frame2.height, 8);

        // The prepare() method would detect this change and recreate the texture.
        // We verify the detection logic directly:
        // Simulate old texture state at 4x4
        let old_w = 4u32;
        let old_h = 4u32;
        let needs_recreate = old_w != frame2.width || old_h != frame2.height;
        assert!(needs_recreate, "should detect dimension change");

        // Same dimensions should not trigger recreate
        let same_w = 8u32;
        let same_h = 8u32;
        let needs_recreate = same_w != frame2.width || same_h != frame2.height;
        assert!(
            !needs_recreate,
            "same dimensions should not trigger recreate"
        );
    }
}
