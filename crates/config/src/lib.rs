//! TOML-backed configuration: parsed at startup with defaults for anything
//! missing (see `.waypoint/design/config-system.md`). Hot reload (5.2) and
//! keybinding-override wiring (5.3) build on top of `load`/`Config` here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Directory name under the platform's config root — a working name (the
/// product itself doesn't have a settled one yet, same open state as the
/// theme question in CONOPS §8), picked from this repo's own directory
/// name rather than inventing branding. Revisit if/when the project is
/// actually named.
const APP_NAME: &str = "pain";

#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub appearance: Appearance,
    pub cursor: Cursor,
    /// Chord string (e.g. `"ctrl+shift+e"`) to action name (e.g.
    /// `"split_vertical"`), overriding the built-in Terminator-equivalent
    /// keymap. `BTreeMap` rather than `HashMap` so `Config::save` writes a
    /// stable, deterministically ordered file. `"none"` as the action name
    /// unbinds the chord without a replacement (Milestone 5.3's job to
    /// apply — this struct just carries the data).
    pub keybindings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct General {
    /// Empty means "platform default" ($SHELL, or the user's configured
    /// default shell) — not `Option<String>`, so an explicit empty string
    /// in a hand-edited file round-trips the same way as an absent key.
    pub default_shell: String,
    pub scrollback_lines: usize,
    /// Ask before pasting text that would run more than one command in a
    /// program that hasn't enabled bracketed paste. On by default: without
    /// bracketing, every newline in a paste executes the moment it
    /// arrives, so an unreviewed multi-line paste runs arbitrary commands
    /// with no chance to look at them first.
    pub confirm_multiline_paste: bool,
}

impl Default for General {
    fn default() -> Self {
        General { default_shell: String::new(), scrollback_lines: 5000, confirm_multiline_paste: true }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Appearance {
    /// Named preset — the only shape this can take until the theme/preset
    /// format itself is decided (CONOPS §8, still open); no inline color
    /// table support yet.
    pub theme: String,
    pub font_family: String,
    pub font_size: f32,
    /// 0.0 (fully transparent) – 1.0 (opaque).
    pub transparency: f32,
    /// Terminal background, as `#rrggbb` hex — a near-black slate by
    /// default (the "Graphite" palette), not pure black; a plain color
    /// setting rather than part of the still-open theme system (CONOPS
    /// §8), a real theme replaces this later. Parse failures (a hand-
    /// edited value that isn't valid hex) fall back to this same default
    /// via `background_rgb`, not a load error — consistent with the rest
    /// of this config's "never crash on a bad edit" handling.
    pub background_color: String,
    /// The one accent color used throughout the chrome — cursor, text
    /// selection, focus/interactive highlights in menus and the settings
    /// panel. Deliberately a single user-configurable color rather than a
    /// full theme (CONOPS §8 is still open on that): semantic colors
    /// (e.g. the broadcast-target border) stay fixed regardless, since
    /// they're a distinct signal, not decoration. Same hex format and
    /// same "never crash on a bad edit" fallback convention as
    /// `background_color`.
    pub accent_color: String,
}

/// The "Graphite" palette's own accent (a desaturated slate blue) — the
/// default `accent_color`, and the fallback if a hand-edited value fails
/// to parse.
const DEFAULT_ACCENT_RGB: [f32; 3] = [127.0 / 255.0, 162.0 / 255.0, 214.0 / 255.0];

/// "Graphite" palette's own near-black ground — the default
/// `background_color`, and its own parse-failure fallback.
const DEFAULT_BACKGROUND_RGB: [f32; 3] = [12.0 / 255.0, 14.0 / 255.0, 17.0 / 255.0];

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: "default".to_string(),
            font_family: "monospace".to_string(),
            font_size: 13.0,
            transparency: 1.0,
            background_color: format_hex_rgb(DEFAULT_BACKGROUND_RGB),
            accent_color: format_hex_rgb(DEFAULT_ACCENT_RGB),
        }
    }
}

impl Appearance {
    /// Parses `background_color` into 0.0–1.0 RGB, falling back to the
    /// Graphite default if it isn't valid `#rrggbb` (or `rrggbb`) hex.
    pub fn background_rgb(&self) -> [f32; 3] {
        parse_hex_rgb(&self.background_color).unwrap_or(DEFAULT_BACKGROUND_RGB)
    }

    /// Sets `background_color` from 0.0–1.0 RGB (e.g. from a UI color
    /// picker), formatted as `#rrggbb` — the inverse of `background_rgb`.
    pub fn set_background_rgb(&mut self, rgb: [f32; 3]) {
        self.background_color = format_hex_rgb(rgb);
    }

    /// Parses `accent_color` into 0.0–1.0 RGB, falling back to the
    /// Graphite default if it isn't valid `#rrggbb` (or `rrggbb`) hex.
    pub fn accent_rgb(&self) -> [f32; 3] {
        parse_hex_rgb(&self.accent_color).unwrap_or(DEFAULT_ACCENT_RGB)
    }

    /// Sets `accent_color` from 0.0–1.0 RGB — the inverse of `accent_rgb`.
    pub fn set_accent_rgb(&mut self, rgb: [f32; 3]) {
        self.accent_color = format_hex_rgb(rgb);
    }
}

fn format_hex_rgb(rgb: [f32; 3]) -> String {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", channel(rgb[0]), channel(rgb[1]), channel(rgb[2]))
}

fn parse_hex_rgb(s: &str) -> Option<[f32; 3]> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| -> Option<f32> {
        Some(u8::from_str_radix(hex.get(range)?, 16).ok()? as f32 / 255.0)
    };
    Some([channel(0..2)?, channel(2..4)?, channel(4..6)?])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Cursor {
    pub style: CursorStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Beam,
}

impl Config {
    /// The platform config file path: `$XDG_CONFIG_HOME/pain/config.toml`
    /// (falling back to `~/.config/pain/`) on Linux, `~/Library/Application
    /// Support/pain/config.toml` on macOS, `%APPDATA%\pain\config.toml` on
    /// Windows — per `.waypoint/design/config-system.md`.
    pub fn default_path() -> PathBuf {
        dir().join("config.toml")
    }

    /// Loads config from `path`, falling back to all-defaults on *any*
    /// problem (missing file or unparseable one) and reporting unparseable
    /// ones to stderr. Fine for a first load, where there's no previous
    /// config to fall back to anyway — hot reload (Milestone 5.2) needs to
    /// tell "missing" and "broken" apart instead, since a broken edit
    /// should keep whatever was running, not reset it to defaults; that's
    /// what `try_load` is for.
    pub fn load(path: &Path) -> Config {
        match Self::try_load(path) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("config: failed to parse {}: {err}", path.display());
                Config::default()
            }
        }
    }

    /// Loads config from `path`. A missing (or otherwise unreadable) file
    /// is not an error — `Ok(Config::default())`, exactly as `.waypoint/
    /// design/config-system.md` specifies. A present-but-unparseable file
    /// is `Err`, so a caller doing a hot reload can keep the last-good
    /// config on a bad edit instead of resetting to defaults.
    pub fn try_load(path: &Path) -> Result<Config, toml::de::Error> {
        match std::fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents),
            Err(_) => Ok(Config::default()),
        }
    }

    /// Serializes and writes `self` to `path`, creating its parent
    /// directory first if needed. This is the entire "apply" step for a
    /// settings-panel save — writing the file is all it does; the
    /// already-running hot-reload watcher (Milestone 5.2) picks the change
    /// up exactly the way it would a hand edit, per `.waypoint/design/
    /// config-system.md`'s single-apply-path rule (no separate "apply from
    /// UI" path that could drift out of sync with "apply from file").
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let contents = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, contents)
    }
}

/// The platform config directory itself (`config.toml`'s parent), public
/// so other files this app stores alongside it — the `session` crate's
/// session file — resolve to the same place without duplicating this
/// platform-detection logic.
#[cfg(target_os = "windows")]
pub fn dir() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(appdata) => PathBuf::from(appdata).join(APP_NAME),
        None => {
            eprintln!("config: %APPDATA% is not set; using current directory for config storage");
            PathBuf::from(".").join(APP_NAME)
        }
    }
}

#[cfg(target_os = "macos")]
pub fn dir() -> PathBuf {
    home_dir().join("Library").join("Application Support").join(APP_NAME)
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn dir() -> PathBuf {
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(xdg) => PathBuf::from(xdg).join(APP_NAME),
        None => home_dir().join(".config").join(APP_NAME),
    }
}

#[cfg(not(target_os = "windows"))]
fn home_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => {
            eprintln!("config: $HOME is not set; using current directory for config storage");
            PathBuf::from(".")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn missing_file_loads_all_defaults() {
        let path = PathBuf::from("/nonexistent/definitely/not/a/real/path/config.toml");
        assert_eq!(Config::load(&path), Config::default());
    }

    #[test]
    fn present_file_overrides_only_what_it_sets() {
        let dir = std::env::temp_dir().join(format!("pain-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        write!(file, "[general]\nscrollback_lines = 1234\n").unwrap();
        drop(file);

        let config = Config::load(&path);
        assert_eq!(config.general.scrollback_lines, 1234);
        // Everything not set in the file keeps its default.
        assert_eq!(config.general.default_shell, "");
        assert_eq!(config.appearance, Appearance::default());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("pain-config-test-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "this is not [valid toml").unwrap();

        assert_eq!(Config::load(&path), Config::default());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn keybinding_overrides_parse_as_a_sparse_map() {
        let dir = std::env::temp_dir().join(format!("pain-config-test-kb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[keybindings]\n\"ctrl+shift+e\" = \"split_vertical\"\n").unwrap();

        let config = Config::load(&path);
        assert_eq!(config.keybindings.get("ctrl+shift+e"), Some(&"split_vertical".to_string()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn background_color_parses_hex_with_or_without_hash() {
        assert_eq!(parse_hex_rgb("#ff0080"), Some([1.0, 0.0, 128.0 / 255.0]));
        assert_eq!(parse_hex_rgb("ff0080"), Some([1.0, 0.0, 128.0 / 255.0]));
    }

    #[test]
    fn background_color_falls_back_to_the_graphite_default_when_invalid() {
        assert_eq!(parse_hex_rgb("not-a-color"), None);
        assert_eq!(parse_hex_rgb("#zzzzzz"), None);
        let appearance = Appearance { background_color: "garbage".to_string(), ..Appearance::default() };
        assert_eq!(appearance.background_rgb(), DEFAULT_BACKGROUND_RGB);
    }

    #[test]
    fn set_background_rgb_round_trips_through_hex() {
        let mut appearance = Appearance::default();
        appearance.set_background_rgb([1.0, 0.0, 128.0 / 255.0]);
        assert_eq!(appearance.background_color, "#ff0080");
        assert_eq!(appearance.background_rgb(), [1.0, 0.0, 128.0 / 255.0]);
    }

    #[test]
    fn accent_color_falls_back_to_the_graphite_default_when_invalid() {
        let appearance = Appearance { accent_color: "garbage".to_string(), ..Appearance::default() };
        assert_eq!(appearance.accent_rgb(), DEFAULT_ACCENT_RGB);
    }

    #[test]
    fn set_accent_rgb_round_trips_through_hex() {
        let mut appearance = Appearance::default();
        appearance.set_accent_rgb([0.0, 1.0, 128.0 / 255.0]);
        assert_eq!(appearance.accent_color, "#00ff80");
        assert_eq!(appearance.accent_rgb(), [0.0, 1.0, 128.0 / 255.0]);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("pain-config-test-save-{}", std::process::id()));
        let path = dir.join("nested").join("config.toml");

        let mut config = Config::default();
        config.appearance.font_size = 21.0;
        config.general.default_shell = "/bin/zsh".to_string();
        config.keybindings.insert("ctrl+shift+e".to_string(), "close_pane".to_string());

        config.save(&path).expect("save should create parent dirs and write the file");
        assert_eq!(Config::load(&path), config);

        std::fs::remove_dir_all(&dir).ok();
    }
}
