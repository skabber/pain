//! Packs rasterized glyphs into a single GPU texture, shelf-packed left to
//! right, top to bottom. A small solid-white block is reserved at the origin
//! for drawing untextured quads (the cursor) through the same pipeline.

use std::collections::HashMap;

use crate::glyph::GlyphRasterizer;

const ATLAS_SIZE: u32 = 2048;
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

/// Tracks the next free position in a shelf-packed atlas: glyphs are laid out
/// left to right along a shelf, and a new shelf starts below the tallest
/// glyph on the previous one once the current shelf runs out of width.
struct ShelfPacker {
    cursor: (u32, u32),
    shelf_height: u32,
}

impl ShelfPacker {
    /// Starts packing just past the reserved solid block, which occupies the
    /// origin of the first shelf.
    fn new() -> Self {
        Self { cursor: (SOLID_SIZE, 0), shelf_height: SOLID_SIZE }
    }

    /// Reserves a `width` x `height` region, advancing to a new shelf when the
    /// current one runs out of horizontal space. A 1px gap is left after each
    /// glyph so nearest-neighbor sampling can never pick up a texel from the
    /// next glyph over, even if a position ends up slightly off the pixel grid.
    ///
    /// `None` once the atlas has no room left — the caller's cue to repack
    /// from scratch. Uploading an unchecked position instead is a hard crash,
    /// not a rendering glitch: wgpu rejects a `write_texture` that runs past
    /// the destination's bounds.
    fn alloc(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width > ATLAS_SIZE || height > ATLAS_SIZE {
            return None;
        }
        if self.cursor.0 + width > ATLAS_SIZE {
            self.cursor.0 = 0;
            self.cursor.1 += self.shelf_height;
            self.shelf_height = 0;
        }
        if self.cursor.1 + height > ATLAS_SIZE {
            return None;
        }

        let pos = self.cursor;
        self.cursor.0 += width + 1;
        self.shelf_height = self.shelf_height.max(height + 1);
        Some(pos)
    }
}

pub struct GlyphAtlas {
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    rasterizer: GlyphRasterizer,
    entries: HashMap<char, AtlasEntry>,
    /// The font size (as raw bits, since `f32` is not `Hash`/`Eq`) and family
    /// the currently packed entries were rasterized at.
    font: (u32, String),
    packer: ShelfPacker,
    pub solid_uv: [f32; 2],
}

impl GlyphAtlas {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph-atlas"),
            size: wgpu::Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 },
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
            wgpu::Extent3d { width: SOLID_SIZE, height: SOLID_SIZE, depth_or_array_layers: 1 },
        );

        let solid_uv = [(SOLID_SIZE as f32 / 2.0) / ATLAS_SIZE as f32, (SOLID_SIZE as f32 / 2.0) / ATLAS_SIZE as f32];

        Self {
            texture,
            view,
            rasterizer: GlyphRasterizer::new(),
            entries: HashMap::new(),
            // No real font size rasterizes to zero bits, so the first `entry`
            // call always repacks — which at that point is a no-op on an
            // already-empty atlas.
            font: (0, String::new()),
            packer: ShelfPacker::new(),
            solid_uv,
        }
    }

    /// Returns the atlas entry for `c` at `size_px` in `family`, rasterizing
    /// and uploading it on first use. `None` for glyphs with no visible
    /// coverage, or larger than the atlas itself.
    pub fn entry(&mut self, queue: &wgpu::Queue, c: char, size_px: f32, family: &str) -> Option<&AtlasEntry> {
        // Entries are keyed by character alone, so a size or family change
        // invalidates all of them at once. Repacking rather than accumulating
        // is what keeps the atlas bounded: every distinct size a user drags
        // the font-size slider through would otherwise take its own permanent
        // copy of the whole character set, and exhaust the atlas within a few
        // adjustments.
        if self.font.0 != size_px.to_bits() || self.font.1 != family {
            self.repack();
            self.font = (size_px.to_bits(), family.to_string());
        }

        if !self.entries.contains_key(&c) {
            let glyph = self.rasterizer.rasterize(c, size_px, family)?;
            let (x, y) = match self.packer.alloc(glyph.width, glyph.height) {
                Some(pos) => pos,
                None => {
                    // More distinct glyphs on screen at once than the atlas
                    // holds. Repacking drops entries already handed out this
                    // frame, so their quads sample whatever lands there
                    // instead — a garbled frame, which the next redraw
                    // corrects, rather than a crash.
                    self.repack();
                    self.packer.alloc(glyph.width, glyph.height)?
                }
            };

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
                wgpu::Extent3d { width: glyph.width, height: glyph.height, depth_or_array_layers: 1 },
            );

            self.entries.insert(
                c,
                AtlasEntry {
                    uv_origin: [x as f32 / ATLAS_SIZE as f32, y as f32 / ATLAS_SIZE as f32],
                    uv_size: [glyph.width as f32 / ATLAS_SIZE as f32, glyph.height as f32 / ATLAS_SIZE as f32],
                    width: glyph.width as f32,
                    height: glyph.height as f32,
                    left: glyph.left as f32,
                    top: glyph.top as f32,
                },
            );
        }
        self.entries.get(&c)
    }

    /// Drops every packed glyph and starts filling the atlas again from the
    /// beginning. The texture's own texels are left alone: the reserved solid
    /// block has to survive (nothing re-uploads it), and every other stale
    /// texel is overwritten by the glyph that gets packed over it before
    /// anything samples there.
    fn repack(&mut self) {
        self.entries.clear();
        self.packer = ShelfPacker::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_stay_within_the_atlas() {
        let mut packer = ShelfPacker::new();
        for _ in 0..10_000 {
            let Some((x, y)) = packer.alloc(30, 50) else {
                break;
            };
            assert!(x + 30 <= ATLAS_SIZE, "allocation runs past the right edge");
            assert!(y + 50 <= ATLAS_SIZE, "allocation runs past the bottom edge");
        }
    }

    #[test]
    fn wraps_to_a_new_shelf_when_the_row_fills() {
        let mut packer = ShelfPacker::new();
        let (_, first_y) = packer.alloc(ATLAS_SIZE - SOLID_SIZE - 1, 20).expect("first fits");
        let (next_x, next_y) = packer.alloc(10, 20).expect("second fits on a new shelf");

        assert_eq!(next_x, 0, "a new shelf starts at the left edge");
        assert!(next_y > first_y, "a new shelf starts below the previous one");
    }

    #[test]
    fn refuses_to_allocate_once_full() {
        // Regression: the packer used to hand out positions past the bottom
        // edge once the atlas filled up, which wgpu rejects as a fatal
        // validation error on upload rather than a recoverable failure.
        let mut packer = ShelfPacker::new();
        let mut allocations = 0;
        while packer.alloc(30, 50).is_some() {
            allocations += 1;
            assert!(allocations < 10_000, "packer never reports the atlas as full");
        }
    }

    #[test]
    fn refuses_a_glyph_larger_than_the_atlas() {
        let mut packer = ShelfPacker::new();
        assert!(packer.alloc(ATLAS_SIZE + 1, 10).is_none());
        assert!(packer.alloc(10, ATLAS_SIZE + 1).is_none());
    }

    #[test]
    fn a_fresh_packer_has_room_again() {
        let mut packer = ShelfPacker::new();
        while packer.alloc(30, 50).is_some() {}

        let mut packer = ShelfPacker::new();
        assert!(packer.alloc(30, 50).is_some(), "repacking should free the atlas");
    }

    /// A headless device, or `None` where no adapter exists at all (a build
    /// machine with no GPU and no software rasterizer) — the GPU-backed test
    /// below skips rather than fails there, since it is testing this crate,
    /// not the host's graphics stack.
    fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    #[test]
    fn sweeping_font_sizes_never_overruns_the_texture() {
        // The reported crash: every font size the settings slider passes
        // through used to claim its own permanent copy of the character set,
        // and once the atlas filled up the next upload ran past the texture's
        // bottom edge — which wgpu treats as fatal. wgpu's validation runs on
        // the real device, so this needs one; a bad `write_texture` panics
        // here exactly as it did in the app.
        let Some((device, queue)) = headless_device() else {
            eprintln!("no wgpu adapter available — skipping");
            return;
        };

        let mut atlas = GlyphAtlas::new(&device, &queue);
        for size in 6..=48 {
            for c in ' '..='~' {
                atlas.entry(&queue, c, size as f32, "");
            }
            device.poll(wgpu::PollType::wait_indefinitely()).expect("device should stay alive");
        }

        // Leaked deliberately: tearing a wgpu device down segfaults inside
        // WSLg's GL driver, which would fail this test for a reason that has
        // nothing to do with the atlas. The process is exiting anyway.
        std::mem::forget((atlas, device, queue));
    }
}
