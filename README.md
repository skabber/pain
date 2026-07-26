# pain

Finding the perfect terminal emulator is a pain.

This is cross-platform, multi-pane terminal emulator with nested splits, resizing,
grouping with broadcast input, and session persistence built on
Alacritty's VT/PTY backend with an original GPU-rendered frontend.

## Why

Terminator is great but is Linux/GTK-only. iTerm2 is great but is Mac only. Alacritty is fast and cross-platform but excludes tabs and splits. This project is inspired by the greats and attempts to combine them into a single native application for Windows, macOS, and Linux.

See [CHANGELOG.md](CHANGELOG.md) for a detailed history.
See [releases](../../releases) for binaries.

macOS builds ship as a universal `pain.app` (Intel and Apple Silicon in one
download) — drag it to Applications. The app isn't code-signed yet, so
Gatekeeper will block the first launch; clear the quarantine flag once:

```sh
xattr -dr com.apple.quarantine /Applications/pain.app
```

Debian/Ubuntu users can add the APT repository instead, for `apt upgrade` support:

```sh
curl -fsSL https://w-p.github.io/pain/pain-archive-keyring.asc | sudo gpg --dearmor -o /etc/apt/keyrings/pain.gpg
echo "deb [signed-by=/etc/apt/keyrings/pain.gpg] https://w-p.github.io/pain ./" | sudo tee /etc/apt/sources.list.d/pain.list
sudo apt update && sudo apt install pain
```

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
```

## Usage

`cargo run -p pain`, or run the built binary directly. `-v`/`--verbose`
(optionally `--verbose=<category>`, e.g. `mouse`, `pty`, `foreground`, or
`all`) turns on diagnostic logging.

## License

MIT — see [LICENSE](LICENSE).
