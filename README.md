# pain

Finding the perfect terminal emulator is a pain.

A cross-platform, multi-pane terminal emulator with nested splits, resizing,
grouping with broadcast input, and session persistence — built on Alacritty's
VT/PTY backend with an original GPU-rendered frontend.

## Why

Terminator is great but is Linux/GTK-only. iTerm2 is great but is Mac only.
Alacritty is fast and cross-platform but excludes tabs and splits. This project
is inspired by the greats and attempts to combine them into a single native
application for Windows, macOS, and Linux.

See [CHANGELOG.md](CHANGELOG.md) for a detailed history, and
[releases](../../releases) for binaries.

## Install

### Debian / Ubuntu

Add the APT repository, for `apt upgrade` support:

```sh
curl -fsSL https://w-p.github.io/pain/pain-archive-keyring.asc | sudo gpg --dearmor -o /etc/apt/keyrings/pain.gpg
echo "deb [signed-by=/etc/apt/keyrings/pain.gpg] https://w-p.github.io/pain ./" | sudo tee /etc/apt/sources.list.d/pain.list
sudo apt update && sudo apt install pain
```

### macOS

macOS builds ship as a universal `pain.app` (Intel and Apple Silicon in one
download) — drag it to Applications and open it like any other app.

The app isn't code-signed, so Gatekeeper blocks the first launch. Clear the
quarantine flag once — note `-r`, since the flag is set on files inside the
bundle too:

```sh
xattr -dr com.apple.quarantine /Applications/pain.app
```

`pain.app` is a bundle, which on disk is a *directory*, not a single
executable file. Running `./pain.app` from a shell fails with "permission
denied" (zsh) or "Is a directory" (bash) — that's the shell refusing to
execute a directory, not a problem with the download. To launch it from a
terminal:

```sh
open pain.app                    # hand it to macOS, same as double-clicking
./pain.app/Contents/MacOS/pain   # run the binary directly, to see log output
```

### Fedora / RHEL / Rocky / Alma

Add the DNF repository, for `dnf upgrade` support:

```sh
sudo dnf config-manager --add-repo https://w-p.github.io/pain/rpm/pain.repo
sudo dnf install pain
```

### Any other Linux

Download the AppImage — one file, no install, works on any distribution
with glibc 2.35 or newer (including immutable ones like Silverblue,
Kinoite, and Bazzite):

```sh
curl -fLO https://w-p.github.io/pain/appimage/pain-x86_64.AppImage
chmod +x pain-x86_64.AppImage
./pain-x86_64.AppImage
```

If it exits complaining about FUSE, your distribution doesn't ship
libfuse2 — Fedora and Ubuntu 24.04 among them. Either install it, or run
without it:

```sh
./pain-x86_64.AppImage --appimage-extract-and-run
```

### Windows

Download the archive from [releases](../../releases).

## Usage

Run `pain`. There are no positional arguments — every pane starts your
configured shell.

| Option | Meaning |
| --- | --- |
| `-h`, `--help` | Usage summary, including this machine's config file path |
| `-V`, `--version` | Print the version |
| `-v`, `--verbose[=LIST]` | Diagnostic logging on stderr |

`LIST` is a comma-separated set of `general`, `mouse`, `pty`, `foreground`, or
`all`. The bare flag enables `general` alone — the others fire constantly
enough to drown it out, so each is an explicit opt-in.

Full documentation is in the man page: `man pain`.

### Keyboard shortcuts

Every shortcut is a default and can be changed — see
[Configuration](#configuration). Keys not listed pass through to the shell.

| Shortcut | Action |
| --- | --- |
| `Ctrl+Shift+O` | Split pane horizontally |
| `Ctrl+Shift+E` | Split pane vertically |
| `Ctrl+Shift+W` | Close pane (closing the last one exits) |
| `Ctrl+Shift+X` | Zoom pane to fill the window, or restore it |
| `Ctrl+Shift+Q` | Quit, saving the session |
| `Alt+Up`, `Alt+Down`, `Alt+Left`, `Alt+Right` | Move focus to the neighbouring pane |
| `Ctrl+Shift+Up`, `Ctrl+Shift+Down`, `Ctrl+Shift+Left`, `Ctrl+Shift+Right` | Resize the focused pane |

Clipboard shortcuts differ per platform, because what a terminal can safely
claim differs per platform:

| Platform | Shortcut | Action |
| --- | --- | --- |
| Windows, Linux | `Ctrl+C` | Copy the selection if there is one, otherwise interrupt the running program |
| Windows, Linux | `Ctrl+V` | Paste |
| macOS | `Cmd+C` / `Cmd+V` | Copy / paste |
| macOS | `Cmd+Q` / `Cmd+W` | Quit / close pane |

`Ctrl+C` costs you nothing: with no selection it interrupts exactly as it
always has, and copying clears the selection so a second press interrupts
rather than copying again. `Ctrl+V` does displace readline's `quoted-insert`;
set `"ctrl+v" = "none"` to get it back. On macOS the Ctrl key is left alone
entirely, since Command is where the clipboard belongs there.

`Ctrl+Shift+C`/`Ctrl+Shift+V`, the usual Linux-terminal clipboard chords,
are not bound. They only ever existed because the unshifted pair wasn't
available, which is no longer the case on either platform. If you have the
muscle memory, bind them back:

```toml
[keybindings]
"ctrl+shift+c" = "copy"
"ctrl+shift+v" = "paste"
```

Pasted text is wrapped in bracketed-paste markers when the running program
supports them, so your shell holds it at the prompt for review instead of
running each line as it arrives. When the program *doesn't* support them, a
multi-line paste asks for confirmation and shows exactly what will be sent.

**Broadcast** — sending your keystrokes to several panes at once — has no
default chord and is set from the title-bar menu. The `broadcast_off`,
`broadcast_group`, and `broadcast_all` actions can be bound if you want them.
Assigning a pane to a group is menu-only: it needs a group name, which a
chord can't carry.

### Mouse

| Input | Action |
| --- | --- |
| Double-click / triple-click | Select word / line |
| `Ctrl+click` | Open the URL under the pointer (holding `Ctrl` underlines it first) |
| `Shift+click` | Force local selection, bypassing an app's own mouse reporting |
| Right-click a title bar | Pane menu: split, arrange, group, broadcast, swap shell, settings |
| Right-click a terminal | Terminal menu: copy, paste, close |
| Scroll wheel | Scroll back through that pane's history |

`Shift+click` is the standard escape hatch for selecting text inside
full-screen programs like vim or htop, which would otherwise eat the click.

## Configuration

Settings live in a TOML file, read at startup and re-read when it changes:

| Platform | Path |
| --- | --- |
| Linux | `~/.config/pain/config.toml` (honours `$XDG_CONFIG_HOME`) |
| macOS | `~/Library/Application Support/pain/config.toml` |
| Windows | `%APPDATA%\pain\config.toml` |

`pain --help` prints the resolved path for the machine you're on. The file
doesn't exist until you save settings from the settings panel or create it
yourself, and every key is optional — a partial file is valid, and anything
missing uses its default.

A malformed file is never fatal. A parse failure at startup falls back to
defaults with a message on stderr; a bad edit while running keeps the settings
already loaded rather than resetting them; and individual bad `[keybindings]`
lines are skipped one at a time rather than poisoning the whole table.

```toml
[general]
default_shell = ""            # empty = platform default ($SHELL, or your Windows default)
scrollback_lines = 5000       # lines of history per pane
confirm_multiline_paste = true

[appearance]
theme = "default"             # reserved; the theme format isn't settled yet
font_family = "monospace"     # any installed monospaced family
font_size = 13                # logical size, scaled by the display's DPI factor
transparency = 1.0            # 0.0 transparent .. 1.0 opaque
background_color = "#0c0e11"
accent_color = "#7fa2d6"      # cursor, selection, interactive highlights

[cursor]
style = "block"               # block | underline | beam

[keybindings]
"ctrl+shift+t" = "split_vertical"
"ctrl+v" = "none"             # hand a chord back to the shell
```

`confirm_multiline_paste` is the last check on an unreviewed paste running
arbitrary commands the instant it arrives; turning it off removes that.

`font_size` is a *logical* size scaled by your display's DPI factor, so 13
matches other applications on a scaled display rather than rendering smaller
than everything else.

Colors that carry meaning rather than style — the broadcast-target border, for
instance — are fixed and unaffected by `accent_color`. An unparseable color
falls back to its default rather than failing to load.

### Keybindings

A chord is `+`-separated, case-insensitive, with modifiers in any order and
exactly one non-modifier segment: a single character, or `up`/`down`/`left`/
`right`. Write `ctrl` (or `control`) and `cmd` (or `logo`/`super`/`win`).

The action `none` unbinds a chord with no replacement. Recognized actions:

`split_horizontal`, `split_vertical`, `close_pane`, `quit`, `focus_up`,
`focus_down`, `focus_left`, `focus_right`, `resize_up`, `resize_down`,
`resize_left`, `resize_right`, `toggle_zoom`, `copy`, `copy_or_interrupt`,
`paste`, `broadcast_off`, `broadcast_group`, `broadcast_all`.

Overrides are applied on top of a fresh copy of the defaults each time the
file is read, so deleting a line restores that chord's built-in binding rather
than leaving it stuck at the old override.

## Built On

| Layer             | Crate                | Role                                              |
| ----------------- | -------------------- | ------------------------------------------------- |
| PTY               | `portable-pty`       | Unix PTY + Windows ConPTY behind one API          |
| VT backend        | `alacritty_terminal` | Parser, screen grid, scrollback, cursor state     |
| Windowing / input | `winit`              | Cross-platform window creation + input events     |
| Rendering         | `wgpu`               | GPU rendering for the text grid and the UI chrome |
| Font shaping      | `cosmic-text`        | Font discovery, shaping, Unicode width handling   |
| UI chrome         | `egui`               | Config panel, menus, non-grid UI                  |

`vendor/wgpu-hal-29.0.4/` is a local-only patched copy of `wgpu-hal`, pulled
in automatically via `[patch.crates-io]` in the workspace `Cargo.toml` — see
`vendor/README.md` for what it fixes and why.

## Building from source

Standard Cargo workspace:

```sh
cargo build --release
cargo test --workspace
cargo run -p pain
```

### Linux packages

The `.deb`, `.rpm`, AppImage, and tarball are all produced from one compile
inside a container, which pins the glibc floor at 2.35 rather than letting
it drift upward with whatever the CI runner happens to be running. The same
script CI uses runs locally, with either podman or docker:

```sh
./scripts/linux-packages.sh build    # artifacts into ./dist
./scripts/linux-packages.sh verify   # install and start each one
./scripts/linux-packages.sh all      # both
```

`verify` installs each package into a stock image of the distribution it
targets and starts the application under a virtual display with software
rendering. That last part matters: `--version` returns before the event loop
starts, so it never reaches the X11, Wayland, xkbcommon, or Vulkan
libraries — which are loaded at runtime and are exactly the dependencies
most likely to be declared wrong.

### Linux packages

The `.deb`, `.rpm`, AppImage, and tarball are all built from one compile
inside a container, which pins the glibc floor at 2.35 rather than letting
it drift upward with whatever the CI runner happens to be running. The
same script CI uses runs locally, with either podman or docker:

```sh
./scripts/linux-packages.sh build    # artifacts into ./dist
./scripts/linux-packages.sh verify   # install and start each one
./scripts/linux-packages.sh all      # both
```

`verify` installs each package into a stock image of the distribution it
targets and starts the application under a virtual display with software
rendering. That matters because `--version` returns before the event loop
starts, so it never touches the X11, Wayland, xkbcommon, or Vulkan
libraries — which are loaded at runtime and are exactly the dependencies
most likely to be declared wrong.

## License

MIT — see [LICENSE](LICENSE).
