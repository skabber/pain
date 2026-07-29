//! Rasterizes glyphs into 8-bit coverage masks via `cosmic-text`.
//!
//! Two paths, deliberately:
//!
//! - **Per character** ([`GlyphRasterizer::rasterize`]) — the default. Grid
//!   cells are fixed-width, so each character is rasterized independently and
//!   cached by `char`. Nothing is shaped across cell boundaries, which is
//!   what makes it cheap.
//! - **Per run** ([`GlyphRasterizer::shape_run`]) — for ligature support,
//!   which is opt-in. A sequence of cells is shaped together so the font can
//!   substitute one glyph for several characters (`!=` becoming `≠`).
//!   Shaping is real work, so results are cached per run text and the whole
//!   cache is dropped whenever the font changes.
//!
//! The two paths coexist rather than one replacing the other: the per-char
//! path is what an idle terminal spends nothing on, and ligatures shouldn't
//! cost that for people who leave them off.

use std::collections::HashMap;

use cosmic_text::{Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent};

/// A glyph the shaper produced for a run: which glyph to draw, and where it
/// sits relative to the run's origin.
///
/// One of these can cover several source characters — that's what a ligature
/// is — so there is deliberately no per-character correspondence here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapedGlyph {
    pub key: CacheKey,
    pub x: i32,
    pub y: i32,
}

/// Cap on how many distinct run texts keep their shaping cached.
///
/// Terminal output repeats heavily frame to frame, so a small cache covers
/// almost every redraw; the cap exists so a pane streaming unique lines
/// can't grow it without bound. Cleared wholesale on overflow rather than
/// evicted one entry at a time — the next few frames re-shape what's still
/// on screen, which is bounded by the window size.
const SHAPE_CACHE_LIMIT: usize = 4096;

/// Color emoji families, tried in order — one per platform's own, then the
/// common third-party replacements.
///
/// An emoji has to be asked for by name rather than left to font fallback.
/// Fallback picks the first installed face that has *a* glyph for the
/// codepoint, and on a typical Linux install that is DejaVu Sans, which
/// carries monochrome outlines for many emoji — so the color font is never
/// reached and every emoji renders as a black-and-white silhouette
/// regardless of what else is installed. Confirmed directly rather than
/// assumed: shaping U+1F600 with the configured monospace family resolved
/// to DejaVu Sans, not to the color font sitting right beside it.
const EMOJI_FAMILIES: &[&str] = &[
    "Noto Color Emoji",  // most Linux distributions
    "Apple Color Emoji", // macOS
    "Segoe UI Emoji",    // Windows
    "Twemoji Mozilla",
    "JoyPixels",
    "EmojiOne Color",
];

/// Whether `c` is a character fonts normally draw as a color emoji.
///
/// Deliberately limited to the astral emoji blocks. Characters in
/// U+2600–U+27BF — `✓`, `★`, `➜`, `✗` — technically have emoji forms too,
/// but terminal programs use them constantly as ordinary text symbols in
/// build output and test results. Routing those to an emoji font would turn
/// a passing test suite into a column of colored pictures, and break their
/// alignment as single-width cells. Leaving them monochrome is both safer
/// and what every other terminal does by default.
fn is_emoji_presentation(c: char) -> bool {
    matches!(
        u32::from(c),
        0x1F300..=0x1F5FF   // Miscellaneous symbols and pictographs
            | 0x1F600..=0x1F64F // Emoticons
            | 0x1F680..=0x1F6FF // Transport and map symbols
            | 0x1F900..=0x1F9FF // Supplemental symbols and pictographs
            | 0x1FA70..=0x1FAFF // Symbols and pictographs extended-A
    )
}

/// A rasterized glyph's pixels, in whichever form the font provided.
pub enum GlyphPixels {
    /// 8-bit coverage. The glyph carries no color of its own — whatever is
    /// drawing it supplies that, which is how ordinary text takes on its
    /// SGR/theme color.
    Mask(Vec<u8>),
    /// Premultiplied RGBA, from a font that draws the glyph in its own
    /// colors (a color emoji). The color is part of the glyph and must not
    /// be tinted by the text color.
    ///
    /// Premultiplied here rather than at draw time: swash hands back
    /// *straight* RGBA (the same interpretation `cosmic-text`'s own
    /// `SwashCache` uses), while this crate's pipeline blends premultiplied
    /// throughout — a constraint that comes from Windows DirectComposition
    /// accepting no other alpha mode. Converting once on upload keeps the
    /// shader from having to know which convention a given texel is in.
    Color(Vec<u8>),
}

impl GlyphPixels {
    /// Whether these pixels carry their own color.
    pub fn is_color(&self) -> bool {
        matches!(self, GlyphPixels::Color(_))
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            GlyphPixels::Mask(data) | GlyphPixels::Color(data) => data,
        }
    }
}

/// A rasterized glyph: its pixels plus its placement relative to the cell's
/// pen origin (baseline-left).
pub struct RasterizedGlyph {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub pixels: GlyphPixels,
}

/// Rasterizes characters on demand. Callers are expected to cache the result
/// (see [`crate::atlas::GlyphAtlas`]) — rasterizing is not free.
pub struct GlyphRasterizer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    /// Shaped runs, keyed by their text. Valid only for `shape_font`; a font
    /// change drops the whole map (see [`GlyphRasterizer::shape_run`]).
    shape_cache: HashMap<String, Vec<ShapedGlyph>>,
    /// The size (as raw bits, since `f32` is neither `Hash` nor `Eq`) and
    /// family `shape_cache`'s entries were shaped at.
    shape_font: (u32, String),
    /// The first installed entry of [`EMOJI_FAMILIES`], resolved once on
    /// demand. The outer `Option` is "not looked up yet", the inner one "no
    /// color emoji font on this system" — a real and unremarkable state on a
    /// minimal Linux install, and not worth re-scanning the font database
    /// for on every emoji.
    emoji_family: Option<Option<String>>,
}

impl GlyphRasterizer {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
            shape_cache: HashMap::new(),
            // No real font size has zero bits, so the first `shape_run` call
            // always invalidates — a no-op on an already-empty cache.
            shape_font: (0, String::new()),
            emoji_family: None,
        }
    }

    /// The installed color emoji family to use, or `None` if this system has
    /// none. Resolved from the font database once and remembered.
    fn emoji_family(&mut self) -> Option<&str> {
        if self.emoji_family.is_none() {
            let db = self.font_system.db();
            let installed = EMOJI_FAMILIES
                .iter()
                .find(|wanted| {
                    db.faces().any(|face| face.families.iter().any(|(name, _)| name.eq_ignore_ascii_case(wanted)))
                })
                .map(|name| name.to_string());
            self.emoji_family = Some(installed);
        }
        self.emoji_family.as_ref().and_then(|family| family.as_deref())
    }

    /// The family `text` should actually be rendered with: a color emoji
    /// family when every character is an emoji and one is installed,
    /// otherwise the configured family unchanged.
    ///
    /// Applied per glyph and per shaped run, since a run reaching the shaper
    /// with the text family would resolve the same wrong (monochrome) face —
    /// see [`EMOJI_FAMILIES`].
    fn family_for<'a>(&mut self, text: &str, configured: &'a str) -> std::borrow::Cow<'a, str> {
        if text.is_empty() || !text.chars().all(is_emoji_presentation) {
            return std::borrow::Cow::Borrowed(configured);
        }
        match self.emoji_family() {
            Some(family) => std::borrow::Cow::Owned(family.to_string()),
            None => std::borrow::Cow::Borrowed(configured),
        }
    }

    /// Shapes `text` as a single run at `size_px` in `family`, letting the
    /// font apply ligatures and contextual substitutions across the whole
    /// run rather than treating each character independently.
    ///
    /// Callers must only pass text that is safe to ligate as a unit: one
    /// color, one set of attributes, and no cursor sitting inside it. The
    /// shaper has no idea where the terminal's cell boundaries are, so it is
    /// the caller's job to have already broken the row into runs where
    /// ligating is correct.
    ///
    /// Positions in the result are relative to the run's origin and come
    /// from the font's own advances, not from cell arithmetic — which is
    /// exactly why this is opt-in: a font whose ligature advances don't
    /// match its cell width will drift from the grid.
    pub fn shape_run(&mut self, text: &str, size_px: f32, family: &str) -> &[ShapedGlyph] {
        if self.shape_font.0 != size_px.to_bits() || self.shape_font.1 != family {
            self.shape_cache.clear();
            self.shape_font = (size_px.to_bits(), family.to_string());
        }
        if self.shape_cache.len() >= SHAPE_CACHE_LIMIT && !self.shape_cache.contains_key(text) {
            self.shape_cache.clear();
        }

        // `contains_key` then `insert` rather than the entry API: shaping
        // borrows `self.font_system` mutably, which an occupied `Entry`
        // holding a borrow of `self.shape_cache` would conflict with.
        if !self.shape_cache.contains_key(text) {
            let shaped = self.shape_uncached(text, size_px, family);
            self.shape_cache.insert(text.to_string(), shaped);
        }
        &self.shape_cache[text]
    }

    fn shape_uncached(&mut self, text: &str, size_px: f32, family: &str) -> Vec<ShapedGlyph> {
        let family = self.family_for(text, family).into_owned();

        let metrics = Metrics::new(size_px, size_px * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_text(text, &Attrs::new().family(family_attr(&family)), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let Some(run) = buffer.layout_runs().next() else {
            return Vec::new();
        };
        run.glyphs
            .iter()
            .map(|glyph| {
                let physical = glyph.physical((0.0, 0.0), 1.0);
                ShapedGlyph { key: physical.cache_key, x: physical.x, y: physical.y }
            })
            .collect()
    }

    /// Rasterizes a glyph the shaper already identified. The per-run
    /// counterpart to [`GlyphRasterizer::rasterize`], which resolves a
    /// `char` to its glyph itself.
    pub fn rasterize_key(&mut self, key: CacheKey) -> Option<RasterizedGlyph> {
        let image = self.swash_cache.get_image(&mut self.font_system, key).as_ref()?;
        rasterized(image)
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
        // An emoji has to name its font explicitly; ordinary fallback finds
        // a monochrome outline first. See `EMOJI_FAMILIES`.
        let text = c.to_string();
        let family = self.family_for(&text, family).into_owned();

        let metrics = Metrics::new(size_px, size_px * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_text(&text, &Attrs::new().family(family_attr(&family)), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let run = buffer.layout_runs().next()?;
        let glyph = run.glyphs.first()?;
        let physical = glyph.physical((0.0, 0.0), 1.0);
        let image = self.swash_cache.get_image(&mut self.font_system, physical.cache_key).as_ref()?;
        rasterized(image)
    }
}

/// Converts a swash image into a coverage mask, or `None` if it has no
/// visible extent. Shared by both the per-character and per-run paths so
/// they can't drift in how they interpret a glyph bitmap.
fn rasterized(image: &cosmic_text::SwashImage) -> Option<RasterizedGlyph> {
    if image.placement.width == 0 || image.placement.height == 0 {
        return None;
    }

    let pixels = match image.content {
        SwashContent::Mask => GlyphPixels::Mask(image.data.clone()),
        // A color emoji, drawn by the font in its own colors.
        SwashContent::Color => GlyphPixels::Color(premultiply(&image.data)),
        // Subpixel-antialiased coverage: three channels of per-subpixel
        // coverage rather than color. Reduced to its alpha channel, since
        // this renderer samples one coverage value per texel — the same
        // treatment as before color glyphs were split out.
        SwashContent::SubpixelMask => GlyphPixels::Mask(image.data.chunks_exact(4).map(|px| px[3]).collect()),
    };

    Some(RasterizedGlyph {
        width: image.placement.width,
        height: image.placement.height,
        left: image.placement.left,
        top: image.placement.top,
        pixels,
    })
}

/// Converts straight RGBA to premultiplied, scaling each color channel by
/// the pixel's own alpha. See [`GlyphPixels::Color`] for why this happens
/// here rather than in the shader.
fn premultiply(straight: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(straight.len());
    for px in straight.chunks_exact(4) {
        let alpha = px[3] as u32;
        // `+ 127) / 255` rounds to nearest rather than truncating; plain
        // integer division would darken every partially-transparent texel
        // by up to a full level, which shows up as a dark fringe around an
        // emoji's antialiased edge.
        let scale = |channel: u8| ((channel as u32 * alpha + 127) / 255) as u8;
        out.extend_from_slice(&[scale(px[0]), scale(px[1]), scale(px[2]), px[3]]);
    }
    out
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
            glyph.pixels.bytes().iter().any(|&byte| byte > 0),
            "expected at least one covered pixel in the rasterized 'A'"
        );
    }

    /// A color emoji must come back as real RGBA, not as the monochrome
    /// silhouette this used to reduce it to. Skipped where no color emoji
    /// font is installed — CI images often have none — so the assertion
    /// below is the meaningful part, not the test merely passing.
    #[test]
    fn a_color_emoji_rasterizes_to_color_pixels_not_a_silhouette() {
        let mut rasterizer = GlyphRasterizer::new();
        let Some(glyph) = rasterizer.rasterize('😀', 32.0, "Noto Color Emoji") else {
            return;
        };

        assert!(glyph.pixels.is_color(), "an emoji should rasterize as color, not coverage");
        let bytes = glyph.pixels.bytes();
        assert_eq!(bytes.len(), (glyph.width * glyph.height * 4) as usize, "color glyphs are 4 bytes per texel");

        // Not greyscale: at least one texel must have channels that differ
        // from each other, which a silhouette (or a mask widened to RGBA)
        // could never produce.
        let has_real_color = bytes.chunks_exact(4).any(|px| px[3] > 0 && (px[0] != px[1] || px[1] != px[2]));
        assert!(has_real_color, "expected at least one genuinely colored texel");
    }

    /// The bug this feature would otherwise have shipped with: the app never
    /// asks for an emoji font by name, it asks for the user's *monospace*
    /// family. Ordinary font fallback then resolves U+1F600 to DejaVu Sans,
    /// which has a monochrome outline for it, and the color font installed
    /// right beside it is never reached — so emoji stay silhouettes and
    /// every other part of this feature looks broken for no visible reason.
    ///
    /// Asserts the real path: configured monospace family in, color glyph
    /// out.
    #[test]
    fn an_emoji_renders_in_color_even_when_a_monospace_family_was_asked_for() {
        let mut rasterizer = GlyphRasterizer::new();
        if rasterizer.emoji_family().is_none() {
            return; // No color emoji font installed; nothing to assert.
        }

        for configured in ["", "monospace", "DejaVu Sans Mono"] {
            let glyph = rasterizer.rasterize('😀', 32.0, configured).expect("emoji should rasterize");
            assert!(
                glyph.pixels.is_color(),
                "emoji requested via family {configured:?} fell back to a monochrome face"
            );
        }
    }

    /// Text symbols that terminal programs print constantly must *not* be
    /// diverted to an emoji font — a passing test suite full of `✓` should
    /// stay monochrome text at one cell wide, not become colored pictures.
    #[test]
    fn common_text_symbols_are_not_treated_as_emoji() {
        for c in ['✓', '✔', '✗', '★', '→', '➜', '■', '°', 'A', ' '] {
            assert!(!is_emoji_presentation(c), "{c:?} should be treated as text, not emoji");
        }
    }

    #[test]
    fn astral_emoji_are_recognized() {
        for c in ['😀', '🎉', '🚀', '🧪', '🩰'] {
            assert!(is_emoji_presentation(c), "{c:?} should be recognized as an emoji");
        }
    }

    /// A mixed run must keep the configured text font — diverting it would
    /// render the whole run, letters included, in the emoji face.
    #[test]
    fn only_an_all_emoji_run_is_diverted_to_the_emoji_font() {
        let mut rasterizer = GlyphRasterizer::new();
        if rasterizer.emoji_family().is_none() {
            return;
        }
        let emoji_family = rasterizer.emoji_family().expect("checked above").to_string();

        assert_eq!(rasterizer.family_for("😀", "Mono"), emoji_family.as_str());
        assert_eq!(rasterizer.family_for("😀🎉", "Mono"), emoji_family.as_str());
        assert_eq!(rasterizer.family_for("ok😀", "Mono"), "Mono");
        assert_eq!(rasterizer.family_for("plain", "Mono"), "Mono");
        assert_eq!(rasterizer.family_for("", "Mono"), "Mono");
    }

    /// Premultiplication is what the pipeline expects, and getting it wrong
    /// shows as a dark fringe rather than an obvious failure — so assert the
    /// invariant directly: no channel may exceed its own alpha.
    #[test]
    fn color_glyph_texels_are_premultiplied() {
        let mut rasterizer = GlyphRasterizer::new();
        let Some(glyph) = rasterizer.rasterize('😀', 32.0, "Noto Color Emoji") else {
            return;
        };

        for px in glyph.pixels.bytes().chunks_exact(4) {
            let alpha = px[3];
            assert!(
                px[0] <= alpha && px[1] <= alpha && px[2] <= alpha,
                "texel {px:?} has a channel above its alpha, so it is not premultiplied"
            );
        }
    }

    #[test]
    fn premultiply_scales_channels_by_alpha_and_rounds_to_nearest() {
        // Fully opaque passes through untouched.
        assert_eq!(premultiply(&[10, 20, 30, 255]), vec![10, 20, 30, 255]);
        // Fully transparent collapses to zero.
        assert_eq!(premultiply(&[255, 255, 255, 0]), vec![0, 0, 0, 0]);
        // Half alpha halves each channel, rounded rather than truncated:
        // 255 * 128 / 255 = 128 exactly.
        assert_eq!(premultiply(&[255, 255, 255, 128]), vec![128, 128, 128, 128]);
    }

    /// Ordinary text must keep taking its color from the caller — the color
    /// path is additional, not a replacement.
    #[test]
    fn an_ordinary_character_still_rasterizes_as_coverage() {
        let mut rasterizer = GlyphRasterizer::new();
        let glyph = rasterizer.rasterize('A', 16.0, "").expect("'A' should rasterize");

        assert!(!glyph.pixels.is_color());
        assert_eq!(glyph.pixels.bytes().len(), (glyph.width * glyph.height) as usize, "masks are 1 byte per texel");
    }

    #[test]
    fn space_has_no_visible_coverage() {
        let mut rasterizer = GlyphRasterizer::new();
        assert!(rasterizer.rasterize(' ', 16.0, "").is_none());
    }

    /// The safety property that matters most, and the one testable without a
    /// ligature font installed: with an ordinary monospace font — which has
    /// no ligatures to apply — shaping a run must place each glyph at
    /// exactly the cell position the per-character path would use. If this
    /// drifts, enabling ligatures misaligns *all* text, not just the pairs a
    /// ligature font would have substituted.
    #[test]
    fn shaped_runs_land_on_cell_boundaries_for_a_font_without_ligatures() {
        let mut rasterizer = GlyphRasterizer::new();
        let size = 16.0;
        // Not the generic "monospace" alias: this needs a face that really
        // is fixed-width for the assertion to mean anything.
        let family = "DejaVu Sans Mono";
        let Some(advance) = rasterizer.advance_width('M', size, family) else {
            return; // Font not installed here; nothing to assert against.
        };

        let shaped = rasterizer.shape_run("a!=b", size, family).to_vec();
        assert_eq!(shaped.len(), 4, "a font without ligatures should shape 4 characters to 4 glyphs");
        for (i, glyph) in shaped.iter().enumerate() {
            let expected = (i as f32 * advance).round() as i32;
            assert!(
                (glyph.x - expected).abs() <= 1,
                "glyph {i} at x={} should sit within a pixel of its cell at {expected}",
                glyph.x
            );
        }
    }

    #[test]
    fn shaping_the_same_run_twice_returns_the_same_glyphs() {
        let mut rasterizer = GlyphRasterizer::new();
        let first = rasterizer.shape_run("hello", 16.0, "").to_vec();
        let second = rasterizer.shape_run("hello", 16.0, "").to_vec();
        assert_eq!(first, second);
        assert!(!first.is_empty(), "a real word should shape to at least one glyph");
    }

    /// A cached run shaped at the old size would place glyphs at the old
    /// advances — text at the wrong spacing until something else evicted it.
    #[test]
    fn the_shape_cache_is_dropped_when_the_font_size_changes() {
        let mut rasterizer = GlyphRasterizer::new();
        let small = rasterizer.shape_run("mmm", 12.0, "").to_vec();
        let large = rasterizer.shape_run("mmm", 24.0, "").to_vec();

        assert_eq!(small.len(), large.len());
        assert_ne!(small, large, "the same text at a different size must re-shape");
        // Both entries can't be live at once — the cache holds one font.
        assert_eq!(rasterizer.shape_cache.len(), 1);
    }

    #[test]
    fn an_empty_run_shapes_to_no_glyphs_rather_than_panicking() {
        let mut rasterizer = GlyphRasterizer::new();
        assert!(rasterizer.shape_run("", 16.0, "").is_empty());
    }

    /// The cache is bounded, so a pane streaming unique text can't grow it
    /// without limit.
    #[test]
    fn the_shape_cache_stays_bounded_across_many_distinct_runs() {
        let mut rasterizer = GlyphRasterizer::new();
        for i in 0..(SHAPE_CACHE_LIMIT + 50) {
            rasterizer.shape_run(&format!("run{i}"), 16.0, "");
        }
        assert!(rasterizer.shape_cache.len() <= SHAPE_CACHE_LIMIT);
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
