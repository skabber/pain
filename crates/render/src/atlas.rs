//! Packs rasterized glyphs into a single GPU texture, shelf-packed left to
//! right, top to bottom. A small solid-white block is reserved at the origin
//! for drawing untextured quads (the cursor) through the same pipeline.

use std::collections::HashMap;

use crate::glyph::GlyphRasterizer;

const ATLAS_SIZE: u32 = 1024;
const SOLID_SIZE: u32 = 4;

/// Where a glyph (or the reserved solid block) lives in the atlas texture.
pub struct AtlasEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub width: f32,
    pub height: f32,
    pub left: f32,
    pub top: f32,
}

pub struct GlyphAtlas {
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    rasterizer: GlyphRasterizer,
    entries: HashMap<(char, u32, String), AtlasEntry>,
    cursor: (u32, u32),
    shelf_height: u32,
    pub solid_uv: [f32; 2],
}

impl GlyphAtlas {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph-atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let solid = vec![255u8; (SOLID_SIZE * SOLID_SIZE) as usize];
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &solid,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SOLID_SIZE),
                rows_per_image: Some(SOLID_SIZE),
            },
            wgpu::Extent3d {
                width: SOLID_SIZE,
                height: SOLID_SIZE,
                depth_or_array_layers: 1,
            },
        );

        let solid_uv = [
            (SOLID_SIZE as f32 / 2.0) / ATLAS_SIZE as f32,
            (SOLID_SIZE as f32 / 2.0) / ATLAS_SIZE as f32,
        ];

        Self {
            texture,
            view,
            rasterizer: GlyphRasterizer::new(),
            entries: HashMap::new(),
            cursor: (SOLID_SIZE, 0),
            shelf_height: SOLID_SIZE,
            solid_uv,
        }
    }

    /// Returns the atlas entry for `c` at `size_px` in `family`, rasterizing
    /// and uploading it on first use. `None` for glyphs with no visible
    /// coverage.
    pub fn entry(&mut self, queue: &wgpu::Queue, c: char, size_px: f32, family: &str) -> Option<&AtlasEntry> {
        let key = (c, size_px.to_bits(), family.to_string());
        if !self.entries.contains_key(&key) {
            let glyph = self.rasterizer.rasterize(c, size_px, family)?;
            let (x, y) = self.alloc(glyph.width, glyph.height);

            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &glyph.coverage,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(glyph.width),
                    rows_per_image: Some(glyph.height),
                },
                wgpu::Extent3d {
                    width: glyph.width,
                    height: glyph.height,
                    depth_or_array_layers: 1,
                },
            );

            self.entries.insert(
                key.clone(),
                AtlasEntry {
                    uv_origin: [x as f32 / ATLAS_SIZE as f32, y as f32 / ATLAS_SIZE as f32],
                    uv_size: [
                        glyph.width as f32 / ATLAS_SIZE as f32,
                        glyph.height as f32 / ATLAS_SIZE as f32,
                    ],
                    width: glyph.width as f32,
                    height: glyph.height as f32,
                    left: glyph.left as f32,
                    top: glyph.top as f32,
                },
            );
        }
        self.entries.get(&key)
    }

    /// Shelf-packs a `width` x `height` region, advancing to a new shelf when
    /// the current one runs out of horizontal space. A 1px gap is left after
    /// each glyph so nearest-neighbor sampling can never pick up a texel
    /// from the next glyph over, even if a position ends up slightly off
    /// the pixel grid.
    ///
    /// v1 does not handle atlas exhaustion — the character sets rendered by
    /// a terminal fit comfortably within one 1024x1024 atlas.
    fn alloc(&mut self, width: u32, height: u32) -> (u32, u32) {
        if self.cursor.0 + width > ATLAS_SIZE {
            self.cursor.0 = 0;
            self.cursor.1 += self.shelf_height;
            self.shelf_height = 0;
        }
        let pos = self.cursor;
        self.cursor.0 += width + 1;
        self.shelf_height = self.shelf_height.max(height + 1);
        pos
    }
}
