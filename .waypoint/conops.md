# CONOPS: Terminal Emulator (working name TBD)

---

## 1. Background and Problem Statement

Terminator provides a genuinely good multi-pane terminal UX — arbitrary nested
splits, drag-resizable dividers, and pane grouping with broadcast input — but it
is Linux/GTK-only. Alacritty is fast, GPU-rendered, and cross-platform, but its
design deliberately excludes tabs and splits: one window, one grid, one PTY,
with multiplexing pushed out to the window manager or tmux.

There is no terminal that gives people Terminator's pane model with
Alacritty-grade rendering, consistently across Windows, macOS, and Linux.
People who rely on Terminator's workflow and also work across multiple
operating systems are stuck choosing between the UX they want and the
platforms they actually use — or adopting a "batteries-included" terminal
whose opinionated feature set (built-in AI, plugin marketplaces, cloud sync,
telemetry) they didn't ask for.

## 2. Vision

Build a new terminal emulator — not a fork of Alacritty — that pairs
Alacritty's backend crate (VT parsing, screen grid, scrollback, cursor state)
with an original frontend where multi-pane is native. The core engine is
inherited from mature, daily-driver crates; the pane/layout system, input
routing, chrome, config, and session handling are built from scratch.

Guiding principles:

- **No frills.** Fast rendering and real multiplexing — not a platform for
  extensions, AI features, or cloud services.
- **Consistent everywhere.** The same pane model, keybindings, and behavior on
  Windows, macOS, and Linux — no platform is a second-class citizen.
- **Layout persists, processes don't.** Session persistence covers layout and
  working directories only; it is not a tmux-style daemon.

## 3. Users and Use Cases

**Users:** the developer building this, and people like them — developers and
power users who work across Windows, macOS, and Linux and want one consistent,
lightweight terminal experience instead of relearning workflows per platform
or accepting a heavyweight, opinionated terminal.

Representative use cases:

1. **Daily driver across OSes.** Working on Windows at a desk, macOS on a
   laptop, and Linux over SSH, and wanting identical split/pane/keybinding
   muscle memory on all three, so switching machines doesn't mean relearning
   shortcuts.
2. **Broadcast to multiple shells.** Typing a command once (`git pull`,
   restarting a service, tailing logs) and having it fan out to every pane in
   a group, without standing up tmux or pssh for it.
3. **Ad hoc task layout.** Splitting into a few panes for a work session (a
   scratch shell, a log tail, a build watcher) and closing the whole thing out
   when the task is done — no persistent session to manage.
4. **Restoring a familiar layout.** Reopening the terminal after a reboot or
   update and getting the same split layout and per-pane working directories
   back, with fresh shells — not resumed processes.
5. **Escaping batteries-included bloat.** Coming from a terminal with built-in
   AI, cloud sync, or a plugin marketplace, and wanting something fast and
   quiet instead — real multiplexing, sane defaults, nothing else.
6. **Mouse-driven pane management.** Clicking to focus a pane, dragging a
   divider to resize, dragging in-grid to select text, and holding Shift to
   force terminal-native selection even when the running program (vim, htop)
   has mouse reporting enabled.

## 4. System Overview

| Layer | Crate / tool | Role |
|---|---|---|
| PTY | `portable-pty` (WezTerm) | Unix PTY + Windows ConPTY behind one API |
| VT backend | `alacritty_terminal` | Parser + screen grid + scrollback + cursor state, per pane |
| Windowing / input | `winit` | Cross-platform window creation + input events |
| Rendering | `wgpu` | GPU rendering for the text grid **and** egui, on one context |
| Font shaping | `cosmic-text` | Font discovery, shaping, layout, Unicode width handling |
| UI chrome | `egui` | Config panel, menus, and non-grid UI, on the wgpu context |

Each **pane** is a self-contained unit: its own `portable-pty` handle (one
shell process), its own `alacritty_terminal` instance (grid, scrollback,
cursor, title), its own size, and a reference to its broadcast group.

Panes are held in a **layout tree**: a binary tree of split nodes
(orientation + ratio) and leaf nodes (panes). A central input router directs
keyboard and mouse events to the focused pane, or — for grouped/broadcast
input — to multiple panes at once. The renderer walks the layout tree each
frame, drawing every visible pane's grid plus the chrome (dividers, optional
tabs, config panel).

We own the pane layout and dividers ourselves; that geometry is the core of
the product and is not delegated to egui's layout system. egui is scoped to
the config panel, menus/dialogs, and non-grid widgets (broadcast-mode
indicator, pane titles).

Data flow per pane: `shell → PTY → alacritty_terminal (parse + grid) → renderer`.
Input flows back: `event → router → group resolution → one or more PTYs`.

## 5. Concept of Operations

### 5a. Panes and layout

Any pane can split horizontally or vertically, and results can split again to
any depth. Each split is drag-resizable via its divider. Focus moves
directionally between panes. A pane can be zoomed to fill the window and
toggled back. Closing a pane rebalances the tree — a sibling expands to fill
the freed space.

### 5b. Grouping and broadcast input

Every pane has a group membership (default: ungrouped). Broadcast modes:
**off** (active pane only), **group** (all panes in the active pane's group),
**all** (every pane in the window). The router resolves
`event → focused pane → broadcast mode → target PTYs`, and the UI visually
indicates which panes are currently receiving broadcast input. This is a
day-one design constraint on the input router, not a later add-on.

### 5c. Input routing and keybindings

Default keybindings are copied directly from Terminator's current documented
defaults. Core chords: split
vertical/horizontal, close pane, quit, directional focus move, directional
resize, zoom/maximize toggle, new tab (if tabs are in the target phase), and
broadcast mode toggles. All keybindings are remappable via config. Pane/split/
broadcast chords are intercepted by the terminal; every other key passes
through to the running program in the focused pane.

### 5d. Mouse and passthrough policy

Chrome regions (dividers, tab bar, config panel) are always consumed by our
UI. In-grid events are forwarded to the running program only when it has
enabled mouse reporting (SGR mouse mode, surfaced by `alacritty_terminal`). A
modifier (Shift, matching Terminator) forces an in-grid event back to
terminal-level selection regardless of the app's mouse reporting state. Click
focuses a pane; dragging a divider resizes; dragging in-grid selects text.

### 5e. Configuration

A config file on disk with hot reload (watch the file, apply on save), plus an
in-app egui settings panel for the common options. Configurable: keybindings,
transparency level, theme/colors, font + size, scrollback size, default shell,
cursor style. Config format is TOML. Default scrollback is 5000 lines per
pane.

### 5f. Transparency

Uniform transparency across all panes — a single window alpha value, not
per-pane compositing — implemented as the window's clear color plus OS
compositor blending via `winit`. Configurable level.

### 5g. Session persistence

On save (and/or quit), write a session file with the layout tree, each pane's
size/split ratios and group membership, and each pane's current working
directory. On restore, rebuild the tree and spawn a fresh shell in each pane's
saved cwd — never attempt to restart whatever was running.

CWD capture priority: OSC 7 (shell-emitted, already parsed by
`alacritty_terminal`) first, OS-level process cwd lookup as fallback (solid on
Linux/macOS, weak on Windows), home directory as the final fallback. Windows
depends on OSC 7 doing the work.

## 6. Scope and Boundaries

**In scope (v1 core):**

- Nested, drag-resizable pane splits with directional focus, zoom, close +
  rebalance
- Pane grouping with broadcast input (off / group / all), group-aware from the
  start
- Terminator-equivalent default keybindings, remappable via config
- Full mouse support with the chrome/in-grid passthrough policy and
  Shift-override
- egui chrome limited to config panel, menus, and non-grid widgets
- Config file with hot reload plus an in-app settings panel
- Uniform window transparency, configurable
- Lightweight session persistence: layout, split ratios, group membership, and
  per-pane cwd; fresh shells on restore
- Windows, macOS, and Linux support

**Out of scope (explicit non-goals):**

- No tmux-style detachable daemon or session survival across a full quit with
  running processes intact
- No restoring running programs on restore — layout and cwd only
- No per-pane transparency — uniform only
- Not a general multiplexer over SSH or a background server model
- No tabs or multi-window in v1 — deferred indefinitely, revisited only if
  demonstrated need arises. v1's focus is split-pane organization, not tabs.

## 7. Assumptions and Constraints

- The project is a solo/small open-source effort with no fixed deadline;
  scope discipline (no frills) matters more than shipping speed.
- Open source from the start under the MIT license.
- Distributed via GitHub Releases — prebuilt binaries and source tarballs; no
  package-manager integration planned for v1.
- Rust toolchain and the chosen crates (`wgpu`, `winit`, `alacritty_terminal`,
  `portable-pty`, `cosmic-text`, `egui`) must behave consistently across
  Windows, macOS, and Linux; cross-platform verification is a dedicated build
  step (§6 build order, step 8), not assumed to fall out for free.
- `wgpu` rendering depends on a working GPU backend (Vulkan/Metal/DX12/GL) on
  each target platform.
- OSC 7 emission varies by shell; cwd capture is assumed reliable on
  Linux/macOS shells that emit it, and weaker on Windows, where it is the only
  practical source (process-cwd lookup is weak there).
- Uniform transparency depends on OS compositor blending behavior, which
  varies across Windows (DWM), macOS, and Linux (X11/Wayland) — visual parity
  across platforms is not guaranteed without testing each.
- No plugin system, extension marketplace, AI integration, or telemetry is
  planned; this is a firm boundary against the "opinionated bloat" the project
  exists to avoid.

## 8. Open Questions

| # | Question | Owner | Needed By |
|---|----------|-------|-----------|
| 1 | Default theme/color scheme and bundled presets | developer | Before chrome/theme design (Planning) |
