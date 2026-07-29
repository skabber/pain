//! Encodes mouse events as the escape sequences a shell program (vim, htop,
//! tmux, ...) expects once it has turned on mouse reporting via `CSI ?
//! 100{0,2,3} h` — separate from `crates/router`'s keyboard chords, since
//! this is data the *program inside the pane* asks for, not a binding the
//! user configures.

use pane::TermMode;

// `Middle`/`Right` aren't constructed by `main.rs` yet — right-click is
// reserved for the local pane context menu (never forwarded, matching every
// other terminal emulator's convention), and middle-click isn't wired to
// anything in this milestone. Both stay part of the enum since the SGR/
// normal-tracking encoding needs a real button number regardless of which
// ones this app currently triggers, and both are exercised by the tests
// below.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Press,
    Release,
    Motion,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// Whether `mode` currently wants a report for this kind of event. Press and
/// release are reported by any of the three tracking modes a program can
/// enable (1000 normal, 1002 button-event, 1003 any-event — collapsed by
/// `alacritty_terminal` into the combined `MOUSE_MODE` flag). Motion is
/// narrower: only button-event mode while a button is actually held, or
/// any-event mode unconditionally.
pub fn wants_report(mode: TermMode, kind: Kind, button_held: bool) -> bool {
    match kind {
        Kind::Press | Kind::Release => mode.intersects(TermMode::MOUSE_MODE),
        Kind::Motion => mode.contains(TermMode::MOUSE_MOTION) || (button_held && mode.contains(TermMode::MOUSE_DRAG)),
    }
}

fn button_bits(button: Button) -> u8 {
    match button {
        Button::Left => 0,
        Button::Middle => 1,
        Button::Right => 2,
    }
}

fn modifier_bits(m: Modifiers) -> u8 {
    (m.shift as u8) << 2 | (m.alt as u8) << 3 | (m.ctrl as u8) << 4
}

/// Encodes a mouse event at 0-indexed grid `col`/`row` for the PTY, in SGR
/// format (mode 1006) when the program has asked for it, otherwise xterm's
/// older normal-tracking format — which packs each coordinate into a single
/// byte offset by 32, so it can only address up to column/row 223.
pub fn encode(mode: TermMode, kind: Kind, button: Button, col: usize, row: usize, modifiers: Modifiers) -> Vec<u8> {
    let motion_bit = if kind == Kind::Motion { 0x20 } else { 0 };
    let cb = button_bits(button) | modifier_bits(modifiers) | motion_bit;

    if mode.contains(TermMode::SGR_MOUSE) {
        let final_byte = if kind == Kind::Release { 'm' } else { 'M' };
        format!("\x1b[<{};{};{}{}", cb, col + 1, row + 1, final_byte).into_bytes()
    } else {
        // Normal tracking has no separate release-button field — xterm
        // always reports button 3 ("released") regardless of which button
        // triggered it.
        let cb = if kind == Kind::Release { 3 | modifier_bits(modifiers) } else { cb };
        let coord = |v: usize| -> u8 { (v + 1).min(223) as u8 + 32 };
        vec![0x1b, b'[', b'M', cb + 32, coord(col), coord(row)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_press_and_release_use_distinct_final_bytes() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let press = encode(mode, Kind::Press, Button::Left, 4, 2, Modifiers::default());
        let release = encode(mode, Kind::Release, Button::Left, 4, 2, Modifiers::default());
        assert_eq!(press, b"\x1b[<0;5;3M");
        assert_eq!(release, b"\x1b[<0;5;3m");
    }

    #[test]
    fn sgr_motion_sets_the_motion_bit() {
        let mode = TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE;
        let motion = encode(mode, Kind::Motion, Button::Left, 0, 0, Modifiers::default());
        assert_eq!(motion, b"\x1b[<32;1;1M");
    }

    #[test]
    fn normal_tracking_offsets_coordinates_by_32() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        let press = encode(mode, Kind::Press, Button::Right, 0, 0, Modifiers::default());
        assert_eq!(press, vec![0x1b, b'[', b'M', 32 + 2, 33, 33]);
    }

    #[test]
    fn normal_tracking_caps_large_coordinates() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        let press = encode(mode, Kind::Press, Button::Left, 9999, 9999, Modifiers::default());
        assert_eq!(press[4], 223 + 32);
        assert_eq!(press[5], 223 + 32);
    }

    #[test]
    fn wants_report_reflects_the_active_tracking_mode() {
        assert!(wants_report(TermMode::MOUSE_REPORT_CLICK, Kind::Press, false));
        assert!(!wants_report(TermMode::empty(), Kind::Press, false));

        assert!(wants_report(TermMode::MOUSE_DRAG, Kind::Motion, true));
        assert!(!wants_report(TermMode::MOUSE_DRAG, Kind::Motion, false));

        assert!(wants_report(TermMode::MOUSE_MOTION, Kind::Motion, false));
    }
}
