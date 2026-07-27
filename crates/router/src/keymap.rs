//! Keyboard chords and the app-level actions they map to.

use std::collections::HashMap;

use layout::{Direction, Orientation};

use crate::BroadcastMode;

/// A named key, independent of platform-specific virtual key codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// A single lowercase ASCII letter or digit.
    Char(char),
    Up,
    Down,
    Left,
    Right,
}

/// A keyboard chord: a key plus the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// The "Super"/Cmd/Windows key.
    pub logo: bool,
}

impl Chord {
    pub fn new(key: Key) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: false,
            logo: false,
        }
    }

    pub fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub fn logo(mut self) -> Self {
        self.logo = true;
        self
    }
}

/// App-level actions a chord can be bound to. A key mapped to one of these
/// never passes through to the pane — chord or passthrough, never both
/// (`.waypoint/design/input-router.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Split(Orientation),
    ClosePane,
    Quit,
    Focus(Direction),
    Resize(Direction),
    ToggleZoom,
    SetBroadcastMode(BroadcastMode),
    /// Copy the focused pane's current selection to the system clipboard.
    Copy,
    /// Copy the focused pane's selection if it has one, and otherwise send
    /// an interrupt (`0x03`) to the shell. What plain `Ctrl+C` is bound to:
    /// the chord keeps its decades-old meaning whenever there's nothing to
    /// copy, so binding it costs nobody their interrupt key. Distinct from
    /// `Copy` because the fallback only makes sense for a chord the
    /// terminal itself would otherwise have claimed.
    CopyOrInterrupt,
    /// Paste the system clipboard into the focused pane.
    Paste,
}

impl Action {
    /// The name this action goes by in `[keybindings]` — the exact inverse
    /// of `parse_action`, so anything listed in the docs or the settings
    /// panel is a string a user can paste straight back into their config.
    pub fn name(self) -> &'static str {
        match self {
            Action::Split(Orientation::Horizontal) => "split_horizontal",
            Action::Split(Orientation::Vertical) => "split_vertical",
            Action::ClosePane => "close_pane",
            Action::Quit => "quit",
            Action::Focus(Direction::Up) => "focus_up",
            Action::Focus(Direction::Down) => "focus_down",
            Action::Focus(Direction::Left) => "focus_left",
            Action::Focus(Direction::Right) => "focus_right",
            Action::Resize(Direction::Up) => "resize_up",
            Action::Resize(Direction::Down) => "resize_down",
            Action::Resize(Direction::Left) => "resize_left",
            Action::Resize(Direction::Right) => "resize_right",
            Action::ToggleZoom => "toggle_zoom",
            Action::SetBroadcastMode(BroadcastMode::Off) => "broadcast_off",
            Action::SetBroadcastMode(BroadcastMode::Group) => "broadcast_group",
            Action::SetBroadcastMode(BroadcastMode::All) => "broadcast_all",
            Action::Copy => "copy",
            Action::CopyOrInterrupt => "copy_or_interrupt",
            Action::Paste => "paste",
        }
    }
}

/// Renders a chord the same way `parse_chord` reads one, so a binding shown
/// to a user is always one they can paste back into `[keybindings]`
/// verbatim. `logo` prints as `cmd` because the only default bindings using
/// it are macOS's, and that's the name on the key there.
impl std::fmt::Display for Chord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (active, name) in
            [(self.ctrl, "ctrl"), (self.shift, "shift"), (self.alt, "alt"), (self.logo, "cmd")]
        {
            if active {
                write!(f, "{name}+")?;
            }
        }
        match self.key {
            Key::Char(c) => write!(f, "{c}"),
            Key::Up => write!(f, "up"),
            Key::Down => write!(f, "down"),
            Key::Left => write!(f, "left"),
            Key::Right => write!(f, "right"),
        }
    }
}

/// A remappable table of chord -> action bindings.
pub struct Keymap {
    bindings: HashMap<Chord, Action>,
}

impl Keymap {
    pub fn empty() -> Self {
        Self { bindings: HashMap::new() }
    }

    /// Terminator's current documented default bindings, verified directly
    /// against its `config.py` source (see `.waypoint/design/input-router.md`)
    /// for every action Terminator itself supports.
    ///
    /// Grouping and broadcast-mode selection are deliberately *not* bound
    /// here at all — they're driven by the UI overlay (`crate::ui` in the
    /// app crate) instead. Terminator itself only assigns pane groups
    /// through its GUI, never a keybinding, and our own first attempt at
    /// chords for these (Ctrl+Shift+G, and Terminator's own Super+G/
    /// Super+T for broadcast mode) ran into the Windows key being too
    /// deeply OS-reserved to be a safe default. Group assignment now also
    /// needs a group *name*, which isn't something a keyboard chord can
    /// carry at all — `Router::assign_to_group`/`remove_from_group` have
    /// no `Action` variant or default chord, full stop, not just no
    /// default one. `Action::SetBroadcastMode` remains independently
    /// bindable — a future config could remap a chord to it — there's just
    /// no default chord for it right now.
    pub fn terminator_defaults() -> Self {
        // `cfg!` rather than `#[cfg]` so both platforms' binding sets always
        // compile and stay testable from any host — a macOS-only chord table
        // that only type-checks on a Mac is one nobody would notice breaking.
        Self::defaults_for_platform(cfg!(target_os = "macos"))
    }

    /// The default bindings, for a platform that either does or doesn't use
    /// Command as its clipboard modifier. See `terminator_defaults`.
    pub fn defaults_for_platform(is_macos: bool) -> Self {
        let mut keymap = Self::empty();

        keymap.bind(
            Chord::new(Key::Char('o')).ctrl().shift(),
            Action::Split(Orientation::Horizontal),
        );
        keymap.bind(
            Chord::new(Key::Char('e')).ctrl().shift(),
            Action::Split(Orientation::Vertical),
        );
        keymap.bind(Chord::new(Key::Char('w')).ctrl().shift(), Action::ClosePane);
        keymap.bind(Chord::new(Key::Char('q')).ctrl().shift(), Action::Quit);

        keymap.bind(Chord::new(Key::Up).alt(), Action::Focus(Direction::Up));
        keymap.bind(Chord::new(Key::Down).alt(), Action::Focus(Direction::Down));
        keymap.bind(Chord::new(Key::Left).alt(), Action::Focus(Direction::Left));
        keymap.bind(Chord::new(Key::Right).alt(), Action::Focus(Direction::Right));

        keymap.bind(Chord::new(Key::Up).ctrl().shift(), Action::Resize(Direction::Up));
        keymap.bind(Chord::new(Key::Down).ctrl().shift(), Action::Resize(Direction::Down));
        keymap.bind(Chord::new(Key::Left).ctrl().shift(), Action::Resize(Direction::Left));
        keymap.bind(Chord::new(Key::Right).ctrl().shift(), Action::Resize(Direction::Right));

        keymap.bind(Chord::new(Key::Char('x')).ctrl().shift(), Action::ToggleZoom);

        // Ctrl+Shift+C/V, the usual Linux-terminal clipboard chords, are
        // deliberately *not* bound. They only ever existed as a workaround
        // for the unshifted pair being unavailable, and on both platforms
        // below that's no longer true — so binding them would mean two
        // chords for one action, and one of them the awkward one. Anyone
        // with the muscle memory can bind them back in `[keybindings]`.
        if is_macos {
            // macOS has a dedicated clipboard modifier that the terminal
            // has never wanted, so Ctrl is left completely alone here:
            // Ctrl+C stays pure SIGINT and Ctrl+V stays literal-next, with
            // no conditional behavior to reason about. Copy needs no
            // interrupt fallback for the same reason.
            keymap.bind(Chord::new(Key::Char('c')).logo(), Action::Copy);
            keymap.bind(Chord::new(Key::Char('v')).logo(), Action::Paste);
            // Cmd+Q and Cmd+W are close to reflexive on macOS; their
            // absence reads as the app being broken rather than as a
            // missing convenience.
            keymap.bind(Chord::new(Key::Char('q')).logo(), Action::Quit);
            keymap.bind(Chord::new(Key::Char('w')).logo(), Action::ClosePane);
        } else {
            // Elsewhere there's no such modifier, so the clipboard has to
            // live on Ctrl. Ctrl+C gives up nothing — it still interrupts
            // whenever there's no selection, see `CopyOrInterrupt`. Ctrl+V
            // genuinely does displace readline's `quoted-insert`; that's a
            // deliberate trade, since inserting a literal control character
            // is rare next to pasting, and `"ctrl+v" = "none"` in config
            // restores it.
            keymap.bind(Chord::new(Key::Char('c')).ctrl(), Action::CopyOrInterrupt);
            keymap.bind(Chord::new(Key::Char('v')).ctrl(), Action::Paste);
        }

        keymap
    }

    pub fn bind(&mut self, chord: Chord, action: Action) {
        self.bindings.insert(chord, action);
    }

    pub fn unbind(&mut self, chord: Chord) {
        self.bindings.remove(&chord);
    }

    pub fn lookup(&self, chord: Chord) -> Option<Action> {
        self.bindings.get(&chord).copied()
    }

    /// Every binding in the table, sorted by how it prints. Sorted because
    /// the backing map has no order of its own, and both consumers — the
    /// settings panel's read-only list and the docs — need a stable one.
    pub fn bindings(&self) -> Vec<(Chord, Action)> {
        let mut bindings: Vec<(Chord, Action)> = self.bindings.iter().map(|(c, a)| (*c, *a)).collect();
        bindings.sort_by_key(|(chord, _)| chord.to_string());
        bindings
    }

    /// Layers config-file overrides (chord string -> action name, e.g.
    /// `"ctrl+shift+e" -> "split_vertical"`) onto this keymap — see
    /// `.waypoint/design/config-system.md`'s `[keybindings]` schema. An
    /// action name of `"none"` unbinds the chord without a replacement.
    /// An unparseable chord or unrecognized action name is reported to
    /// stderr and skipped, not treated as fatal — one bad line in a
    /// hand-edited config shouldn't take out every other override, the
    /// same "never crash on a bad edit" rule the rest of the config system
    /// follows. Callers apply this on top of a fresh `terminator_defaults()`
    /// each time (not incrementally), so a removed override reverts its
    /// chord to the built-in default on the next reload rather than
    /// staying stuck at a stale rebinding.
    pub fn apply_overrides(&mut self, overrides: &std::collections::BTreeMap<String, String>) {
        for (chord_str, action_str) in overrides {
            let Some(chord) = parse_chord(chord_str) else {
                eprintln!("keymap: unrecognized chord {chord_str:?}, skipping");
                continue;
            };

            if action_str == "none" {
                self.unbind(chord);
                continue;
            }

            let Some(action) = parse_action(action_str) else {
                eprintln!("keymap: unrecognized action {action_str:?}, skipping");
                continue;
            };
            self.bind(chord, action);
        }
    }
}

/// Parses a chord string like `"ctrl+shift+e"` (case-insensitive,
/// `+`-separated, modifiers in any order, exactly one non-modifier segment
/// — a single character or an arrow-key name). `logo`/`super`/`cmd`/`win`
/// all mean the same modifier: a user can still choose to bind it
/// themselves even though no *default* binding uses it (see
/// `terminator_defaults`'s doc comment for why we don't ship one).
fn parse_chord(s: &str) -> Option<Chord> {
    let (mut ctrl, mut shift, mut alt, mut logo) = (false, false, false, false);
    let mut key: Option<Key> = None;

    for part in s.split('+') {
        let part = part.trim().to_ascii_lowercase();
        let parsed_key = match part.as_str() {
            "ctrl" | "control" => {
                ctrl = true;
                continue;
            }
            "shift" => {
                shift = true;
                continue;
            }
            "alt" => {
                alt = true;
                continue;
            }
            "logo" | "super" | "cmd" | "win" | "windows" => {
                logo = true;
                continue;
            }
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            other => {
                let mut chars = other.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                Key::Char(c)
            }
        };

        if key.is_some() {
            return None; // more than one non-modifier segment
        }
        key = Some(parsed_key);
    }

    Some(Chord { key: key?, ctrl, shift, alt, logo })
}

/// Parses an action name as it appears in `[keybindings]` — the same set
/// `terminator_defaults` binds, plus the broadcast-mode actions that have
/// no default chord but remain independently bindable. Group assignment
/// isn't in this list at all — it needs a group name a chord can't carry.
fn parse_action(s: &str) -> Option<Action> {
    Some(match s {
        "split_horizontal" => Action::Split(Orientation::Horizontal),
        "split_vertical" => Action::Split(Orientation::Vertical),
        "close_pane" => Action::ClosePane,
        "quit" => Action::Quit,
        "focus_up" => Action::Focus(Direction::Up),
        "focus_down" => Action::Focus(Direction::Down),
        "focus_left" => Action::Focus(Direction::Left),
        "focus_right" => Action::Focus(Direction::Right),
        "resize_up" => Action::Resize(Direction::Up),
        "resize_down" => Action::Resize(Direction::Down),
        "resize_left" => Action::Resize(Direction::Left),
        "resize_right" => Action::Resize(Direction::Right),
        "toggle_zoom" => Action::ToggleZoom,
        "broadcast_off" => Action::SetBroadcastMode(BroadcastMode::Off),
        "broadcast_group" => Action::SetBroadcastMode(BroadcastMode::Group),
        "broadcast_all" => Action::SetBroadcastMode(BroadcastMode::All),
        "copy" => Action::Copy,
        "copy_or_interrupt" => Action::CopyOrInterrupt,
        "paste" => Action::Paste,
        _ => return None,
    })
}

impl Default for Keymap {
    fn default() -> Self {
        Self::terminator_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminator_defaults_bind_the_core_actions() {
        let keymap = Keymap::terminator_defaults();

        assert_eq!(
            keymap.lookup(Chord::new(Key::Char('o')).ctrl().shift()),
            Some(Action::Split(Orientation::Horizontal))
        );
        assert_eq!(
            keymap.lookup(Chord::new(Key::Char('e')).ctrl().shift()),
            Some(Action::Split(Orientation::Vertical))
        );
        assert_eq!(keymap.lookup(Chord::new(Key::Char('w')).ctrl().shift()), Some(Action::ClosePane));
        assert_eq!(keymap.lookup(Chord::new(Key::Char('q')).ctrl().shift()), Some(Action::Quit));
        assert_eq!(keymap.lookup(Chord::new(Key::Up).alt()), Some(Action::Focus(Direction::Up)));
        assert_eq!(keymap.lookup(Chord::new(Key::Char('x')).ctrl().shift()), Some(Action::ToggleZoom));
    }

    /// Ctrl+Shift+C/V exist only to work around the unshifted pair being
    /// unavailable, which isn't the situation on either platform any more.
    /// Leaving them bound would give one action two chords, so they're
    /// gone — asserted rather than merely deleted, since "we stopped
    /// binding this on purpose" is exactly the kind of decision that gets
    /// silently undone later.
    #[test]
    fn the_shifted_clipboard_chords_are_not_bound_on_any_platform() {
        for is_macos in [false, true] {
            let keymap = Keymap::defaults_for_platform(is_macos);
            assert_eq!(
                keymap.lookup(Chord::new(Key::Char('c')).ctrl().shift()),
                None,
                "ctrl+shift+c, is_macos={is_macos}"
            );
            assert_eq!(
                keymap.lookup(Chord::new(Key::Char('v')).ctrl().shift()),
                None,
                "ctrl+shift+v, is_macos={is_macos}"
            );
        }
    }

    #[test]
    fn off_macos_plain_ctrl_c_and_v_are_the_clipboard_chords() {
        let keymap = Keymap::defaults_for_platform(false);

        // Ctrl+C only reaches the clipboard when there's a selection; with
        // none it still interrupts, which is why binding it is safe at all.
        assert_eq!(keymap.lookup(Chord::new(Key::Char('c')).ctrl()), Some(Action::CopyOrInterrupt));
        assert_eq!(keymap.lookup(Chord::new(Key::Char('v')).ctrl()), Some(Action::Paste));

        // Command isn't a modifier anyone presses here, and binding it
        // would collide with the OS on Windows.
        assert_eq!(keymap.lookup(Chord::new(Key::Char('c')).logo()), None);
        assert_eq!(keymap.lookup(Chord::new(Key::Char('v')).logo()), None);
    }

    #[test]
    fn macos_uses_command_and_leaves_the_control_key_untouched() {
        let keymap = Keymap::defaults_for_platform(true);

        assert_eq!(keymap.lookup(Chord::new(Key::Char('c')).logo()), Some(Action::Copy));
        assert_eq!(keymap.lookup(Chord::new(Key::Char('v')).logo()), Some(Action::Paste));
        assert_eq!(keymap.lookup(Chord::new(Key::Char('q')).logo()), Some(Action::Quit));
        assert_eq!(keymap.lookup(Chord::new(Key::Char('w')).logo()), Some(Action::ClosePane));

        // The whole point of having Command: Ctrl+C stays pure SIGINT and
        // Ctrl+V stays readline's literal-next, with nothing conditional
        // about either. Unbound here means "pass through to the shell".
        assert_eq!(keymap.lookup(Chord::new(Key::Char('c')).ctrl()), None);
        assert_eq!(keymap.lookup(Chord::new(Key::Char('v')).ctrl()), None);
    }

    /// `Display`/`name` exist so the settings panel and the docs can show
    /// bindings, and both are only useful if what they print is what a user
    /// can paste back into `[keybindings]`. Asserting the round trip over
    /// every real default is what makes that a guarantee rather than a
    /// hope — a new chord or action that doesn't survive it fails here.
    #[test]
    fn every_default_binding_prints_as_something_config_can_parse_back() {
        for is_macos in [false, true] {
            for (chord, action) in Keymap::defaults_for_platform(is_macos).bindings() {
                let printed = chord.to_string();
                assert_eq!(parse_chord(&printed), Some(chord), "chord {printed:?} round trip");
                assert_eq!(parse_action(action.name()), Some(action), "action {:?}", action.name());
            }
        }
    }

    /// A shortcut table rots the moment someone adds a binding and forgets
    /// the docs, and nothing about a passing build would ever say so. This
    /// checks the README actually mentions every default chord and every
    /// bindable action name, on both platforms — the cheapest available
    /// guard against documentation that quietly stops being true.
    ///
    /// Deliberately a containment check, not a parse of the tables: the
    /// prose is free to explain and group bindings however reads best, and
    /// a test that dictated formatting would just get deleted.
    #[test]
    fn the_readme_documents_every_default_binding_and_action() {
        let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"))
            .expect("README.md sits at the workspace root")
            .to_ascii_lowercase();

        for is_macos in [false, true] {
            for (chord, action) in Keymap::defaults_for_platform(is_macos).bindings() {
                let chord = chord.to_string();
                assert!(readme.contains(&chord), "README doesn't mention the {chord:?} shortcut");
                assert!(
                    readme.contains(action.name()),
                    "README doesn't mention the {:?} action",
                    action.name()
                );
            }
        }

        // The actions with no default chord are reachable only by binding
        // them by hand, which makes documenting them the *only* way anyone
        // finds out they exist.
        for unbound in ["broadcast_off", "broadcast_group", "broadcast_all", "none"] {
            assert!(readme.contains(unbound), "README doesn't mention the {unbound:?} action");
        }
    }

    /// Anyone who wants `quoted-insert` back, or who wants the Ctrl+C
    /// fallback on a platform that doesn't ship it, goes through config.
    #[test]
    fn the_new_clipboard_chords_can_be_overridden_and_unbound() {
        let mut keymap = Keymap::defaults_for_platform(false);
        keymap.apply_overrides(&std::collections::BTreeMap::from([
            ("ctrl+v".to_string(), "none".to_string()),
            ("ctrl+shift+c".to_string(), "copy_or_interrupt".to_string()),
        ]));

        assert_eq!(keymap.lookup(Chord::new(Key::Char('v')).ctrl()), None);
        assert_eq!(
            keymap.lookup(Chord::new(Key::Char('c')).ctrl().shift()),
            Some(Action::CopyOrInterrupt)
        );
    }

    #[test]
    fn unbound_chord_resolves_to_none() {
        let keymap = Keymap::terminator_defaults();
        assert_eq!(keymap.lookup(Chord::new(Key::Char('k')).ctrl()), None);
    }

    #[test]
    fn rebinding_a_chord_replaces_the_previous_action() {
        let mut keymap = Keymap::empty();
        let chord = Chord::new(Key::Char('e')).ctrl().shift();
        keymap.bind(chord, Action::Split(Orientation::Vertical));
        keymap.bind(chord, Action::ClosePane);
        assert_eq!(keymap.lookup(chord), Some(Action::ClosePane));
    }

    #[test]
    fn unbind_removes_a_binding() {
        let mut keymap = Keymap::terminator_defaults();
        let chord = Chord::new(Key::Char('w')).ctrl().shift();
        keymap.unbind(chord);
        assert_eq!(keymap.lookup(chord), None);
    }

    #[test]
    fn override_rebinds_a_chord() {
        let mut keymap = Keymap::terminator_defaults();
        let overrides = std::collections::BTreeMap::from([
            ("ctrl+shift+e".to_string(), "close_pane".to_string()),
        ]);
        keymap.apply_overrides(&overrides);
        assert_eq!(
            keymap.lookup(Chord::new(Key::Char('e')).ctrl().shift()),
            Some(Action::ClosePane)
        );
    }

    #[test]
    fn override_of_none_unbinds_without_a_replacement() {
        let mut keymap = Keymap::terminator_defaults();
        let overrides =
            std::collections::BTreeMap::from([("ctrl+shift+w".to_string(), "none".to_string())]);
        keymap.apply_overrides(&overrides);
        assert_eq!(keymap.lookup(Chord::new(Key::Char('w')).ctrl().shift()), None);
    }

    #[test]
    fn override_can_bind_a_previously_unbound_action() {
        let mut keymap = Keymap::terminator_defaults();
        let overrides = std::collections::BTreeMap::from([
            ("ctrl+shift+g".to_string(), "broadcast_all".to_string()),
        ]);
        keymap.apply_overrides(&overrides);
        assert_eq!(
            keymap.lookup(Chord::new(Key::Char('g')).ctrl().shift()),
            Some(Action::SetBroadcastMode(BroadcastMode::All))
        );
    }

    #[test]
    fn override_with_unparseable_chord_is_skipped_not_fatal() {
        let mut keymap = Keymap::terminator_defaults();
        let overrides = std::collections::BTreeMap::from([
            ("not a chord".to_string(), "quit".to_string()),
            ("ctrl+shift+q".to_string(), "close_pane".to_string()),
        ]);
        keymap.apply_overrides(&overrides);
        // The malformed entry didn't stop the well-formed one after it.
        assert_eq!(
            keymap.lookup(Chord::new(Key::Char('q')).ctrl().shift()),
            Some(Action::ClosePane)
        );
    }

    #[test]
    fn override_with_unknown_action_is_skipped_not_fatal() {
        let mut keymap = Keymap::terminator_defaults();
        let overrides = std::collections::BTreeMap::from([(
            "ctrl+shift+e".to_string(),
            "not_a_real_action".to_string(),
        )]);
        keymap.apply_overrides(&overrides);
        // Unrecognized action left the original binding in place.
        assert_eq!(
            keymap.lookup(Chord::new(Key::Char('e')).ctrl().shift()),
            Some(Action::Split(Orientation::Vertical))
        );
    }

    #[test]
    fn chord_modifiers_parse_in_any_order_case_insensitively() {
        let mut keymap = Keymap::empty();
        let overrides = std::collections::BTreeMap::from([(
            "Shift+CTRL+e".to_string(),
            "quit".to_string(),
        )]);
        keymap.apply_overrides(&overrides);
        assert_eq!(keymap.lookup(Chord::new(Key::Char('e')).ctrl().shift()), Some(Action::Quit));
    }
}
