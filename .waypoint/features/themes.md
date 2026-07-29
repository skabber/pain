# Built-in color themes

**Shipped:** 2026-07-28. Closes CONOPS §8, the project's last open question.

## What exists

`appearance.theme` names one of 602 built-in themes. A theme supplies the 16
ANSI colors programs draw with, plus the default foreground and background a
cell falls back to. The settings panel has a filterable picker.

- `crates/config/src/themes.rs` — **generated**, do not hand-edit. A flat
  table of packed `0xRRGGBB` values plus `find`/`default_theme`.
- `assets/themes/generate.py` — regenerates it. See
  `assets/themes/README.md` for the procedure and the upstream licence.
- `crates/app/src/color.rs` — `resolve` takes a `&Palette` instead of the
  hardcoded const it used to own.

## Why it is built this way

**Compiled in, not loaded from disk.** Themes are a table in the binary, so
there is no asset path to resolve at runtime and no file I/O at startup, and
behaviour is identical across the tarball, `.deb`, RPM, AppImage, Windows zip
and macOS `.app`. Loading from a directory would have meant a different
answer per package format for where that directory lives.

**Vendored from iTerm2-Color-Schemes' Alacritty exports.** That collection's
Alacritty format is exactly our color model — 16 slots plus a default
foreground/background — so this is a parse, not a conversion. MIT; note the
collection's own caveat that per-theme copyright stays with each theme's
author. Same basis Ghostty, Alacritty and WezTerm redistribute it on.

**The default is defined in the generator, not vendored.** `Graphite` is this
app's own palette (xterm's standard 16 over the Graphite ground and ink) and
is emitted first, unconditionally. Two reasons: the shipped default must not
depend on an external collection, and re-vendoring a newer upstream must
never silently restyle everyone who never picked a theme. A test asserts the
default still produces the exact pre-theme colors.

**Name collision is reported, not silently resolved.** Upstream contains its
own, different `Graphite`. The generator drops it and says so, because two
entries with one name would make `find` silently prefer whichever came first.
That one theme is the cost of keeping our established palette name.

**The 256-color cube stays unthemed.** Only slots 0-15 follow the theme.
Indices 16-255 are computed from the standard xterm formula, the same in
every terminal — a program asking for index 200 wants that exact color, not
a reinterpretation. This is the universal convention and there is a test
pinning it.

**`background_color` became an override rather than being removed.** Empty
means "follow the theme", which is the default and is right, because a
theme's background is part of its design and a light theme forced onto a
near-black ground is unreadable. But configs written before themes existed
set it explicitly, and those authors meant it — so a non-empty value still
wins. Removing the field would have silently discarded a deliberate setting
on upgrade.

**`accent_color` is untouched.** The split is: the theme governs what
*programs'* colors look like; the accent governs what the *app's own*
highlights look like (cursor, selection, link underline). Semantic signals —
the broadcast border, the activity dot — stay fixed regardless of both.

## Consequences worth knowing

- Switching theme restyles panes that are already open, with nothing to
  invalidate: `redraw` reads the palette fresh each frame, the same way every
  other appearance setting already worked.
- An unrecognised theme name resolves to the default rather than failing to
  load, and the name is preserved in the config rather than rewritten — so a
  config written by a newer version survives a downgrade.
- The picker caps its list at 100 matches and says so. Silent truncation
  would read as "that's all there is".
