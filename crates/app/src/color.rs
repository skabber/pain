//! Resolves a terminal cell's ANSI color (a named slot, a 256-index, or a
//! direct RGB spec — `pane::Color`, straight off `alacritty_terminal`'s
//! grid) into a concrete RGB the renderer can use.
//!
//! The 16 base ANSI colors come from the user's chosen theme
//! (`config::themes`), threaded in as a `Palette` rather than hardcoded, so
//! switching theme restyles every already-running pane's output on the next
//! frame with nothing to invalidate. `Color::Named(Foreground/Background)`
//! resolve to whatever the caller passes as the default, which is likewise
//! the theme's — so cells left at their default color stay consistent with
//! the rest of the chrome (and with transparency/background-color settings)
//! instead of being pinned to a fixed value.
//!
//! The 256-color cube and grayscale ramp are *not* themed: those are
//! computed from the standard xterm formula, the same in every terminal.
//! Only slots 0-15 of the indexed range come from the theme, which is the
//! universal convention — a program asking for index 200 wants that exact
//! color, not a reinterpretation of it.

use pane::{Color, Flags, NamedColor};

/// The 16 base ANSI colors (0-7 normal, 8-15 bright) of the active theme.
pub type Palette = [[f32; 3]; 16];

/// Resolves a 256-color palette index (0-255): 0-15 are the theme's base
/// ANSI colors, 16-231 a 6x6x6 color cube, 232-255 a 24-step grayscale
/// ramp — the standard xterm 256-color layout.
fn indexed_color(index: u8, palette: &Palette) -> [f32; 3] {
    match index {
        0..=15 => palette[index as usize],
        16..=231 => {
            const LEVELS: [f32; 6] = [0.0, 95.0, 135.0, 175.0, 215.0, 255.0];
            let i = index - 16;
            let r = LEVELS[(i / 36) as usize];
            let g = LEVELS[((i / 6) % 6) as usize];
            let b = LEVELS[(i % 6) as usize];
            [r / 255.0, g / 255.0, b / 255.0]
        }
        232..=255 => {
            let level = (8 + 10 * (index - 232)) as f32 / 255.0;
            [level, level, level]
        }
    }
}

/// Resolves any `Color` value, given what `Named(Foreground)`/
/// `Named(Background)` (a cell left at its default) should fall back to.
/// `bold_brightens` promotes a base 0-7 named color to its 8-15 bright
/// counterpart — the conventional "bold means bright" terminal behavior,
/// which only ever applies to foreground text, never backgrounds.
pub fn resolve(color: Color, flags: Flags, bold_brightens: bool, default: [f32; 3], palette: &Palette) -> [f32; 3] {
    match color {
        Color::Named(NamedColor::Foreground) | Color::Named(NamedColor::Background) => default,
        Color::Named(named) => {
            let index = named as usize;
            if index >= 16 {
                return default;
            }
            if bold_brightens && flags.contains(Flags::BOLD) && index < 8 { palette[index + 8] } else { palette[index] }
        }
        Color::Indexed(index) => indexed_color(index, palette),
        Color::Spec(rgb) => [rgb.r as f32 / 255.0, rgb.g as f32 / 255.0, rgb.b as f32 / 255.0],
    }
}

/// Whether `color` is the default background (`Named(Background)`, what
/// every cell starts as) — callers should skip drawing a background rect
/// for these instead of painting the ambient pane background over itself.
pub fn is_default_background(color: Color) -> bool {
    matches!(color, Color::Named(NamedColor::Background))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped default theme — xterm's standard 16, which is what this
    /// module used to hardcode.
    fn palette() -> Palette {
        config::Appearance::default().palette()
    }

    #[test]
    fn named_ansi_colors_resolve_to_the_active_palette() {
        let palette = palette();
        assert_eq!(resolve(Color::Named(NamedColor::Red), Flags::empty(), true, [1.0, 1.0, 1.0], &palette), palette[1]);
    }

    #[test]
    fn bold_brightens_a_base_color_for_foreground_but_not_background() {
        let palette = palette();
        let bold = Flags::BOLD;
        assert_eq!(resolve(Color::Named(NamedColor::Red), bold, true, [0.0, 0.0, 0.0], &palette), palette[9]);
        assert_eq!(resolve(Color::Named(NamedColor::Red), bold, false, [0.0, 0.0, 0.0], &palette), palette[1]);
    }

    #[test]
    fn default_foreground_and_background_fall_back_to_the_given_default() {
        let palette = palette();
        let default = [0.5, 0.25, 0.75];
        assert_eq!(resolve(Color::Named(NamedColor::Foreground), Flags::empty(), true, default, &palette), default);
        assert_eq!(resolve(Color::Named(NamedColor::Background), Flags::empty(), false, default, &palette), default);
    }

    #[test]
    fn spec_colors_convert_directly_from_8_bit_channels() {
        let rgb = pane::Rgb { r: 51, g: 102, b: 255 };
        let resolved = resolve(Color::Spec(rgb), Flags::empty(), true, [0.0, 0.0, 0.0], &palette());
        assert!((resolved[0] - 51.0 / 255.0).abs() < 1e-6);
        assert!((resolved[1] - 102.0 / 255.0).abs() < 1e-6);
        assert!((resolved[2] - 255.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn indexed_cube_and_grayscale_ranges_resolve_without_panicking() {
        // 16 = cube's own black corner; 231 = cube's own white corner;
        // 232/255 = grayscale ramp's endpoints. Mostly a guard against an
        // off-by-one in the cube/ramp math panicking or silently
        // producing an out-of-range channel value.
        let palette = palette();
        for index in [16u8, 231, 232, 255] {
            let [r, g, b] = indexed_color(index, &palette);
            assert!((0.0..=1.0).contains(&r) && (0.0..=1.0).contains(&g) && (0.0..=1.0).contains(&b));
        }
    }

    /// The point of the whole theme system: a program's ANSI red has to
    /// actually become the theme's red, not a fixed one.
    #[test]
    fn switching_theme_changes_what_a_named_ansi_color_resolves_to() {
        let default = config::Appearance::default().palette();
        let dracula = config::Appearance { theme: "Dracula".to_string(), ..Default::default() }.palette();

        let red = |palette: &Palette| resolve(Color::Named(NamedColor::Red), Flags::empty(), true, [0.0; 3], palette);
        assert_ne!(red(&default), red(&dracula));
        assert_eq!(red(&dracula), [0xff as f32 / 255.0, 0x55 as f32 / 255.0, 0x55 as f32 / 255.0]);
    }

    /// Indices past 15 are the standard xterm cube/ramp and must stay put
    /// whatever theme is chosen — a program asking for index 200 wants that
    /// exact color, not a themed reinterpretation of it.
    #[test]
    fn the_256_color_cube_is_not_themed() {
        let default = config::Appearance::default().palette();
        let dracula = config::Appearance { theme: "Dracula".to_string(), ..Default::default() }.palette();

        for index in [16u8, 100, 200, 231, 240] {
            assert_eq!(indexed_color(index, &default), indexed_color(index, &dracula), "index {index}");
        }
        // ...while the first 16 do follow the theme.
        assert_ne!(indexed_color(1, &default), indexed_color(1, &dracula));
    }

    #[test]
    fn only_the_default_background_is_reported_as_default() {
        assert!(is_default_background(Color::Named(NamedColor::Background)));
        assert!(!is_default_background(Color::Named(NamedColor::Foreground)));
        assert!(!is_default_background(Color::Named(NamedColor::Red)));
    }
}
