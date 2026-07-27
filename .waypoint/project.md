# Project state

**Phase:** Released and iterating. All of `.waypoint/plan/v1-build-plan.md`'s
milestones are done, and the project has shipped **v1.0.0 through v1.4.0**
publicly from `github.com/w-p/pain`.

The product name is settled: **pain**. It's the crate, the binary, the
config directory, and the published package name — no longer a placeholder.

**Distribution is fully automated.** Pushing a `v*` tag runs
`.github/workflows/release.yml`, which builds four targets, publishes a
GitHub Release with notes taken verbatim from `CHANGELOG.md`'s matching
section, and updates a GPG-signed APT repository on the `gh-pages` branch.
Users get: a Linux tarball, a `.deb`, `apt install`/`apt upgrade` via the
hosted repo, a Windows zip, and a universal macOS `.app` (Intel + Apple
Silicon in one download). Cutting a release is driven by
`.waypoint/skills/version-bump.md`, which decides the semver bump from what
changed, confirms the version and commit message, then commits, tags, and
pushes.

**Verification status by platform:** Linux and Windows are exercised
continuously in development. macOS is now covered by a real tester — the
v1.3.0 transparency fix (Metal only ever offers a `PostMultiplied`
composite mode, which the surface setup didn't accept, leaving every Mac
opaque) was confirmed working there. Binaries are unsigned on both macOS
and Windows, so Gatekeeper/SmartScreen warn on first launch; the README
documents the `xattr` workaround.

**Code signing: declined, decided 2026-07-26.** Priced out at ~$99/yr
(Apple Developer Program, which fully clears Gatekeeper) plus ~$120/yr
(Azure Artifact Signing, subject to geographic eligibility) or $215-685/yr
for a traditional Windows cert — note that a plain OV cert does *not*
immediately clear SmartScreen, since reputation accrues with download
volume. Judged not worth the recurring cost for a free project at this
scale. Don't re-raise this as an open question; revisit only if the
developer asks or the unsigned warnings start demonstrably costing
adoption.

**Known deferred items:** scrollback search, named/saved layouts (judged
niche), arm64 Windows/Linux build targets, shader effects and animated
backgrounds (declined as fluff), and the long-standing WSL cwd-tracking
gap. Rendering is a single direct-to-swapchain pass with no offscreen
target, so any post-processing would need that refactor first.

Between Milestone 6 and Milestone 7, the developer requested an out-of-plan
feature — pane title bars, colored/named groups, and related chrome — not
in `v1-build-plan.md` at all (asked about directly, then specified in full
rather than routed through a Planning-phase design doc first). Confirmed
working interactively; follow-up bugs and small feature asks from that pass
(WSL/`conhost.exe` title bug, sRGB title-bar contrast, ANSI/256/true-color
rendering, scrollback, a font-family selector, a "Swap shell" context-menu
action, `--verbose` logging categories) are all implemented and fixed — see
memory log for the full account, including two real bugs found only
through the developer's own hands-on testing.

Milestone 7 (session file) is implemented in full — layout tree, window
size, per-pane cwd, chosen shell, and group membership; save on quit,
auto-restore on next launch, never restarts whatever was running. The
developer's first real-hardware pass found the directory and chosen-shell
halves weren't actually restoring (layout/window size were fine) — chosen
shell was a real gap (nothing tracked which shell a pane was even running),
fixed directly. Directory needed real evidence (the developer's actual
`session.toml`) to pin down: on Windows, neither cwd signal actually works
in practice (a WSL pane's cwd is invisible to Windows entirely — the same
boundary as foreground-process detection; the plain PowerShell pane's
OS-level lookup failed too) — so `pane::integration` now injects OSC 7
shell integration at spawn time for bash and PowerShell (composing with,
never replacing, the user's own dotfiles/profile — same technique iTerm2/
Windows Terminal/VS Code use). cmd.exe and `wsl.exe`'s own inner shell are
explicit, disclosed gaps, not silently dropped. See memory log for the
full account. Layout/window-size restore confirmed working on real
hardware; the shell-integration fix itself and PowerShell specifically are
not yet re-verified there — needs another pass before Milestone 8
(cross-platform pass) starts.

**Shipped:**

- Milestone 0 — Cargo workspace (`crates/pane`, `layout`, `router`, `config`,
  `render`, `app`), MIT license, `.gitignore`, README/CHANGELOG skeletons.
- Milestone 1 — single pane, confirmed on Linux (WSL2 dev loop, CPU/
  llvmpipe rendering) and native Windows (real GPU, AMD Radeon RX 6950 XT):
  `portable-pty` wrapper (`crates/pane`), `alacritty_terminal` grid/cursor
  parsing with `Event::PtyWrite` handling (DSR/cursor-position query
  replies — required for cmd.exe to progress past its startup handshake), a
  `winit` + `wgpu` window with resize handling (`crates/app`), a
  `cosmic-text`-backed glyph atlas and instanced-quad grid renderer with
  pixel-snapped positions and real font-metric cell sizing
  (`crates/render`), and keyboard passthrough (raw text, Backspace/Enter/
  Tab/Escape/arrows, Ctrl+letter control bytes — no chords yet). Diagnostic
  logging gated behind a `--verbose`/`-v` flag (`crates/app/src/verbose.rs`).
- Milestone 2 — splits/layout tree (unit-tested, interactive behavior not
  yet manually verified): binary split tree with rect computation and
  directional-focus adjacency (`crates/layout`, 10 unit tests covering
  split/close/resize/zoom/focus and tiling correctness); `render`'s API
  reworked to be pane-agnostic (`GlyphCell`/`SolidRect` in absolute pixel
  coordinates, so multiple panes' content and dividers all draw in one
  instanced pass); `crates/app` now holds a `HashMap<PaneId, PaneSession>` +
  `Layout` instead of one pane, resizes every visible pane's PTY+grid
  whenever geometry changes (window resize, split, close, zoom), and kills
  a pane's child process automatically on drop
  (`Pty::kill`/`impl Drop for Pty`). Also fixed post-ship: focus-after-close
  now picks the most recently *created* surviving pane (`PaneId` order),
  not tree-traversal order; panes now auto-close when their shell exits on
  its own (typed `exit`), not just via an app-level close action.
- Milestone 3 — input routing + grouping (unit-tested, 9 tests; confirmed
  working interactively): `crates/router` — `Keymap` with
  Terminator's real default bindings, verified directly against its
  `config.py` source (not memory) — split/close/quit/focus/resize/zoom all
  match Terminator exactly. `Router` resolves broadcast targets
  (off/group/all) as a pure function of current state, per the design doc.
  `crates/layout` gained `resize_target` for keyboard-driven resize (finds
  the ancestor split matching a direction's axis). `crates/app`'s
  `Graphics::send_input` now fans out to every broadcast target's PTY, not
  just the focused pane; `redraw()` draws an orange border around every
  pane currently receiving broadcast input when mode isn't Off.
  Grouping/broadcast-mode *control* is UI-driven, not keybindings — see
  below; `Action::ToggleGroup`/`SetBroadcastMode` exist but have no default
  chord after the developer flagged the Windows key as too OS-reserved to
  be a safe default anywhere.
- Milestone 5 (partial, pulled forward) — a right-click context menu
  (`crates/app/src/ui.rs`) for the two things Milestone 3 explicitly
  shouldn't be keybindings: broadcast-mode selection (Off/Group/All) and
  toggling a pane's group membership, matching Terminator's own precedent
  exactly (it only exposes grouping through a context menu, never a
  keybinding or a persistent panel — a first version used an always-visible
  floating `egui::Window` and was revised after developer feedback that
  it's the wrong chrome pattern). Right-click targets whichever pane is
  under the cursor, not necessarily the focused one. Required downgrading
  `wgpu` 30.0.0 → 29.0.4 workspace-wide, since `egui-wgpu` (latest) pins to
  `wgpu 29`; the downgrade needed only two small fixes (`VertexState.buffers`
  un-wraps from `Option`, `present()` moves back to `SurfaceTexture`). The
  full config/settings panel is still Milestone 5's job when its turn comes
  — this is just the slice needed now.
- Milestone 4 — mouse (unit-tested, 5 new tests in `crates/app/src/mouse.rs`;
  confirmed working interactively): click-to-focus (any left
  click on a pane focuses it before other mouse handling runs, ahead of
  reporting/selection); SGR (mode 1006) and legacy normal-tracking mouse
  reporting, forwarding left-button press/release/drag-motion to the PTY as
  the real escape sequences a program that enables mouse mode (`vim`, `htop`,
  ...) expects, decided per-pane from `alacritty_terminal`'s own `TermMode`
  flags rather than anything hand-rolled; in-grid click-drag text selection
  using `alacritty_terminal`'s own `Selection`/`SelectionRange` types when a
  pane's program hasn't turned on reporting, rendered as a highlight and
  copied to the system clipboard (via `arboard`, text-only — its default
  `image-data` feature was trimmed since only text copy is needed) once the
  drag ends, unless the "drag" never actually moved (then it's discarded, not
  left highlighting a single cell); holding Shift always forces local
  selection, bypassing reporting entirely — the standard xterm escape hatch
  for selecting text out of full-screen programs that would otherwise treat
  the click as their own input. Right-click stays reserved for the pane
  context menu unconditionally (never forwarded), matching the same
  chrome-vs-program-input convention essentially every terminal emulator
  uses. Only one pane's selection is ever live at a time — starting a new one
  clears any other pane's leftover highlight.
- Milestone 5 — chrome + config (unit-tested, 9 new tests across `config`
  and `router`; confirmed working interactively — config files are written
  on first save, and editing the file live-updates running terminals as
  expected. The developer also confirmed cursor style and transparency
  changes don't do anything yet, which was expected at that point:
  transparency was explicitly still Milestone 6's job, and cursor-style
  rendering was never in any milestone's acceptance criteria — both round-
  trip through load/save correctly regardless): a new
  `crates/config` implementing `.waypoint/design/config-system.md`'s schema
  (`serde` + `toml`) with per-platform config-file resolution (XDG/AppData/
  Library, working app name "pain" from this repo's own directory — the
  product itself still has no settled name, tracked the same way as the
  open theme question); `Config::load`/`try_load` distinguish "missing"
  (defaults, not an error) from "present but broken" (defaults *with* a
  stderr report for the first load; the *previous* config for a hot
  reload — a bad edit never resets a running session to defaults, only a
  first load has no "previous" to fall back to). `crates/app` loads this
  once at startup (wired to `default_shell` and `appearance.font_size`,
  the only two fields with an observable effect so far — `transparency` is
  explicitly Milestone 6's job to wire into rendering, `scrollback_lines`
  has no effect since scrollback itself isn't implemented yet, and
  `cursor.style` has no rendering effect yet either; all three still round-
  trip through load/save correctly, just inert for now) and watches its
  directory with `notify` for hot reload, re-applying font-size/keybinding
  changes live each frame without needing a restart; watcher-setup failure
  degrades to "no hot reload" rather than failing startup. `crates/router`
  gained `Keymap::apply_overrides` (chord-string/action-name parsing,
  built from scratch since neither `alacritty_terminal` nor any dependency
  owns this — it's app-level policy), rebuilt from `terminator_defaults()`
  on every reload (not patched incrementally) so a removed override
  reverts to the built-in default rather than getting stuck. The right-
  click context menu (`crates/app/src/ui.rs`) gained a "Settings..." entry
  opening an `egui::Window` settings panel (font size, transparency,
  scrollback lines, default shell, cursor style, read-only keybinding-
  override display) — this is the one place in the whole UI so far that's
  a persistent-while-open `egui::Window` rather than an ephemeral `Area`,
  deliberately: Terminator's own Preferences dialog is exactly this shape,
  reached through the same right-click menu (no menu bar to hang it off
  instead), so it doesn't run into the "always-visible panel" objection
  that killed the first attempt at the broadcast/group UI — it's open only
  between "Settings..." and Save/Cancel/close. "Save" writes `config.toml`
  via a new `Config::save` and does *not* apply anything to live state
  directly; the already-running hot-reload watcher picks the write up the
  same way it would a hand edit, exactly the single-apply-path design the
  doc calls for. Theme picker deliberately excluded per the plan's own
  note — still blocked on CONOPS §8.
- Milestone 6 — transparency (no new unit tests — this is GPU/windowing
  wiring with nothing pure-logic to unit test beyond an `f32::clamp`;
  confirmed working interactively on Windows after four post-ship fixes,
  see below): the window is now always
  created transparent-capable (`winit`'s `with_transparent(true)`, set
  unconditionally since it can't be changed after creation, unlike the
  transparency *level* which has to stay hot-reloadable); the wgpu surface
  now explicitly requests `CompositeAlphaMode::PostMultiplied` when the
  adapter offers it (falls back silently — logged only under `--verbose` —
  to whatever `get_default_config`'s `Auto` picks otherwise, typically
  `Opaque`, on a backend/platform without compositing support). Chose
  `PostMultiplied` deliberately: it expects straight, non-premultiplied
  color values, which is exactly what `crates/render`'s existing
  `ALPHA_BLENDING` pipeline already produces — no pipeline changes needed
  at all, just picking the compositor mode that matches what was already
  being rendered. The background clear color's alpha now comes from
  `settings.appearance.transparency` (clamped to 0.0–1.0) every frame
  instead of a fixed 1.0; text, cursor, dividers, selection, and the
  broadcast border all keep their own existing opacity untouched, so only
  genuinely empty cells become see-through — the same convention every
  other terminal emulator's background transparency uses, and it's what
  "over" alpha blending naturally produces for free (a glyph drawn fully
  opaque over a partially-transparent background composites back to fully
  opaque, without any special-casing). Milestone 6.2's "config-driven,
  hot-reloadable level" acceptance criterion needed zero additional code:
  `redraw` already reads `settings.appearance.transparency` fresh every
  single frame, and Milestone 5.2's hot-reload watcher already replaces
  `settings` wholesale on any valid edit — there was no cached/stale value
  anywhere that changing the level needed to invalidate.
  **Post-ship fixes (confirmed working on real Windows):** the initial
  Windows test surfaced four distinct real bugs, each traced to concrete
  evidence (an HRESULT, a D3D12 debug-layer message, a screenshot, and
  diagnostic logging) rather than guessed — full write-up with root
  causes in `vendor/README.md` and the memory log, summary here:
  1. A plain window-handle swapchain only ever reports
     `CompositeAlphaMode::Opaque` on Windows; real transparency needs a
     DirectComposition-backed swapchain (`Dx12BackendOptions.
     presentation_system = Dx12SwapchainKind::DxgiFromVisual`, wgpu backend
     pinned to DX12 on Windows via `platform_backends()`).
  2. `wgpu-hal` 29.0.4 (also 30.0.0) unconditionally sets
     `DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING`, which composition swapchains
     reject outright — a genuine upstream bug, fixed via a local-only fork
     (`vendor/wgpu-hal-29.0.4/`, `[patch.crates-io]` in the workspace
     `Cargo.toml`; deliberately not submitted upstream, see memory log).
  3. Composition swapchains only accept `CompositeAlphaMode::PreMultiplied`
     — this app had been requesting `PostMultiplied` to match the
     renderer's straight-alpha output; fixed properly by switching
     `crates/render`'s pipeline to premultiplied blending (shader now
     folds glyph-edge coverage into the premultiply, not just instance
     alpha) rather than just changing the requested enum value.
  4. Resizing left a frozen, opaque rectangle at the old size with the
     newly-exposed area fully transparent. Root cause turned out to be
     `winit`, not wgpu: it enables an older, GDI-redirection-based
     transparency mechanism (`DwmEnableBlurBehindWindow`) automatically
     alongside the new DirectComposition path unless the window is created
     with `WS_EX_NOREDIRECTIONBITMAP` — two independent transparency
     mechanisms were active on the same window, and the legacy one had no
     resize awareness. Fixed via winit's own public
     `with_no_redirection_bitmap(true)` (`platform_window_attributes` in
     `main.rs`) — also Microsoft's documented recommendation for any app
     presenting through its own swapchain. (A real-but-not-actually-causal
     fix along the way — re-committing the DirectComposition visual on
     resize, since `wgpu-hal`'s own resize path doesn't — is also patched
     into `vendor/wgpu-hal-29.0.4/`; harmless and correct, just wasn't
     this bug.)

  Also added: `env_logger` initialization in `main.rs` (the app had no
  logger installed at all, silently dropping every `log::error!`/`warn!`
  from `wgpu`/`wgpu-hal` — needed to diagnose any of the above).

  Separately, the same first report described WSL as the opposite problem:
  completely see-through (not just empty background) and click-through.
  Standard X11 alpha visuals don't cause click-through on their own — that
  needs an explicit XShape input-region change, which this app never makes
  — so this looked like a WSLg-compositor-specific quirk, consistent with
  this project's established pattern for WSL2/WSLg display quirks (see the
  cursor-theme decision earlier this session). Developer's call: WSL isn't
  a target platform at all (Windows and native Linux are), so rather than
  investigate further, transparency is now disabled outright there —
  `crates/app/src/platform.rs` (new, shared `is_wsl()` — the same check
  `main.rs` already used for the X11-vs-Wayland event loop workaround, now
  reused instead of duplicated): `Graphics::new` skips requesting
  `PreMultiplied` on WSL and `redraw` forces the background alpha to fully
  opaque regardless of the configured level (needed on top of the surface-
  level change, or the premultiplied shader math would still dim every
  color for zero visible benefit); the settings panel's transparency
  slider is disabled (not hidden) under WSL with a one-line explanation.

  First pass at this only addressed the swapchain's requested alpha mode
  and still left the window fully see-through — the developer caught it
  immediately. Missing piece: `.with_transparent(true)` on window
  *creation* is a separate mechanism from the swapchain's alpha mode — on
  X11 it makes winit request a 32-bit ARGB visual for the window itself,
  and WSLg was compositing based on *that*, regardless of what
  `CompositeAlphaMode` the swapchain asked for. Same shape of bug as the
  Windows DirectComposition-vs-redirection-bitmap issue earlier this
  session: two independent transparency mechanisms, only one of which had
  been turned off. Fixed by also making the window-creation-time
  `with_transparent(true)` call itself conditional on `!is_wsl()`.

  Verified directly both times (this dev environment *is* WSL, so unlike
  everything else in this investigation this didn't need the developer's
  separate hardware) — confirmed via `--verbose`, comparing before/after:
  with only the swapchain-level fix, `caps.alpha_modes` still reported
  `[PreMultiplied, Inherit]` (driven by the still-ARGB window); with the
  window-creation fix added, the same surface now reports `[Opaque,
  Inherit]` — direct proof the window itself is no longer ARGB, not just
  an assumption that it should be.
- **Out-of-plan: pane title bars + named/colored groups** (unit-tested, 8
  new tests in `router`; interactive behavior not yet manually verified) —
  requested directly by the developer, not in `v1-build-plan.md`. Every
  pane now has a title bar reserved from the top of its rect (`Graphics::
  content_rect`/`title_bar_height`, scaled to font size — every place that
  converts a pane rect to grid rows/cols, or positions text/cursor/
  selection, now goes through this instead of the raw pane rect):
  - Default: dark grey background, light grey text, the pane's actual
    foreground process name centered (see its own entry below — this
    replaced an initial OSC-title-based attempt that didn't hold up),
    falling back to `"shell"` if nothing could be determined.
  - Grouping is now fully redesigned: `router::GroupId` wraps a
    user-chosen `String` name directly (was an opaque `u64` with one
    hardcoded default group) — `Router::assign_to_group(pane, name)`
    creates the group if new and moves the pane into it (removing it from
    any previous group first), `remove_from_group(pane)` removes it,
    deleting the group entirely once empty, `group_names()` lists every
    group with a member for the context menu's picker.
    `Action::ToggleGroup` is gone — grouping needs a name a keyboard chord
    can't carry, so this is UI-only in a way that isn't just "no default
    binding" anymore, it has no `Action` variant at all.
  - A grouped pane's title bar background is picked from a 10-color
    palette (`GROUP_COLOR_PALETTE`) keyed by a hash of the group's name —
    deterministic, not re-rolled on every creation, so a group's color
    stays stable across reloads/restarts/reassignments rather than
    flickering; text color is computed by perceived luminance (light text
    on a dark-picked color, dark text on light) — the group name is also
    shown, left-aligned, alongside the still-centered title.
  - Context menu gained both split commands (previously keyboard-only) and
    a full group-assignment UI (new-name text field + "Add", plus a
    dropdown of existing group names) replacing the old single toggle
    button, targeting whichever pane was right-clicked specifically — this
    also fixed a latent mismatch where splitting via the context menu
    would have split the *focused* pane regardless of which one was
    right-clicked (`Graphics::split` is now `split_pane(pane, ...)` under
    the hood, with `split(orientation)` as a focused-pane convenience
    wrapper for the keyboard-chord path).
  - New `appearance.background_color` config field (`#rrggbb` hex, default
    black — matches most terminals' own default), with a color picker in
    the settings panel (`egui::color_edit_button_rgb`); parse failures on
    a hand-edited value fall back to black rather than erroring, same
    "never crash on a bad edit" convention as the rest of config.
  - Clicking/dragging within a pane's title bar strip focuses it and opens
    its context menu (via the existing full-pane-rect hit test) but does
    *not* start a text selection or forward a mouse report — `cell_at` now
    returns `None` above the content rect rather than clamping into it,
    so title-bar clicks fall through to focus-only.
- **Out-of-plan: real foreground-process detection for the title bar**
  (unit-tested — 4 new tests in `app`, including one spawning a real shell
  through the real `pane::Pty` and running a real command in it, not just
  a raw `std::process` stand-in). The developer asked for OSC-title-based
  naming to only show the application, not the host/path banner it was
  actually showing — investigating why revealed the real problem: most
  shells' default prompt only sets the OSC title at the *prompt* (host +
  cwd), never while a command is actually running, so it could never have
  answered "what's running now" regardless of trimming. Asked directly
  whether to invest in real OS-level foreground-process detection instead
  of patching the symptom — developer: "Nope... I want the foreground
  process. For sure." Replaced the OSC-title mechanism entirely (removed
  `pane::Screen`'s title tracking added earlier this session — dead code
  once superseded, not kept around) with:
  - `pane::Pty::foreground_pgid()` (Unix only) — `portable_pty`'s own
    `process_group_leader()`, `tcgetpgrp` on the pty master, already built
    into the dependency; a shell puts each foreground job in its own
    process group led by the job itself, so this is the correct, direct
    signal, not a heuristic.
  - `crates/app/src/foreground_process.rs`'s `ForegroundProcesses` — a
    `sysinfo`-backed (added to `app` only, trimmed to its `system` feature)
    shared, throttled (500ms) process-list snapshot. Prefers the Unix pgid
    signal; otherwise (always on Windows, which has no equivalent concept)
    walks the process tree down from the shell's own pid, picking the
    most-recently-started live child at each level as a best-effort
    approximation of "the current job" — this is a real, disclosed
    limitation on Windows specifically (approximate, not authoritative),
    not present on Unix.
  - Verified directly here (WSL): a genuine unit test spawns a real
    `pane::Pty`, writes `sleep 5\n` to it, and confirms `foreground_pgid`
    + the lookup correctly resolve to `"sleep"` — not just "compiles,"
    the actual pipeline verified end to end on the platform available
    here. The Windows tree-walk path remains unverified beyond compiling
    and unit-testing its own logic in isolation — needs the developer's
    real hardware, same as everything Windows-specific this session.
- **Out-of-plan: Windows-only default-shell quick-pick** — settings panel
  gained three Windows-only (`#[cfg(target_os = "windows")]`) buttons
  (Command Prompt/PowerShell/WSL) that fill in the existing free-text
  `default_shell` field with `cmd.exe`/`powershell.exe`/`wsl.exe` — asked
  for directly since Windows, unlike Linux/macOS (one obvious default,
  already picked up by leaving the field empty), has no single obvious
  shell choice. The field itself is untouched and still takes any custom
  value (a specific WSL distro invocation, `pwsh.exe`, ...).
- **Out-of-plan: "Swap shell" context-menu action, ANSI/256/true-color
  rendering, scrollback, a font-family selector, `--verbose` logging
  categories** — see memory log for the full account. `Graphics::
  restart_pane_shell` replaces one pane's shell in place (no effect on
  layout/group/broadcast state) — the fix for a pane whose foreground-
  process detection can't see past a Windows→WSL2 boundary. `crates/app/
  src/color.rs` resolves real per-cell SGR colors instead of one hardcoded
  gray. `pane::Screen::scroll`/`scroll_to_bottom` plus a new `MouseWheel`
  handler expose scrollback `alacritty_terminal` already retained by
  default but never exposed. The font picker lists every monospaced font
  actually installed (`cosmic-text`'s font database), not free text.
  `crate::verbose::Category` (`General`/`Mouse`/`Pty`/`Foreground`) replaced
  a single boolean flag so bare `--verbose` isn't drowned out by
  high-frequency streams.
- **Milestone 7 — session file** (implemented, not yet interactively
  verified — see memory log): layout tree, window size, and per-pane cwd/
  group membership; save on quit, auto-restore on next launch, never
  restarts whatever was running. `pane::cwd`'s `CwdWatcher` is a
  self-contained OSC 7 scanner independent of `vte`/`alacritty_terminal`
  (checked directly: neither actually parses OSC 7 at all, despite CONOPS
  §5g's assumption) — patching either was judged a bigger, two-crate fork
  than the existing single-file `wgpu-hal` precedent, so this hand-rolls
  the one narrow, stable escape sequence instead. `layout::SavedNode` +
  `Layout::snapshot`/`from_snapshot` serialize the tree's shape without
  pane identity (ids never survive a restart) — restored per-pane state is
  correlated *positionally*, both the snapshot's leaves and `Layout::
  panes()` walking the tree in the same order. New `session` crate
  mirrors `config::Config`'s load/save conventions but stays a separate
  file (`session.toml`, `config::dir()` reused for the location) so an
  automatic save on every quit can never clobber a hand-edited
  `config.toml`.

**Approach**

- New terminal emulator, not an Alacritty fork: `alacritty_terminal` +
  `portable-pty` for the VT/PTY backend, `winit` + `wgpu` + `cosmic-text` for
  windowing/rendering/font shaping, `egui` (on the same `wgpu` context) scoped
  to config panel/menus/non-grid widgets only.
- Pane layout is a binary split tree, owned and rendered by us — not
  delegated to egui.
- Input router is group-aware from day one: broadcast modes off / group / all.
- Session persistence is layout + cwd only; never restores running programs.
- Language: Rust, Cargo workspace, one crate per major component. Config
  format: TOML. Default scrollback: 5000 lines/pane.
- `wgpu` is pinned to 29.x workspace-wide, not the newest 30.x, because
  `egui-wgpu` (latest release) requires `wgpu = "29.0"` — revisit the pin
  once egui-wgpu catches up to a newer wgpu.
- `vendor/wgpu-hal-29.0.4/` is a local-only fork (Cargo `[patch.crates-io]`
  in the workspace `Cargo.toml`) with one targeted fix for a real upstream
  bug blocking Windows transparency — see `vendor/README.md`. Deliberately
  not submitted upstream; that's a separate future decision, not a default
  next step.
- Keybindings copied directly from Terminator's current documented defaults.
- Open source under MIT from the start; distributed via GitHub Releases
  (binaries + source tarballs), no package-manager integration in v1.

**Deferred**

- Tabs — deferred pending a design discussion, not for lack of demand: users
  have now asked for them (2026-07-27). Noted here rather than started because
  the model change is real — window → tabs → layout tree → panes, which
  touches session persistence, the tab-bar chrome, and chord assignments. Two
  questions to settle first: whether `Ctrl+Shift+W` closes the pane or the
  tab, and whether broadcast-all means the current tab or every tab. The
  non-obvious implementation risk: a background tab's PTYs still have to be
  drained even though nothing renders them, or the pipe buffer fills and the
  program blocks — a tab you switched away from would silently freeze.
- Multi-window — indefinitely, until there's demonstrated need.
- Default theme/color scheme and bundled presets — still undecided, see
  CONOPS §8.

**Ground truth docs**

- Intent: `.waypoint/conops.md`
- Standing orders: `.waypoint/opord.md`
- Design: `.waypoint/design/layout-tree.md`, `.waypoint/design/input-router.md`,
  `.waypoint/design/config-system.md`

**Plan**

- `.waypoint/plan/v1-build-plan.md` — 9 milestones (scaffolding through
  cross-platform release), following the CONOPS §6 build order
