//! Resolves a terminal cell's ANSI color (a named slot, a 256-index, or a
//! direct RGB spec — `pane::Color`, straight off `alacritty_terminal`'s
//! grid) into a concrete RGB the renderer can use.
//!
//! No theme system exists yet (CONOPS §8 — still an open, deliberately
//! deferred decision), so the 16 base ANSI colors and the 256-color cube/
//! ramp below are the one built-in default, same standard values every
//! xterm-descended terminal ships with. `Color::Named(Foreground/
//! Background)` are the two colors this *does* know how to theme — they
//! resolve to whatever the app's own config says, so cells left at their
//! default color stay consistent with the rest of the chrome (and with
//! transparency/background-color settings) instead of being pinned to a
//! fixed value themselves.

use pane::{Color, Flags, NamedColor};

/// The 16 base ANSI colors (0-7 normal, 8-15 bright), xterm's own defaults.
const ANSI_PALETTE: [[f32; 3]; 16] = [
    [0.0, 0.0, 0.0],
    [0.804, 0.0, 0.0],
    [0.0, 0.804, 0.0],
    [0.804, 0.804, 0.0],
    [0.0, 0.0, 0.933],
    [0.804, 0.0, 0.804],
    [0.0, 0.804, 0.804],
    [0.898, 0.898, 0.898],
    [0.298, 0.298, 0.298],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.361, 0.361, 1.0],
    [1.0, 0.0, 1.0],
    [0.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
];

/// Resolves a 256-color palette index (0-255): 0-15 are the base ANSI
/// colors above, 16-231 a 6x6x6 color cube, 232-255 a 24-step grayscale
/// ramp — the standard xterm 256-color layout.
fn indexed_color(index: u8) -> [f32; 3] {
    match index {
        0..=15 => ANSI_PALETTE[index as usize],
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
pub fn resolve(color: Color, flags: Flags, bold_brightens: bool, default: [f32; 3]) -> [f32; 3] {
    match color {
        Color::Named(NamedColor::Foreground) | Color::Named(NamedColor::Background) => default,
        Color::Named(named) => {
            let index = named as usize;
            if index >= 16 {
                return default;
            }
            if bold_brightens && flags.contains(Flags::BOLD) && index < 8 {
                ANSI_PALETTE[index + 8]
            } else {
                ANSI_PALETTE[index]
            }
        }
        Color::Indexed(index) => indexed_color(index),
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

    #[test]
    fn named_ansi_colors_resolve_to_the_xterm_default_palette() {
        assert_eq!(resolve(Color::Named(NamedColor::Red), Flags::empty(), true, [1.0, 1.0, 1.0]), ANSI_PALETTE[1]);
    }

    #[test]
    fn bold_brightens_a_base_color_for_foreground_but_not_background() {
        let bold = Flags::BOLD;
        assert_eq!(resolve(Color::Named(NamedColor::Red), bold, true, [0.0, 0.0, 0.0]), ANSI_PALETTE[9]);
        assert_eq!(resolve(Color::Named(NamedColor::Red), bold, false, [0.0, 0.0, 0.0]), ANSI_PALETTE[1]);
    }

    #[test]
    fn default_foreground_and_background_fall_back_to_the_given_default() {
        let default = [0.5, 0.25, 0.75];
        assert_eq!(resolve(Color::Named(NamedColor::Foreground), Flags::empty(), true, default), default);
        assert_eq!(resolve(Color::Named(NamedColor::Background), Flags::empty(), false, default), default);
    }

    #[test]
    fn spec_colors_convert_directly_from_8_bit_channels() {
        let rgb = pane::Rgb { r: 51, g: 102, b: 255 };
        let resolved = resolve(Color::Spec(rgb), Flags::empty(), true, [0.0, 0.0, 0.0]);
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
        for index in [16u8, 231, 232, 255] {
            let [r, g, b] = indexed_color(index);
            assert!((0.0..=1.0).contains(&r) && (0.0..=1.0).contains(&g) && (0.0..=1.0).contains(&b));
        }
    }

    #[test]
    fn only_the_default_background_is_reported_as_default() {
        assert!(is_default_background(Color::Named(NamedColor::Background)));
        assert!(!is_default_background(Color::Named(NamedColor::Foreground)));
        assert!(!is_default_background(Color::Named(NamedColor::Red)));
    }
}
