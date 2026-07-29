//! TOML-backed configuration: parsed at startup with defaults for anything
//! missing (see `.waypoint/design/config-system.md`). Hot reload (5.2) and
//! keybinding-override wiring (5.3) build on top of `load`/`Config` here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod themes;

/// Directory name under the platform's config root — a working name (the
/// product itself doesn't have a settled one yet, same open state as the
/// theme question in CONOPS §8), picked from this repo's own directory
/// name rather than inventing branding. Revisit if/when the project is
/// actually named.
const APP_NAME: &str = "pain";

/// Bounds every numeric appearance value is forced into by
/// [`Config::sanitize`]. These match the settings panel's own slider ranges,
/// so a hand-edited file can't reach a state the UI would never produce.
///
/// The font-size bounds are not cosmetic. `render::measure_cell` builds a
/// `cosmic_text::Metrics` from the font size, and a size of zero gives a
/// zero line height, which `cosmic_text::Buffer` asserts against — a
/// `font_size = 0` in a hand-edited file used to panic the whole app the
/// moment the hot-reload watcher picked the edit up. A negative size doesn't
/// panic; it hangs, spinning at 100% CPU inside text layout and never
/// returning. Neither is something a running terminal should ever be able to
/// do to itself over a config edit.
pub const MIN_FONT_SIZE: f32 = 6.0;
pub const MAX_FONT_SIZE: f32 = 48.0;
/// A ceiling on retained history per pane. Scrollback is allocated as it
/// fills rather than up front, so this bounds how much memory a pane can
/// eventually reach, not what it starts at.
pub const MAX_SCROLLBACK_LINES: usize = 1_000_000;

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
    /// Name of a built-in theme (see [`themes::THEMES`]) — the source of
    /// the 16 ANSI colors and the default foreground/background a cell
    /// falls back to. An unrecognized name resolves to
    /// [`themes::DEFAULT_THEME`] rather than failing to load, the same
    /// "never crash on a bad edit" convention as the color fields.
    ///
    /// No inline color table support: a theme is picked by name, and a
    /// one-off tweak is what `background_color` is for.
    pub theme: String,
    pub font_family: String,
    pub font_size: f32,
    /// Shape each row's text in runs so the font can apply ligatures
    /// (`!=` rendering as `≠`), rather than rasterizing every cell
    /// independently.
    ///
    /// **Off by default, deliberately.** A terminal grid is fixed-width and
    /// this hands glyph positioning to the font's own advances: with a font
    /// designed for it (Fira Code, JetBrains Mono, Cascadia Code, Iosevka)
    /// that lines up exactly, but a font whose ligature advances don't match
    /// its cell width will drift out of the grid. It also costs real work
    /// per frame that the per-character path doesn't — text has to be shaped,
    /// not just looked up. Neither is a cost to impose on someone who never
    /// asked for ligatures.
    pub ligatures: bool,
    /// 0.0 (fully transparent) – 1.0 (opaque).
    pub transparency: f32,
    /// Terminal background override, as `#rrggbb` hex.
    ///
    /// **Empty means "follow the chosen theme"**, which is the default —
    /// a theme's background is part of its identity, and a light theme
    /// forced onto a near-black ground is unreadable. A non-empty value
    /// wins over the theme, so a config that set this before themes
    /// existed keeps doing exactly what its author asked for rather than
    /// silently losing the setting.
    ///
    /// Parse failures (a hand-edited value that isn't valid hex) fall back
    /// to the theme's background via `background_rgb`, not a load error —
    /// consistent with the rest of this config's "never crash on a bad
    /// edit" handling.
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

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: themes::DEFAULT_THEME.to_string(),
            font_family: "monospace".to_string(),
            font_size: 13.0,
            ligatures: false,
            transparency: 1.0,
            // Empty: follow the theme. See the field's own doc comment.
            background_color: String::new(),
            accent_color: format_hex_rgb(DEFAULT_ACCENT_RGB),
        }
    }
}

/// Unpacks a `0xRRGGBB` theme color into 0.0–1.0 RGB.
fn unpack_rgb(packed: u32) -> [f32; 3] {
    let channel = |shift: u32| ((packed >> shift) & 0xff) as f32 / 255.0;
    [channel(16), channel(8), channel(0)]
}

impl Appearance {
    /// The built-in theme `theme` names, falling back to the default for an
    /// unset or unrecognized name.
    pub fn resolved_theme(&self) -> &'static themes::Theme {
        themes::find(&self.theme).unwrap_or_else(themes::default_theme)
    }

    /// The 16 ANSI colors (0-7 normal, 8-15 bright) of the chosen theme, as
    /// 0.0–1.0 RGB.
    pub fn palette(&self) -> [[f32; 3]; 16] {
        self.resolved_theme().ansi.map(unpack_rgb)
    }

    /// What a cell left at its default foreground color resolves to — the
    /// chosen theme's foreground.
    pub fn foreground_rgb(&self) -> [f32; 3] {
        unpack_rgb(self.resolved_theme().foreground)
    }

    /// The effective terminal background: `background_color` if it's set to
    /// valid hex, otherwise the chosen theme's own background.
    pub fn background_rgb(&self) -> [f32; 3] {
        parse_hex_rgb(&self.background_color).unwrap_or_else(|| unpack_rgb(self.resolved_theme().background))
    }

    /// Sets `background_color` from 0.0–1.0 RGB (e.g. from a UI color
    /// picker), formatted as `#rrggbb` — the inverse of `background_rgb`.
    /// This makes it an explicit override; see `follow_theme_background` to
    /// undo it.
    pub fn set_background_rgb(&mut self, rgb: [f32; 3]) {
        self.background_color = format_hex_rgb(rgb);
    }

    /// Clears any background override, so the background follows whichever
    /// theme is chosen.
    pub fn follow_theme_background(&mut self) {
        self.background_color.clear();
    }

    /// Whether the background currently follows the theme rather than an
    /// explicit override.
    pub fn follows_theme_background(&self) -> bool {
        self.background_color.is_empty()
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

/// Prints what [`Config::sanitize`] changed, one line each. Public so the
/// app's hot-reload path reports adjustments in the same voice the initial
/// load does, instead of formatting them itself.
pub fn report(adjustments: &[String]) {
    for adjustment in adjustments {
        eprintln!("config: {adjustment}");
    }
}

/// `value` clamped into `min..=max`, or `fallback` when it's `NaN` —
/// `f32::clamp` propagates `NaN` straight through rather than bounding it,
/// so a `NaN` in the file would otherwise pass this check untouched.
fn sane(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_nan() { fallback } else { value.clamp(min, max) }
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
            Ok((config, adjustments)) => {
                report(&adjustments);
                config
            }
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
    /// Also returns whatever [`Config::sanitize`] had to change, phrased for
    /// the user, rather than printing it here. A file watcher re-reads the
    /// file several times for a single save (one write is several
    /// filesystem events, and only the caller knows whether the result
    /// differs from what's already loaded), so reporting at parse time
    /// printed the same complaint about a dozen times per edit. The caller
    /// reports it at the point it decides to actually apply the result.
    pub fn try_load(path: &Path) -> Result<(Config, Vec<String>), toml::de::Error> {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                let mut config: Config = toml::from_str(&contents)?;
                let adjustments = config.sanitize();
                Ok((config, adjustments))
            }
            Err(_) => Ok((Config::default(), Vec::new())),
        }
    }

    /// Forces every numeric value into a range the rest of the app can
    /// actually handle, reporting anything it had to change.
    ///
    /// Applied to every file that parses, so no hand-edited value ever
    /// reaches the renderer or the terminal grid unchecked. This is the same
    /// "never crash on a bad edit" convention `background_color`'s parse
    /// fallback already follows, extended to the numeric fields — which
    /// previously had no validation at all, and where a bad value was not a
    /// cosmetic problem but a panic or a hang (see [`MIN_FONT_SIZE`]).
    ///
    /// Out-of-range values are clamped rather than reset to the default: a
    /// `font_size = 100` is a legible intent ("as big as you'll give me"),
    /// and 48 serves it better than silently dropping back to 13 would.
    /// A non-finite value has no intent to preserve, so it takes the
    /// default.
    fn sanitize(&mut self) -> Vec<String> {
        let defaults = Appearance::default();
        let mut adjustments = Vec::new();

        let font_size = sane(self.appearance.font_size, MIN_FONT_SIZE, MAX_FONT_SIZE, defaults.font_size);
        if font_size != self.appearance.font_size {
            adjustments.push(format!(
                "font_size {} is out of range ({MIN_FONT_SIZE}-{MAX_FONT_SIZE}); using {font_size}",
                self.appearance.font_size
            ));
            self.appearance.font_size = font_size;
        }

        let transparency = sane(self.appearance.transparency, 0.0, 1.0, defaults.transparency);
        if transparency != self.appearance.transparency {
            adjustments.push(format!(
                "transparency {} is out of range (0.0-1.0); using {transparency}",
                self.appearance.transparency
            ));
            self.appearance.transparency = transparency;
        }

        if self.general.scrollback_lines > MAX_SCROLLBACK_LINES {
            adjustments.push(format!(
                "scrollback_lines {} exceeds the {MAX_SCROLLBACK_LINES} line maximum; using {MAX_SCROLLBACK_LINES}",
                self.general.scrollback_lines
            ));
            self.general.scrollback_lines = MAX_SCROLLBACK_LINES;
        }

        adjustments
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
    fn background_color_falls_back_to_the_theme_when_invalid() {
        assert_eq!(parse_hex_rgb("not-a-color"), None);
        assert_eq!(parse_hex_rgb("#zzzzzz"), None);
        let appearance = Appearance { background_color: "garbage".to_string(), ..Appearance::default() };
        assert_eq!(appearance.background_rgb(), unpack_rgb(themes::default_theme().background));
    }

    /// The default has no override, so the background is the theme's — and
    /// changing theme moves it. A theme's background is part of its
    /// identity; a light theme on a forced near-black ground is unreadable.
    #[test]
    fn an_unset_background_follows_the_chosen_theme() {
        let mut appearance = Appearance::default();
        assert!(appearance.follows_theme_background());
        assert_eq!(appearance.background_rgb(), unpack_rgb(themes::default_theme().background));

        appearance.theme = "Dracula".to_string();
        assert_eq!(appearance.background_rgb(), unpack_rgb(0x282a36));
    }

    /// A config written before themes existed set this explicitly. That
    /// value has to keep winning, or upgrading would silently discard a
    /// setting its author deliberately chose.
    #[test]
    fn an_explicit_background_overrides_the_theme() {
        let appearance =
            Appearance { theme: "Dracula".to_string(), background_color: "#123456".to_string(), ..Default::default() };
        assert!(!appearance.follows_theme_background());
        assert_eq!(appearance.background_rgb(), parse_hex_rgb("#123456").unwrap());
    }

    #[test]
    fn following_the_theme_background_again_clears_an_override() {
        let mut appearance = Appearance { theme: "Dracula".to_string(), ..Default::default() };
        appearance.set_background_rgb([1.0, 0.0, 0.0]);
        assert!(!appearance.follows_theme_background());

        appearance.follow_theme_background();
        assert!(appearance.follows_theme_background());
        assert_eq!(appearance.background_rgb(), unpack_rgb(0x282a36));
    }

    #[test]
    fn an_unrecognized_theme_name_resolves_to_the_default_rather_than_failing() {
        let appearance = Appearance { theme: "no such theme".to_string(), ..Default::default() };
        assert_eq!(appearance.resolved_theme().name, themes::DEFAULT_THEME);
    }

    #[test]
    fn the_palette_unpacks_a_themes_sixteen_ansi_slots() {
        let appearance = Appearance { theme: "Dracula".to_string(), ..Default::default() };
        let palette = appearance.palette();
        assert_eq!(palette[1], unpack_rgb(0xff5555), "ANSI red");
        assert_eq!(palette[8], unpack_rgb(0x6272a4), "ANSI bright black");
        assert_eq!(appearance.foreground_rgb(), unpack_rgb(0xf8f8f2));
    }

    /// The shipped default must be visually identical to what the app
    /// looked like before themes existed, or every existing user's terminal
    /// silently restyles on upgrade.
    #[test]
    fn the_default_theme_preserves_the_original_graphite_look() {
        let appearance = Appearance::default();
        assert_eq!(appearance.background_rgb(), [12.0 / 255.0, 14.0 / 255.0, 17.0 / 255.0]);
        assert_eq!(appearance.foreground_rgb(), [223.0 / 255.0, 226.0 / 255.0, 230.0 / 255.0]);
        // xterm's standard palette, which is what `color.rs` hardcoded.
        assert_eq!(appearance.palette()[0], [0.0, 0.0, 0.0]);
        assert_eq!(appearance.palette()[15], [1.0, 1.0, 1.0]);
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

    /// Writes `body` to a throwaway `config.toml` and loads it back, so
    /// these exercise the real parse-then-sanitize path rather than calling
    /// `sanitize` directly — the file is where a bad value actually comes
    /// from, and `try_load` is the only thing standing between it and the
    /// renderer.
    fn load_from_toml(name: &str, body: &str) -> Config {
        let dir = std::env::temp_dir().join(format!("pain-config-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, body).unwrap();
        let config = Config::load(&path);
        std::fs::remove_dir_all(&dir).ok();
        config
    }

    /// A zero font size used to take down the whole app: it reaches
    /// `cosmic_text::Metrics` as a zero *line height*, which `Buffer`
    /// asserts against. Via the config watcher that meant saving the file
    /// panicked a running terminal — not a startup-only problem.
    #[test]
    fn a_zero_font_size_is_clamped_rather_than_reaching_the_renderer() {
        let config = load_from_toml("zero-font", "[appearance]\nfont_size = 0.0\n");
        assert_eq!(config.appearance.font_size, MIN_FONT_SIZE);
    }

    /// A negative font size didn't panic — it hung, spinning at 100% CPU
    /// inside text layout and never returning, which is strictly worse
    /// (no message, no exit, nothing to report).
    #[test]
    fn a_negative_font_size_is_clamped_rather_than_reaching_the_renderer() {
        let config = load_from_toml("negative-font", "[appearance]\nfont_size = -13.0\n");
        assert_eq!(config.appearance.font_size, MIN_FONT_SIZE);
    }

    #[test]
    fn an_absurdly_large_font_size_is_clamped_to_the_maximum() {
        let config = load_from_toml("huge-font", "[appearance]\nfont_size = 4000.0\n");
        assert_eq!(config.appearance.font_size, MAX_FONT_SIZE);
    }

    /// `f32::clamp` returns `NaN` unchanged, so this needs its own handling
    /// — a clamp alone would let it straight through.
    #[test]
    fn a_non_numeric_font_size_falls_back_to_the_default() {
        let config = load_from_toml("nan-font", "[appearance]\nfont_size = nan\n");
        assert_eq!(config.appearance.font_size, Appearance::default().font_size);
    }

    #[test]
    fn transparency_outside_zero_to_one_is_clamped() {
        assert_eq!(load_from_toml("over-alpha", "[appearance]\ntransparency = 4.0\n").appearance.transparency, 1.0);
        assert_eq!(load_from_toml("under-alpha", "[appearance]\ntransparency = -1.0\n").appearance.transparency, 0.0);
        assert_eq!(
            load_from_toml("nan-alpha", "[appearance]\ntransparency = nan\n").appearance.transparency,
            Appearance::default().transparency
        );
    }

    #[test]
    fn scrollback_lines_is_capped() {
        let config = load_from_toml("huge-scrollback", "[general]\nscrollback_lines = 99999999999\n");
        assert_eq!(config.general.scrollback_lines, MAX_SCROLLBACK_LINES);
    }

    #[test]
    fn values_already_in_range_are_left_exactly_as_written() {
        let config = load_from_toml(
            "in-range",
            "[appearance]\nfont_size = 17.5\ntransparency = 0.8\n\n[general]\nscrollback_lines = 200\n",
        );
        assert_eq!(config.appearance.font_size, 17.5);
        assert_eq!(config.appearance.transparency, 0.8);
        assert_eq!(config.general.scrollback_lines, 200);
    }
}
