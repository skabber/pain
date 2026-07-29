//! Packs rasterized glyphs into a single GPU texture, shelf-packed left to
//! right, top to bottom. A small solid-white block is reserved at the origin
//! for drawing untextured quads (the cursor) through the same pipeline.

use std::collections::HashMap;

use crate::glyph::GlyphRasterizer;

const ATLAS_SIZE: u32 = 2048;
const SOLID_SIZE: u32 = 4;

/// Color glyphs (emoji) live in their own smaller RGBA atlas rather than
/// sharing the coverage one.
///
/// Two atlases rather than one RGBA atlas for everything: an RGBA texel is
/// four times the size of a coverage texel, so a single shared atlas would
/// let a screenful of ordinary text hold a quarter as many distinct glyphs
/// before exhausting itself and repacking — a real cost to CJK or
/// symbol-heavy output, paid for a feature it isn't using. Splitting them
/// keeps the text atlas exactly as capacious as it was, and costs 4MB
/// instead of the 12MB that widening the main atlas would have.
///
/// 1024 is generous for the purpose: emoji are sparse in terminal output,
/// and even at the maximum font size this holds far more distinct ones than
/// fit on screen at once. Running out is handled the same way as the main
/// atlas — repack, and the next frame refills.
const COLOR_ATLAS_SIZE: u32 = 1024;

/// What a packed atlas entry is keyed by.
///
/// The two variants correspond to the rasterizer's two paths and never
/// collide: the per-character path resolves a `char` to a glyph itself,
/// while the ligature path is handed a glyph the shaper already picked —
/// and one shaped glyph can stand for several characters, so there is no
/// `char` that would identify it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GlyphKey {
    Char(char),
    Shaped(cosmic_text::CacheKey),
}

/// Where a glyph (or the reserved solid block) lives in an atlas texture.
pub struct AtlasEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub width: f32,
    pub height: f32,
    pub left: f32,
    pub top: f32,
    /// Whether this lives in the color atlas and carries its own color,
    /// rather than in the coverage atlas to be tinted by the text color.
    /// Selects which texture the shader samples.
    pub colored: bool,
}

/// Tracks the next free position in a shelf-packed atlas: glyphs are laid out
/// left to right along a shelf, and a new shelf starts below the tallest
/// glyph on the previous one once the current shelf runs out of width.
struct ShelfPacker {
    cursor: (u32, u32),
    shelf_height: u32,
    /// Side length of the texture being packed — the coverage and color
    /// atlases are different sizes.
    size: u32,
}

impl ShelfPacker {
    /// Starts packing just past the reserved solid block, which occupies the
    /// origin of the first shelf.
    fn new() -> Self {
        Self { cursor: (SOLID_SIZE, 0), shelf_height: SOLID_SIZE, size: ATLAS_SIZE }
    }

    /// Starts packing a `size`-square texture from its origin — for an atlas
    /// with no reserved block to step over.
    fn with_size(size: u32) -> Self {
        Self { cursor: (0, 0), shelf_height: 0, size }
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
        if width > self.size || height > self.size {
            return None;
        }
        if self.cursor.0 + width > self.size {
            self.cursor.0 = 0;
            self.cursor.1 += self.shelf_height;
            self.shelf_height = 0;
        }
        if self.cursor.1 + height > self.size {
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
    /// RGBA companion to `texture`, holding only glyphs that carry their own
    /// color. See [`COLOR_ATLAS_SIZE`] for why these are separate.
    color_texture: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    rasterizer: GlyphRasterizer,
    entries: HashMap<GlyphKey, AtlasEntry>,
    /// The font size (as raw bits, since `f32` is not `Hash`/`Eq`) and family
    /// the currently packed entries were rasterized at.
    font: (u32, String),
    packer: ShelfPacker,
    color_packer: ShelfPacker,
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

        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph-atlas-color"),
            size: wgpu::Extent3d { width: COLOR_ATLAS_SIZE, height: COLOR_ATLAS_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

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
            color_texture,
            color_view,
            rasterizer: GlyphRasterizer::new(),
            entries: HashMap::new(),
            // No real font size rasterizes to zero bits, so the first `entry`
            // call always repacks — which at that point is a no-op on an
            // already-empty atlas.
            font: (0, String::new()),
            packer: ShelfPacker::new(),
            color_packer: ShelfPacker::with_size(COLOR_ATLAS_SIZE),
            solid_uv,
        }
    }

    /// Returns the atlas entry for `c` at `size_px` in `family`, rasterizing
    /// and uploading it on first use. `None` for glyphs with no visible
    /// coverage, or larger than the atlas itself.
    pub fn entry(&mut self, queue: &wgpu::Queue, c: char, size_px: f32, family: &str) -> Option<&AtlasEntry> {
        self.invalidate_on_font_change(size_px, family);
        self.pack(queue, GlyphKey::Char(c), |rasterizer| rasterizer.rasterize(c, size_px, family))
    }

    /// Shapes `text` as one run — see [`GlyphRasterizer::shape_run`]. Lives
    /// here because the rasterizer (which owns the font system and the
    /// shaping cache) does, so callers only need the atlas.
    pub fn shape_run(&mut self, text: &str, size_px: f32, family: &str) -> &[crate::glyph::ShapedGlyph] {
        self.rasterizer.shape_run(text, size_px, family)
    }

    /// Returns the atlas entry for a glyph the shaper already identified —
    /// the ligature path's counterpart to [`GlyphAtlas::entry`].
    ///
    /// `size_px`/`family` are passed only to keep this sharing the same
    /// font-change invalidation; the glyph itself is fully determined by
    /// `shaped`, whose cache key already encodes the face and size it was
    /// shaped at.
    pub fn shaped_entry(
        &mut self,
        queue: &wgpu::Queue,
        shaped: crate::glyph::ShapedGlyph,
        size_px: f32,
        family: &str,
    ) -> Option<&AtlasEntry> {
        self.invalidate_on_font_change(size_px, family);
        self.pack(queue, GlyphKey::Shaped(shaped.key), |rasterizer| rasterizer.rasterize_key(shaped.key))
    }

    /// Drops every packed entry when the font changes, so a size or family
    /// change invalidates all of them at once.
    ///
    /// Repacking rather than accumulating is what keeps the atlas bounded:
    /// every distinct size a user drags the font-size slider through would
    /// otherwise take its own permanent copy of the whole character set, and
    /// exhaust the atlas within a few adjustments.
    fn invalidate_on_font_change(&mut self, size_px: f32, family: &str) {
        if self.font.0 != size_px.to_bits() || self.font.1 != family {
            self.repack();
            self.font = (size_px.to_bits(), family.to_string());
        }
    }

    /// Rasterizes and uploads `key`'s glyph on first use, then returns its
    /// entry. `rasterize` is only called on a miss.
    fn pack(
        &mut self,
        queue: &wgpu::Queue,
        key: GlyphKey,
        rasterize: impl FnOnce(&mut GlyphRasterizer) -> Option<crate::glyph::RasterizedGlyph>,
    ) -> Option<&AtlasEntry> {
        if !self.entries.contains_key(&key) {
            let glyph = rasterize(&mut self.rasterizer)?;
            let colored = glyph.pixels.is_color();
            // Each kind goes to its own texture, with its own packer and its
            // own side length to normalize UVs against. Only `Copy` values
            // are held here — the packer and texture are re-borrowed at each
            // use, since `repack` below needs `&mut self`.
            let (atlas_size, bytes_per_texel) = if colored { (COLOR_ATLAS_SIZE, 4) } else { (ATLAS_SIZE, 1) };
            let alloc = |atlas: &mut Self| {
                let packer = if colored { &mut atlas.color_packer } else { &mut atlas.packer };
                packer.alloc(glyph.width, glyph.height)
            };

            let (x, y) = match alloc(self) {
                Some(pos) => pos,
                None => {
                    // More distinct glyphs on screen at once than the atlas
                    // holds. Repacking drops entries already handed out this
                    // frame, so their quads sample whatever lands there
                    // instead — a garbled frame, which the next redraw
                    // corrects, rather than a crash.
                    self.repack();
                    alloc(self)?
                }
            };
            let texture = if colored { &self.color_texture } else { &self.texture };

            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                glyph.pixels.bytes(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(glyph.width * bytes_per_texel),
                    rows_per_image: Some(glyph.height),
                },
                wgpu::Extent3d { width: glyph.width, height: glyph.height, depth_or_array_layers: 1 },
            );

            self.entries.insert(
                key,
                AtlasEntry {
                    uv_origin: [x as f32 / atlas_size as f32, y as f32 / atlas_size as f32],
                    uv_size: [glyph.width as f32 / atlas_size as f32, glyph.height as f32 / atlas_size as f32],
                    width: glyph.width as f32,
                    height: glyph.height as f32,
                    left: glyph.left as f32,
                    top: glyph.top as f32,
                    colored,
                },
            );
        }
        self.entries.get(&key)
    }

    /// Drops every packed glyph and starts filling the atlas again from the
    /// beginning. The texture's own texels are left alone: the reserved solid
    /// block has to survive (nothing re-uploads it), and every other stale
    /// texel is overwritten by the glyph that gets packed over it before
    /// anything samples there.
    fn repack(&mut self) {
        self.entries.clear();
        self.packer = ShelfPacker::new();
        self.color_packer = ShelfPacker::with_size(COLOR_ATLAS_SIZE);
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

    /// Color glyphs upload to a different texture, in a different format, at
    /// four bytes per texel instead of one. A wrong `bytes_per_row` or a
    /// glyph routed to the wrong texture is a wgpu validation failure, not a
    /// visual glitch — and only a real device catches it.
    ///
    /// Also checks the two kinds coexist: text and emoji are packed in the
    /// same pass, into separate atlases, and each entry must know which one
    /// it landed in.
    #[test]
    fn color_glyphs_upload_to_the_color_atlas_alongside_ordinary_text() {
        let Some((device, queue)) = headless_device() else {
            eprintln!("no wgpu adapter available — skipping");
            return;
        };

        let mut atlas = GlyphAtlas::new(&device, &queue);
        let emoji_is_colored = atlas.entry(&queue, '😀', 32.0, "Noto Color Emoji").map(|entry| entry.colored);
        let text_is_colored = atlas.entry(&queue, 'A', 32.0, "Noto Color Emoji").map(|entry| entry.colored);
        device.poll(wgpu::PollType::wait_indefinitely()).expect("device should stay alive");

        // `None` where no emoji font is installed; the assertion is only
        // meaningful when one is.
        if let Some(colored) = emoji_is_colored {
            assert!(colored, "an emoji should be packed into the color atlas");
        }
        if let Some(colored) = text_is_colored {
            assert!(!colored, "ordinary text should stay in the coverage atlas");
        }

        std::mem::forget((atlas, device, queue));
    }

    /// Exhausting the color atlas must repack rather than run past the
    /// texture's edge — the same failure mode the font-size sweep above
    /// covers for the coverage atlas, on the smaller texture where it is
    /// reached sooner.
    #[test]
    fn filling_the_color_atlas_repacks_instead_of_overrunning_it() {
        let Some((device, queue)) = headless_device() else {
            eprintln!("no wgpu adapter available — skipping");
            return;
        };

        let mut atlas = GlyphAtlas::new(&device, &queue);
        // A wide sweep of emoji at a large size, which is what actually
        // pressures a 1024-square atlas.
        for size in [32, 64, 96] {
            for c in '\u{1F600}'..='\u{1F64F}' {
                atlas.entry(&queue, c, size as f32, "Noto Color Emoji");
            }
            device.poll(wgpu::PollType::wait_indefinitely()).expect("device should stay alive");
        }

        std::mem::forget((atlas, device, queue));
    }
}
