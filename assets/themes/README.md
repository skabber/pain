# Built-in themes

`crates/config/src/themes.rs` is generated, not hand-written. It holds every
built-in color theme as a compact table of packed `0xRRGGBB` values, compiled
into the binary — so themes work identically across the tarball, `.deb`, RPM,
AppImage, Windows zip, and macOS `.app` with no asset paths to resolve at
runtime and no file I/O on startup.

## Where they come from

All but one are vendored from
[iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes),
specifically its Alacritty exports, whose color model (16 ANSI slots plus a
default foreground and background) is exactly ours. `LICENSE` in this
directory is that collection's, reproduced in full: MIT, Copyright (c) 2011 to
Present Mark Badolato.

Note the collection's own closing caveat — the copyright for each individual
theme belongs to that theme's author. This is the same basis on which
Ghostty, Alacritty, WezTerm and others redistribute the set.

The exception is **Graphite**, this app's own default palette (xterm's
standard 16 colors over the Graphite ground and ink). It's defined directly in
`generate.py` rather than vendored, for two reasons: the shipped default
shouldn't depend on an external collection, and re-vendoring a newer upstream
must never silently restyle everyone who never picked a theme.

The upstream collection happens to contain a *different* theme also called
`Graphite`, which the generator drops and reports rather than emitting a
duplicate name. That one theme is the cost of keeping our own established
palette name; the other 601 come through untouched.

## Regenerating

```sh
git clone --depth 1 --filter=blob:none --sparse \
    https://github.com/mbadolato/iTerm2-Color-Schemes.git schemes
cd schemes && git sparse-checkout set alacritty && cd ..

python3 assets/themes/generate.py schemes/alacritty crates/config/src/themes.rs
```

The generator prints how many themes it wrote and lists anything it skipped,
with the reason. A skip is worth reading rather than ignoring: it means a file
was missing a color the table requires, or collided with the built-in default.

`generate.py` uses the standard library's `tomllib` where available (Python
3.11+) and falls back to a small purpose-built reader otherwise, since these
files are a uniform, trivially-shaped subset of TOML.
