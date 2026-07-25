# Design: Config System

**Status:** proposed — pending review
**Feeds from:** `.waypoint/conops.md` §5e (Configuration)

## Intent

A TOML config file with hot reload, backing both startup config and an
in-app egui settings panel, without the two getting out of sync.

## File location

Platform config directory, `config.toml`:

- Linux: `$XDG_CONFIG_HOME/<app>/config.toml` (fallback `~/.config/<app>/`)
- macOS: `~/Library/Application Support/<app>/config.toml`
- Windows: `%APPDATA%\<app>\config.toml`

## Schema (serde + `toml` crate)

```toml
[general]
default_shell = ""       # empty = platform default ($SHELL / user's default shell)
scrollback_lines = 5000

[appearance]
theme = "default"         # see design/theme.md (open question, CONOPS §8)
font_family = "monospace"
font_size = 13.0
transparency = 1.0        # 0.0 (fully transparent) – 1.0 (opaque)

[cursor]
style = "block"           # block | underline | beam

[keybindings]
# chord string -> action name; overrides layered on top of built-in
# Terminator-equivalent defaults, not a full replacement
"ctrl+shift+e" = "split_vertical"
"ctrl+shift+o" = "split_horizontal"
# ...
```

Keybinding entries are a sparse override layer: any chord not present here
falls back to the built-in default keymap (the Terminator-equivalent set).
Remapping a chord to `"none"` unbinds it without requiring a replacement.

## Load and hot reload

1. On startup: read `config.toml` if present, `serde`-deserialize into a
   `Config` struct, merge keybinding overrides onto the built-in default
   keymap. Missing file → use all defaults, do not error.
2. A filesystem watcher (`notify` crate) watches the config file's directory.
3. On a write event: re-parse. If parsing succeeds, apply the new `Config` to
   live state (font/theme/transparency trigger a re-render; keybinding
   changes rebuild the router's `keymap`). If parsing fails, keep the last
   good config and surface a non-blocking error (e.g. a toast in the egui
   layer) — a bad edit never crashes or blanks the running session.

## Settings panel (egui)

- Reads from the same in-memory `Config` struct the router/renderer use — no
  separate copy.
- "Save" serializes `Config` back to `config.toml`. This re-triggers the same
  filesystem watcher path used for external edits — the panel does not have a
  separate apply path from hand-editing the file.

## Rationale

- Routing settings-panel saves through the same watch-and-reload path as
  external file edits means there is exactly one code path that applies
  config changes, regardless of source — no divergent "apply from UI" vs.
  "apply from file" logic to keep in sync.
- Keybindings as a sparse override layer (rather than requiring a full keymap
  in the file) keeps `config.toml` short for the common case of remapping one
  or two chords, and means shipping updated Terminator-equivalent defaults
  later doesn't require every user's config to be migrated.

## Open questions

- Theme/color scheme format and bundled presets — carried from CONOPS §8,
  unresolved. Affects the `[appearance] theme` field's shape (named preset vs.
  inline color table) once decided.
