//! Rasterizes individual characters into 8-bit coverage masks via `cosmic-text`.
//!
//! Grid cells are fixed-width, so glyphs are rasterized independently per
//! character rather than shaped as a run — there is no ligature or kerning
//! concern in a monospace terminal grid.

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent};

/// A rasterized glyph: an 8-bit coverage mask plus its placement relative to
/// the cell's pen origin (baseline-left).
pub struct RasterizedGlyph {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub coverage: Vec<u8>,
}

/// Rasterizes characters on demand. Callers are expected to cache the result
/// (see [`crate::atlas::GlyphAtlas`]) — rasterizing is not free.
pub struct GlyphRasterizer {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl GlyphRasterizer {
    pub fn new() -> Self {
        Self { font_system: FontSystem::new(), swash_cache: SwashCache::new() }
    }

    /// Returns the advance width of `c` at `size_px` in `family` — for a
    /// monospace font this is the terminal grid's cell width.
    pub fn advance_width(&mut self, c: char, size_px: f32, family: &str) -> Option<f32> {
        let metrics = Metrics::new(size_px, size_px * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_text(&c.to_string(), &Attrs::new().family(family_attr(family)), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let run = buffer.layout_runs().next()?;
        let glyph = run.glyphs.first()?;
        Some(glyph.w)
    }

    /// Rasterizes `c` at `size_px` in `family`. Returns `None` for
    /// characters with no visible coverage (space, control characters, a
    /// font with no glyph).
    pub fn rasterize(&mut self, c: char, size_px: f32, family: &str) -> Option<RasterizedGlyph> {
        let metrics = Metrics::new(size_px, size_px * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_text(&c.to_string(), &Attrs::new().family(family_attr(family)), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let run = buffer.layout_runs().next()?;
        let glyph = run.glyphs.first()?;
        let physical = glyph.physical((0.0, 0.0), 1.0);
        let image = self.swash_cache.get_image(&mut self.font_system, physical.cache_key).as_ref()?;

        if image.placement.width == 0 || image.placement.height == 0 {
            return None;
        }

        let coverage = match image.content {
            SwashContent::Mask => image.data.clone(),
            // Color glyphs (emoji) are out of scope for v1's monospace grid;
            // fall back to the alpha channel so they at least render as a shape.
            SwashContent::Color | SwashContent::SubpixelMask => image.data.chunks_exact(4).map(|px| px[3]).collect(),
        };

        Some(RasterizedGlyph {
            width: image.placement.width,
            height: image.placement.height,
            left: image.placement.left,
            top: image.placement.top,
            coverage,
        })
    }
}

impl Default for GlyphRasterizer {
    fn default() -> Self {
        Self::new()
    }
}

/// `""` and `"monospace"` both mean "system default monospace" — the same
/// convention `config::Appearance::default`'s `font_family` uses — so an
/// empty config value (as ships out of the box) and an explicit generic
/// name both resolve the same way, rather than an empty string failing to
/// match any real font.
fn family_attr(name: &str) -> Family<'_> {
    if name.is_empty() || name.eq_ignore_ascii_case("monospace") { Family::Monospace } else { Family::Name(name) }
}

/// Every monospaced font family installed on the system, deduplicated and
/// sorted — for the settings panel's font picker. Scans the system font
/// database on first call only (a real, if one-time, disk/registry scan),
/// then reuses that result for the rest of the process's lifetime; a
/// user's installed fonts don't change while this is running.
pub fn monospace_font_families() -> &'static [String] {
    static FAMILIES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FAMILIES.get_or_init(|| {
        let db = FontSystem::new().db().clone();
        let mut names: Vec<String> = db
            .faces()
            .filter(|face| face.monospaced)
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    })
}

/// A native-feeling UI sans-serif face, tried roughly in "most likely to
/// actually be the platform's own default" order, falling through to
/// `fontdb`'s own generic `SansSerif` mapping only as a last resort — that
/// generic mapping (via `cosmic_text::FontSystem`) is hardcoded to "Open
/// Sans" regardless of platform, a Google web font that isn't actually
/// installed by default on Windows, macOS, or most Linux distros, so
/// relying on it alone would fail far more often than it should.
const SYSTEM_SANS_CANDIDATES: &[&str] = &[
    "Segoe UI",        // Windows
    "Helvetica Neue",  // macOS
    "Ubuntu",          // Ubuntu desktop
    "Cantarell",       // GNOME
    "Noto Sans",       // common on many Linux distros/Android
    "DejaVu Sans",     // near-universal on Linux
    "Liberation Sans", // near-universal on Linux, metric-compatible with Arial
    "Arial",           // near-universal, ships or is aliased almost everywhere else
];

/// The system's own default UI sans-serif face (raw font bytes + face
/// index, e.g. for a `.ttc` collection) — for theming `egui` chrome
/// (context menu, settings panel) with a native-feeling font instead of
/// `egui`'s bundled default, the same way the terminal grid itself
/// resolves a real installed font rather than shipping one. `None` if
/// nothing in `SYSTEM_SANS_CANDIDATES`, nor `fontdb`'s own generic
/// mapping, resolves to an installed font (unusual, but not impossible) —
/// callers should fall back to leaving `egui`'s own default font alone in
/// that case, not treat it as an error.
///
/// Scans the system font database on first call only, same as
/// `monospace_font_families`, and for the same reason.
pub fn system_ui_font_data() -> Option<&'static (Vec<u8>, u32)> {
    static FONT: std::sync::OnceLock<Option<(Vec<u8>, u32)>> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let db = FontSystem::new().db().clone();
        let mut families: Vec<fontdb::Family> =
            SYSTEM_SANS_CANDIDATES.iter().map(|name| fontdb::Family::Name(name)).collect();
        families.push(fontdb::Family::SansSerif);
        let query = fontdb::Query { families: &families, ..fontdb::Query::default() };
        let id = db.query(&query)?;
        db.with_face_data(id, |data, index| (data.to_vec(), index))
    })
    .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterizes_a_visible_character() {
        let mut rasterizer = GlyphRasterizer::new();
        let glyph = rasterizer.rasterize('A', 16.0, "").expect("'A' should rasterize to a visible glyph");

        assert!(glyph.width > 0);
        assert!(glyph.height > 0);
        assert!(
            glyph.coverage.iter().any(|&byte| byte > 0),
            "expected at least one covered pixel in the rasterized 'A'"
        );
    }

    #[test]
    fn space_has_no_visible_coverage() {
        let mut rasterizer = GlyphRasterizer::new();
        assert!(rasterizer.rasterize(' ', 16.0, "").is_none());
    }

    #[test]
    fn system_ui_font_data_finds_real_non_empty_font_bytes() {
        // Every real desktop this runs on has *some* sans-serif installed
        // (it's how the OS renders its own UI) — `None` would mean the
        // system's own reported default can't be found in its own font
        // database, which would be a genuinely broken font setup, not
        // something to design around here.
        let (bytes, _index) = system_ui_font_data().expect("a real system should have a default sans-serif face");
        assert!(!bytes.is_empty());
    }
}
