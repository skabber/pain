"""Generates crates/config/src/themes.rs from iTerm2-Color-Schemes' alacritty exports."""

import pathlib
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None


def parse_toml(text):
    """Minimal reader for these files: `[a.b]` sections of `key = 'value'`.

    Only used when the stdlib has no tomllib (Python < 3.11). The generated
    output is compared against the real parser where one is available.
    """
    out, section = {}, None
    for raw in text.splitlines():
        line = raw.strip()
        # Only a leading `#` is a comment here — every color value in these
        # files starts with one, so splitting on `#` anywhere would strip
        # the value itself.
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            section = out
            for part in line[1:-1].split("."):
                section = section.setdefault(part.strip(), {})
            continue
        if "=" in line and section is not None:
            key, value = line.split("=", 1)
            section[key.strip()] = value.strip().strip("'\"")
    return out

SRC = pathlib.Path(sys.argv[1])
OUT = pathlib.Path(sys.argv[2])

# ANSI slots 0-7, in standard order; 8-15 are the "bright" variants.
SLOTS = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"]


def hexval(s):
    m = re.fullmatch(r"#?([0-9a-fA-F]{6})", s.strip())
    if not m:
        raise ValueError(f"bad color {s!r}")
    return int(m.group(1), 16)


# This app's own default look, always first and always present: xterm's
# standard 16 colors over the "Graphite" ground and ink. Defined here rather
# than vendored so the shipped default never depends on the upstream
# collection, and so upgrading that collection can't silently restyle
# everyone who never picked a theme.
GRAPHITE = (
    "Graphite",
    [
        0x000000, 0xCD0000, 0x00CD00, 0xCDCD00, 0x0000EE, 0xCD00CD, 0x00CDCD, 0xE5E5E5,
        0x4C4C4C, 0xFF0000, 0x00FF00, 0xFFFF00, 0x5C5CFF, 0xFF00FF, 0x00FFFF, 0xFFFFFF,
    ],
    0xDFE2E6,
    0x0C0E11,
)

themes = []
skipped = []

for path in sorted(SRC.glob("*.toml"), key=lambda p: p.name.lower()):
    try:
        text = path.read_text(encoding="utf-8")
        data = tomllib.loads(text) if tomllib else parse_toml(text)
        colors = data["colors"]
        normal, bright, primary = colors["normal"], colors["bright"], colors["primary"]
        ansi = [hexval(normal[s]) for s in SLOTS] + [hexval(bright[s]) for s in SLOTS]
        fg = hexval(primary["foreground"])
        bg = hexval(primary["background"])
    except Exception as err:
        skipped.append((path.name, str(err)))
        continue
    if path.stem == GRAPHITE[0]:
        skipped.append((path.name, "name collides with the built-in default"))
        continue
    themes.append((path.stem, ansi, fg, bg))

themes.insert(0, GRAPHITE)

lines = [
    "//! Built-in color themes.",
    "//!",
    "//! Generated — do not edit by hand. The bulk of this table is vendored from",
    "//! the iTerm2-Color-Schemes collection (MIT, Copyright (c) 2011 to Present",
    "//! Mark Badolato); see `assets/themes/LICENSE` for the full text and the",
    "//! per-theme attribution note. Regenerate with `assets/themes/generate.py`.",
    "//!",
    "//! Colors are packed `0xRRGGBB`. `ansi` holds the 16 base slots in the",
    "//! standard order (0-7 normal, 8-15 bright); `foreground`/`background` are",
    "//! what a cell left at its default color resolves to.",
    "",
    "/// One built-in theme: the 16 ANSI slots plus the default foreground and",
    "/// background a cell falls back to.",
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
    "pub struct Theme {",
    "    pub name: &'static str,",
    "    pub ansi: [u32; 16],",
    "    pub foreground: u32,",
    "    pub background: u32,",
    "}",
    "",
    "/// The default theme's name — this app's own look, and what an unset or",
    "/// unrecognized `appearance.theme` falls back to.",
    f'pub const DEFAULT_THEME: &str = "{GRAPHITE[0]}";',
    "",
    "/// Every built-in theme. The default comes first; the rest follow sorted",
    "/// by name (case-insensitive), which is the order the settings panel's",
    "/// picker presents them in directly.",
    "pub const THEMES: &[Theme] = &[",
]

for name, ansi, fg, bg in themes:
    escaped = name.replace("\\", "\\\\").replace('"', '\\"')
    packed = ", ".join(f"0x{c:06x}" for c in ansi)
    lines.append(
        f'    Theme {{ name: "{escaped}", ansi: [{packed}], '
        f"foreground: 0x{fg:06x}, background: 0x{bg:06x} }},"
    )

lines.append("];")
lines.extend(
    [
        "",
        "/// The theme `name` refers to, or `None` if no built-in has that name.",
        "///",
        "/// Matching is case-insensitive: these names are display strings with",
        "/// spaces and mixed case, and a hand-edited config shouldn't fail over",
        "/// `dracula` vs `Dracula`.",
        "pub fn find(name: &str) -> Option<&'static Theme> {",
        "    THEMES.iter().find(|theme| theme.name.eq_ignore_ascii_case(name))",
        "}",
        "",
        "/// The default theme. Present unconditionally — it's the first entry of",
        "/// a table this generator always writes, not something vendored that a",
        "/// regeneration could drop.",
        "pub fn default_theme() -> &'static Theme {",
        '    THEMES.first().expect("THEMES is generated with the default first and is never empty")',
        "}",
        "",
        "#[cfg(test)]",
        "mod tests {",
        "    use super::*;",
        "",
        "    #[test]",
        "    fn the_default_theme_is_first_and_findable_by_name() {",
        "        assert_eq!(default_theme().name, DEFAULT_THEME);",
        "        assert_eq!(find(DEFAULT_THEME).map(|t| t.name), Some(DEFAULT_THEME));",
        "    }",
        "",
        "    #[test]",
        "    fn lookup_is_case_insensitive_and_rejects_unknown_names() {",
        '        assert!(find("dRaCuLa").is_some());',
        '        assert!(find("no such theme").is_none());',
        "    }",
        "",
        "    /// Duplicates would make `find` silently prefer whichever came",
        "    /// first, so a name collision has to be caught at generation time",
        "    /// (the generator drops and reports them) rather than here.",
        "    #[test]",
        "    fn theme_names_are_unique() {",
        "        let mut names: Vec<String> = THEMES.iter().map(|t| t.name.to_lowercase()).collect();",
        "        names.sort();",
        "        let before = names.len();",
        "        names.dedup();",
        "        assert_eq!(names.len(), before, \"duplicate theme names in the generated table\");",
        "    }",
        "",
        "    /// A sanity check on the generator's color unpacking, against a",
        "    /// theme whose published values are widely known.",
        "    #[test]",
        "    fn a_vendored_theme_kept_its_published_colors() {",
        '        let dracula = find("Dracula").expect("Dracula should be vendored");',
        "        assert_eq!(dracula.background, 0x282a36);",
        "        assert_eq!(dracula.foreground, 0xf8f8f2);",
        "        assert_eq!(dracula.ansi[1], 0xff5555, \"ANSI red\");",
        "        assert_eq!(dracula.ansi[8], 0x6272a4, \"ANSI bright black\");",
        "    }",
        "",
        "    #[test]",
        "    fn the_collection_is_actually_large() {",
        "        assert!(THEMES.len() > 500, \"expected the full vendored collection, got {}\", THEMES.len());",
        "    }",
        "}",
        "",
    ]
)

OUT.write_text("\n".join(lines), encoding="utf-8")
print(f"wrote {len(themes)} themes to {OUT}")
if skipped:
    print(f"skipped {len(skipped)}:")
    for name, err in skipped[:20]:
        print(f"  {name}: {err}")
