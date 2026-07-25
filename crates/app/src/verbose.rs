//! Opt-in diagnostic logging, enabled via `--verbose`/`-v`.
//!
//! A plain atomic rather than a threaded-through flag: the PTY reader runs
//! on its own background thread and needs to check this too.
//!
//! Gated per-category, not by one blanket flag: a few streams (every mouse
//! motion event, every raw PTY byte chunk, a foreground-process scan line
//! every ~500ms per pane) fire constantly and drown out everything else —
//! low-frequency, structural events like startup info, config reloads, and
//! shell spawn/exit — the moment they're all on together. Bare `--verbose`
//! enables just the low-frequency tier (`General`); the noisy ones are each
//! an explicit opt-in via `--verbose=<name>[,<name>...]`.

use std::sync::atomic::{AtomicU8, Ordering};

/// A diagnostic stream, gated independently of the others.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Low-frequency, always worth seeing: startup info, config load/
    /// reload, shell spawn/exit. What bare `--verbose` enables.
    General,
    /// Every mouse event: motion, clicks, drags, wheel. Fires continuously
    /// while the mouse moves at all — `--verbose=mouse`.
    Mouse,
    /// Raw PTY reads/writes: every byte chunk read, every keystroke
    /// written. Fires continuously while anything runs in the shell —
    /// `--verbose=pty`.
    Pty,
    /// The foreground-process scan line, once per pane every ~500ms for as
    /// long as the app runs — `--verbose=foreground`.
    Foreground,
}

const GENERAL: u8 = 1 << 0;
const MOUSE: u8 = 1 << 1;
const PTY: u8 = 1 << 2;
const FOREGROUND: u8 = 1 << 3;

impl Category {
    fn bit(self) -> u8 {
        match self {
            Category::General => GENERAL,
            Category::Mouse => MOUSE,
            Category::Pty => PTY,
            Category::Foreground => FOREGROUND,
        }
    }
}

static ENABLED: AtomicU8 = AtomicU8::new(0);

/// Parses `--verbose`/`-v`'s value (`None` for the bare flag) and enables
/// the resulting categories for the rest of the process's lifetime.
pub fn set_verbose(value: Option<&str>) {
    ENABLED.store(parse_mask(value), Ordering::Relaxed);
}

/// A bare flag enables just `General`; an explicit value is a comma-
/// separated list of `general`/`mouse`/`pty`/`foreground`/`all`. An
/// unrecognized name is ignored, not rejected outright — a typo in a
/// diagnostic flag shouldn't stop the app from starting.
fn parse_mask(value: Option<&str>) -> u8 {
    match value {
        None => GENERAL,
        Some(value) => value.split(',').fold(0, |mask, name| mask | category_bit(name.trim())),
    }
}

fn category_bit(name: &str) -> u8 {
    match name {
        "all" => GENERAL | MOUSE | PTY | FOREGROUND,
        "general" => GENERAL,
        "mouse" => MOUSE,
        "pty" => PTY,
        "foreground" => FOREGROUND,
        _ => 0,
    }
}

/// Whether `category`'s diagnostic output is enabled.
pub fn is_verbose(category: Category) -> bool {
    ENABLED.load(Ordering::Relaxed) & category.bit() != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_flag_enables_only_general() {
        let mask = parse_mask(None);
        assert_eq!(mask, GENERAL);
    }

    #[test]
    fn explicit_categories_enable_only_those_named() {
        let mask = parse_mask(Some("mouse, pty"));
        assert_eq!(mask, MOUSE | PTY);
    }

    #[test]
    fn all_enables_every_category() {
        let mask = parse_mask(Some("all"));
        assert_eq!(mask, GENERAL | MOUSE | PTY | FOREGROUND);
    }

    #[test]
    fn unrecognized_names_are_ignored_not_rejected() {
        assert_eq!(parse_mask(Some("bogus")), 0);
        assert_eq!(parse_mask(Some("mouse,bogus")), MOUSE);
    }
}
