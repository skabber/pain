//! Draws a pane's character grid: glyphs rasterized via `cosmic-text`, packed
//! into a GPU texture atlas, and drawn as instanced quads via `wgpu`.

mod atlas;
mod glyph;

use bytemuck::{Pod, Zeroable};

pub use glyph::{GlyphRasterizer, RasterizedGlyph, ShapedGlyph, monospace_font_families, system_ui_font_data};

/// Measures a font's cell size at `font_size_px` in `font_family` (`""` or
/// `"monospace"` for the system default): the advance width of a
/// representative glyph, and a line height of `1.25x` the font size. Both
/// are rounded to whole pixels so grid positions land on the pixel grid —
/// see the position-rounding in [`GridRenderer::render`] for why that
/// matters.
pub fn measure_cell(font_size_px: f32, font_family: &str) -> (f32, f32) {
    let mut rasterizer = GlyphRasterizer::new();
    let width = rasterizer.advance_width('M', font_size_px, font_family).unwrap_or(font_size_px * 0.6);
    (width.round(), (font_size_px * 1.25).round())
}

const QUAD_CORNERS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    corner: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    pos: [f32; 2],
    size: [f32; 2],
    uv_origin: [f32; 2],
    uv_size: [f32; 2],
    color: [f32; 4],
    /// 1.0 to sample the color atlas and use the glyph's own colors, 0.0 to
    /// sample the coverage atlas and tint by `color`. A float rather than a
    /// `u32` flag so the vertex format stays uniformly `Float32*` — there is
    /// exactly one bit of information here and nowhere to grow.
    colored: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    screen_size: [f32; 2],
}

/// One character to draw. `x`/`y` are the absolute pixel position of the
/// cell's top-left corner — callers (which know about pane layout, if any)
/// are responsible for offsetting by a pane's screen position; the renderer
/// itself has no notion of panes.
pub struct GlyphCell {
    pub x: f32,
    pub y: f32,
    pub c: char,
    pub color: [f32; 4],
}

/// A run of characters to shape and draw together, so the font can apply
/// ligatures across them. The ligature-mode counterpart to [`GlyphCell`].
///
/// `x`/`y` are the absolute pixel position of the run's first cell. Glyphs
/// within the run are then placed by the font's own advances rather than by
/// cell arithmetic — so the caller must only group cells where ligating is
/// actually correct: one color, and no cursor sitting inside the run.
pub struct GlyphRun {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub color: [f32; 4],
}

/// A solid-filled rectangle: the cursor, a divider, or any other chrome
/// drawn without a glyph. Sampled from a reserved 1x1 opaque texel in the
/// atlas, so it goes through the same instanced draw as glyphs.
pub struct SolidRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: [f32; 4],
}

/// Draws glyphs and solid rects (cursors, dividers) via one instanced pass.
pub struct GridRenderer {
    pipeline: wgpu::RenderPipeline,
    quad_vbo: wgpu::Buffer,
    instance_vbo: wgpu::Buffer,
    instance_capacity: usize,
    globals_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    atlas: atlas::GlyphAtlas,
}

impl GridRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let atlas = atlas::GlyphAtlas::new(device, queue);

        // Nearest, not linear: glyph quads are always drawn at the exact
        // pixel size they were rasterized at (see the position-rounding in
        // `render`), so there is no scaling for linear filtering to smooth —
        // only a risk of it bleeding into the next glyph packed edge-to-edge
        // in the atlas.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grid-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Color glyphs (emoji) live in their own RGBA texture — see
                // `atlas::COLOR_ATLAS_SIZE`. Both are bound for every draw
                // and the fragment shader picks per instance, rather than
                // splitting into two pipelines and two passes for what is
                // usually a handful of glyphs.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grid-bind-group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&atlas.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&atlas.color_view) },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grid-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("grid-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grid-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<QuadVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![
                            1 => Float32x2,
                            2 => Float32x2,
                            3 => Float32x2,
                            4 => Float32x2,
                            5 => Float32x4,
                            6 => Float32,
                        ],
                    },
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Premultiplied, not straight: `fs_main` outputs
                    // premultiplied color (RGB already scaled by its own
                    // effective alpha), which this blend mode expects —
                    // see the shader's own comment for why (Windows'
                    // DirectComposition swapchains, used for window
                    // transparency, only accept premultiplied content).
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let quad_vbo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quad-vbo"),
            size: (QUAD_CORNERS.len() * std::mem::size_of::<QuadVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let quad_data: Vec<QuadVertex> = QUAD_CORNERS.into_iter().map(|corner| QuadVertex { corner }).collect();
        queue.write_buffer(&quad_vbo, 0, bytemuck::cast_slice(&quad_data));

        let instance_capacity = 65536;
        let instance_vbo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance-vbo"),
            size: (instance_capacity * std::mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self { pipeline, quad_vbo, instance_vbo, instance_capacity, globals_buffer, bind_group, atlas }
    }

    /// Clears `view` to `background` and draws `rects` (cursors, dividers)
    /// followed by `glyphs`, all in absolute pixel coordinates.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        screen_size: (u32, u32),
        font_size_px: f32,
        font_family: &str,
        background: wgpu::Color,
        rects: impl Iterator<Item = SolidRect>,
        glyphs: impl Iterator<Item = GlyphCell>,
        runs: impl Iterator<Item = GlyphRun>,
    ) {
        queue.write_buffer(
            &self.globals_buffer,
            0,
            bytemuck::bytes_of(&Globals { screen_size: [screen_size.0 as f32, screen_size.1 as f32] }),
        );

        let mut instances = Vec::new();

        for rect in rects {
            instances.push(Instance {
                pos: [rect.x.round(), rect.y.round()],
                size: [rect.width, rect.height],
                uv_origin: self.atlas.solid_uv,
                uv_size: [0.0, 0.0],
                color: rect.color,
                colored: 0.0,
            });
        }

        for glyph in glyphs {
            let Some(entry) = self.atlas.entry(queue, glyph.c, font_size_px, font_family) else {
                continue;
            };
            // `top` is the offset from the baseline up to the bitmap's top edge;
            // the baseline itself sits `font_size_px` down from the cell's top.
            let pen_y = glyph.y + font_size_px;
            // Snapped to whole pixels: the atlas has no padding between glyphs
            // and the sampler sees each texel as an exact screen pixel, so any
            // fractional position bleeds neighboring glyphs' edges together.
            instances.push(Instance {
                pos: [(glyph.x + entry.left).round(), (pen_y - entry.top).round()],
                size: [entry.width, entry.height],
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
                color: glyph.color,
                colored: if entry.colored { 1.0 } else { 0.0 },
            });
        }

        for run in runs {
            // The baseline sits `font_size_px` below the run's top edge,
            // same as the per-character path.
            let pen_y = run.y + font_size_px;
            // Collected because `shape_run` borrows the atlas mutably and so
            // does `shaped_entry` — the shaped glyphs are small and there is
            // one run per contiguous stretch of same-colored cells, not one
            // per cell.
            let shaped: Vec<glyph::ShapedGlyph> = self.atlas.shape_run(&run.text, font_size_px, font_family).to_vec();

            for glyph in shaped {
                let Some(entry) = self.atlas.shaped_entry(queue, glyph, font_size_px, font_family) else {
                    continue;
                };
                instances.push(Instance {
                    pos: [(run.x + glyph.x as f32 + entry.left).round(), (pen_y + glyph.y as f32 - entry.top).round()],
                    size: [entry.width, entry.height],
                    uv_origin: entry.uv_origin,
                    uv_size: entry.uv_size,
                    color: run.color,
                    colored: if entry.colored { 1.0 } else { 0.0 },
                });
            }
        }

        if instances.len() > self.instance_capacity {
            instances.truncate(self.instance_capacity);
        }
        if !instances.is_empty() {
            queue.write_buffer(&self.instance_vbo, 0, bytemuck::cast_slice(&instances));
        }

        // `LoadOp::Clear` writes this value directly into the render
        // target — unlike every draw call, it never passes through
        // `fs_main`, so it has to already be premultiplied by hand here to
        // match everything else the pipeline now produces.
        let premultiplied_background = wgpu::Color {
            r: background.r * background.a,
            g: background.g * background.a,
            b: background.b * background.a,
            a: background.a,
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("grid"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(premultiplied_background),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            if !instances.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.quad_vbo.slice(..));
                pass.set_vertex_buffer(1, self.instance_vbo.slice(..));
                pass.draw(0..4, 0..instances.len() as u32);
            }
        }

        queue.submit(Some(encoder.finish()));
    }
}
