# Ligatures and color emoji

**Shipped:** 2026-07-28. Two features, one document, because they share the
glyph atlas and the reasoning only makes sense together.

## What exists

- **Ligatures**, opt-in via `appearance.ligatures` (default off). Text is
  shaped in runs so a font can render `!=` as one glyph.
- **Color emoji**, always on. Emoji render in their font's own colors rather
  than as monochrome silhouettes.

Files: `crates/render/src/glyph.rs` (rasterizing, shaping, emoji font
selection), `crates/render/src/atlas.rs` (two atlases), `shader.wgsl`,
`crates/app/src/run.rs` (run splitting).

## Ligatures

**A second rendering path, not a replacement.** The per-character path —
rasterize each cell independently, cache by `char` — is what makes an idle
terminal cost nothing, and it is still the default. Shaping is real per-frame
work. Replacing the cheap path with the expensive one for everybody would
have undone the v1.5 idle-cost work for a feature most users won't enable.

**Off by default for a second reason beyond cost.** Shaping hands glyph
positioning to the font's own advances instead of the cell grid. With a font
designed for it that lines up exactly; with one whose ligature widths don't
match its cell width, text drifts out of alignment. That is not a default
worth imposing.

**Runs break on three conditions**, each for a concrete reason (`run.rs`):

- *Color change* — a ligature is one glyph and carries one color. Red `!` and
  green `=` cannot be ligated without losing one.
- *Whitespace* — ligatures never span a space, and excluding spaces avoids
  shaping the empty right-hand side of most rows.
- *The cursor* — editing `!=` with the cursor between the characters must
  show which one you're on. Every ligature-capable terminal does this.

**The property that was actually verified.** No ligature font was installed
in the development environment, so substitution could not be demonstrated
locally. The property that matters more *is* testable and is asserted: with
an ordinary monospace font, shaped runs land within one pixel of the
per-character cell positions. If that drifted, enabling ligatures would
misalign *all* text, not just the pairs a ligature font would substitute.

## Color emoji

**Two atlases, not one widened atlas.** The obvious approach — make the
existing 2048² atlas RGBA — costs 16MB instead of 4MB *and* quarters how many
ordinary text glyphs fit before a repack, because an RGBA texel is four times
a coverage texel. That second cost is the real one: it degrades CJK and
symbol-heavy output for users who never render an emoji. Instead the coverage
atlas is untouched and a separate 1024² RGBA atlas holds color glyphs. Total
8MB, text capacity unchanged.

Both textures are bound for every draw and the fragment shader selects per
instance via a `colored` flag, rather than splitting into two pipelines and
two passes for what is usually a handful of glyphs.

**Color texels are premultiplied on upload.** swash returns *straight* RGBA
(verified against `cosmic-text`'s own `SwashCache`, which reads it that way),
while this pipeline blends premultiplied throughout — a constraint inherited
from Windows DirectComposition accepting no other alpha mode. Converting once
at upload keeps the shader from having to know which convention a texel is
in. Rounding is to nearest, not truncating: truncating darkens every
partially-transparent texel by up to a level, showing as a dark fringe around
an emoji's antialiased edge.

**The emoji font must be named explicitly.** This is the non-obvious part,
and the bug that would otherwise have shipped the whole feature invisible.
The app asks for the user's *monospace* family. Ordinary font fallback then
resolves U+1F600 to the first installed face that has any glyph for it — on a
typical Linux install, DejaVu Sans, which carries monochrome outlines for
many emoji. The color font sitting right beside it is never reached, and
every emoji renders as a silhouette exactly as before.

`EMOJI_FAMILIES` + `family_for` divert an all-emoji glyph or run to the first
installed color emoji family, resolved once from the font database and
cached. A regression test asserts the real path: configured monospace family
in, color glyph out.

**Only the astral blocks are diverted.** `is_emoji_presentation` covers
U+1F300–U+1FAFF. U+2600–U+27BF (`✓`, `✗`, `★`, `➜`) is deliberately excluded:
those have emoji forms, but terminal programs print them constantly as
ordinary text in build and test output. Diverting them would turn a passing
test suite into a column of colored pictures and break their single-width
alignment. Tested in both directions.

## Consequences worth knowing

- The atlas is keyed by `GlyphKey::{Char, Shaped}`. One shaped glyph can
  stand for several characters, so no `char` identifies it.
- The shaping cache is capped and cleared wholesale on overflow or font
  change; a pane streaming unique lines cannot grow it without bound.
- Emoji tests skip silently where no color emoji font is installed. That is
  by design, but it means **CI does not currently protect this feature** — a
  bare runner has no such font. See the CI gap noted in `project.md`.
