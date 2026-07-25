# Plan: v1 Build

**Status:** proposed — pending review
**Fed by:** `.waypoint/conops.md` §6 (build order), `.waypoint/design/*`

Milestones are sequential and match the CONOPS's suggested de-risking order.
Within a milestone, tasks are ordered so later tasks depend on earlier ones in
the same milestone unless noted otherwise. Each task's acceptance criterion is
how we know it's done — not a test-suite requirement, but the observable
behavior to check for.

---

## Milestone 0 — Project scaffolding

- **0.1 Cargo workspace.** Set up a workspace with one crate per major
  component per `opord.md` §3e: `pane` (PTY + `alacritty_terminal` wrapper),
  `layout` (split tree), `router` (input/broadcast), `config`, `render`
  (wgpu + cosmic-text), `app` (winit event loop, binary entrypoint). Add
  `LICENSE` (MIT), `.gitignore`, README skeleton, empty `CHANGELOG.md`.
  *Acceptance:* `cargo build` succeeds across all placeholder crates.
  *Depends on:* nothing.

## Milestone 1 — Single pane, one OS

De-risks the full vertical slice before any pane-management complexity.
Primary dev OS first (Linux); Windows/macOS come back in Milestone 8.

- **1.1 PTY wrapper.** Wrap `portable-pty` to spawn a shell, exposing a
  read/write byte-stream handle. *Acceptance:* spawns `$SHELL`, captures raw
  output, accepts input bytes. *Depends on:* 0.1.
- **1.2 Terminal state.** Feed PTY output through `alacritty_terminal`;
  expose a grid/cursor snapshot. *Acceptance:* unit tests against known VT
  sequences produce the expected grid contents. *Depends on:* 1.1.
- **1.3 Window + GPU context.** `winit` window, `wgpu` context, clear color,
  resize handling. *Acceptance:* window opens, resizes, closes cleanly.
  *Depends on:* 0.1.
- **1.4 Text rendering.** `cosmic-text` shaping + glyph rendering of the grid
  via `wgpu`. *Acceptance:* a single pane's grid renders on screen, readable
  monospace text, visible cursor. *Depends on:* 1.2, 1.3.
- **1.5 Keyboard passthrough.** Keyboard events write straight to the PTY, no
  chords yet. *Acceptance:* can interactively use a shell in the one pane —
  run commands, see output. *Depends on:* 1.4.

## Milestone 2 — Splits (layout tree)

Implements `.waypoint/design/layout-tree.md`.

- **2.1 Tree operations.** `Node`/`Layout` types; split, close+rebalance,
  resize (ratio), zoom. *Acceptance:* unit tests cover each operation.
  *Depends on:* 0.1.
- **2.2 Rect computation.** Per-frame walk of the tree into screen rects.
  *Acceptance:* for a given tree + window size, computed rects tile the
  window with no gaps or overlaps. *Depends on:* 2.1.
- **2.3 Multi-pane rendering.** Renderer walks the tree, draws each pane's
  grid plus dividers. *Acceptance:* two or more panes render independently,
  each running its own shell. *Depends on:* 2.2, Milestone 1.
- **2.4 Directional focus.** Keyboard-triggered focus move (placeholder
  chords ok). *Acceptance:* focus moves correctly in all four directions
  across a nested layout. *Depends on:* 2.3.
- **2.5 Divider drag-resize.** Mouse-driven ratio adjustment.
  *Acceptance:* dragging a divider live-resizes both sides smoothly.
  *Depends on:* 2.3.
- **2.6 Zoom toggle.** *Acceptance:* toggling zoom fills the window with the
  focused pane and restores exactly on toggle-back. *Depends on:* 2.3.
- **2.7 Close + teardown.** Wire tree close to real pane teardown (kill PTY,
  drop term state). *Acceptance:* closing a pane frees its resources and the
  sibling expands to fill the space; closing the last pane quits the app.
  *Depends on:* 2.1, Milestone 1.

## Milestone 3 — Input routing + grouping

Implements `.waypoint/design/input-router.md`.

- **3.1 Keymap.** Chord table with default Terminator-equivalent bindings for
  split/close/quit/focus/resize/zoom (copied from Terminator's current docs
  per CONOPS §5c). *Acceptance:* every core chord triggers its action; unbound
  keys pass through to the pane. *Depends on:* Milestone 2.
- **3.2 Group assignment.** Minimal command/chord to group and ungroup panes
  (exact chord TBD — not in Terminator's default set, needs a decision during
  this task). *Acceptance:* two or more panes can be placed in the same
  group, confirmed via the visual indicator (3.4). *Depends on:* 3.1.
- **3.3 Broadcast resolution + fan-out.** Off/group/all modes; resolve target
  set per keystroke; write to every target pane's PTY. *Acceptance:* typing
  into a grouped pane with mode=group sends input to every group member's
  shell simultaneously. *Depends on:* 3.2.
- **3.4 Broadcast indicator.** Highlighted border on current target panes.
  *Acceptance:* border appears/disappears correctly as mode and focus change.
  *Depends on:* 3.3.

## Milestone 4 — Mouse

Divider drag (2.5) and click-to-focus groundwork already exist; this
milestone covers in-grid passthrough policy.

- **4.1 Click-to-focus.** *Acceptance:* clicking any pane focuses it before
  any other mouse handling occurs. *Depends on:* Milestone 2.
- **4.2 SGR passthrough.** Forward in-grid events to the PTY when the pane has
  mouse reporting enabled. *Acceptance:* verified against a program that
  turns on mouse reporting (e.g. `vim`, `htop`) — clicks/drags reach it.
  *Depends on:* 4.1.
- **4.3 Text selection.** In-grid click-drag selects text when mouse
  reporting is off. *Acceptance:* selecting text in a plain shell prompt
  works and is copyable. *Depends on:* 4.1.
- **4.4 Shift override.** Holding Shift forces terminal-level selection even
  when reporting is on. *Acceptance:* Shift-drag over `vim`/`htop` selects
  text instead of sending mouse events to the program. *Depends on:* 4.2, 4.3.

## Milestone 5 — Chrome + config

Implements `.waypoint/design/config-system.md`.

- **5.1 Config load.** TOML parse into `Config`, defaults when the file is
  absent. *Acceptance:* missing file runs with defaults; present file
  overrides correctly. *Depends on:* 0.1.
- **5.2 Hot reload.** Filesystem watcher; re-parse on change; keep last-good
  config and surface a non-blocking error on a bad edit. *Acceptance:* editing
  `config.toml` while running applies the change live; an invalid edit
  doesn't crash or blank the session. *Depends on:* 5.1.
- **5.3 Keybinding overrides.** Wire `[keybindings]` overrides onto the
  built-in keymap from 3.1. *Acceptance:* remapping a chord in the config
  takes effect on reload; unmapping via `"none"` un-binds without a
  replacement. *Depends on:* 5.2, 3.1.
- **5.4 Settings panel.** egui panel for font, transparency, scrollback lines,
  default shell, cursor style, and keybinding display; saves back through the
  same reload path as 5.2. *Acceptance:* changing a setting in-panel updates
  live state and persists to `config.toml`. *Note:* theme selection UI is
  blocked on the still-open theme/preset decision (CONOPS §8) — ship this
  task with a single built-in default theme and no picker until that's
  resolved. *Depends on:* 5.2.

## Milestone 6 — Transparency

- **6.1 Alpha wiring.** Window alpha via `wgpu` clear color + `winit`
  transparent-window support. *Acceptance:* window visibly shows desktop
  content behind it at a non-1.0 alpha. *Depends on:* Milestone 1.
- **6.2 Config-driven level.** Transparency level reads from config and
  hot-reloads. *Acceptance:* changing the level in `config.toml` or the
  settings panel updates the running window without restart. *Depends on:*
  6.1, 5.2.

## Milestone 7 — Session file

Implements `.waypoint/conops.md` §5g.

- **7.1 OSC 7 cwd capture.** Track each pane's latest cwd from the OSC 7
  sequences `alacritty_terminal` already parses. *Acceptance:* changing
  directory in a pane's shell updates its tracked cwd. *Depends on:*
  Milestone 1.
- **7.2 Cwd fallbacks.** OS-level process cwd lookup (Linux/macOS), home
  directory as the final fallback. *Acceptance:* a pane with no OSC 7 support
  still has a sane cwd recorded. *Depends on:* 7.1.
- **7.3 Session save.** Serialize layout tree, ratios, group membership, and
  per-pane cwd to a session file on save/quit. *Acceptance:* session file
  contents match the live layout at save time. *Depends on:* 7.2, Milestone 2,
  Milestone 3.
- **7.4 Session restore.** Rebuild the tree and spawn fresh shells in saved
  cwds. *Acceptance:* quit and relaunch reproduces the same layout with
  correct cwds and fresh (not resumed) shells. *Depends on:* 7.3.

## Milestone 8 — Cross-platform pass

- **8.1 Windows.** Validate the ConPTY path via `portable-pty`; confirm OSC 7
  cwd capture on PowerShell (the primary fallback source per CONOPS §7).
  *Depends on:* Milestones 1–7 complete on Linux.
- **8.2 macOS.** Bring up windowing, font discovery, and compositor
  transparency. *Depends on:* Milestones 1–7 complete on Linux.
- **8.3 OSC 7 verification.** Confirm cwd capture across bash/zsh/fish (and
  PowerShell per 8.1). *Depends on:* 8.1, 8.2.
- **8.4 Transparency parity.** Check compositor blending behavior on DWM
  (Windows), Quartz (macOS), and X11/Wayland (Linux); document any per-OS
  differences. *Depends on:* 8.1, 8.2, Milestone 6.
- **8.5 Release packaging.** Build binaries for all three OSes plus a source
  tarball; publish via GitHub Releases per CONOPS §7. *Acceptance:* a fresh
  checkout on each OS builds and runs the full v1 feature set; release
  artifacts are produced by a documented, repeatable process. *Depends on:*
  8.1–8.4.

---

## Notes

- Milestone boundaries are also natural commit/review points — each one
  produces a runnable, demoable state.
- Task 5.4's theme picker is the one place this plan is explicitly blocked on
  an open CONOPS question; everything else can proceed independent of it.
