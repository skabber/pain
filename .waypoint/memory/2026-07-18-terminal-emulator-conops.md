# Session: Terminal emulator — CONOPS

- **2026-07-18:** Started a new project: a cross-platform (Windows/macOS/Linux)
  terminal emulator with a Terminator-style multi-pane UX, built on Alacritty's
  reusable backend crates rather than forking Alacritty. Developer supplied a
  detailed spec covering tech stack, architecture, feature requirements, and
  build order; we walked it through the `new-project` skill to converge on a
  CONOPS.

  Resolved during the conversation:
  - Audience is the developer and people like them — cross-platform users who
    want Terminator's pane model without Terminator's Linux-only constraint or
    other terminals' feature bloat (no AI/plugins/telemetry planned, ever).
  - Open source under MIT from the start; distributed via GitHub Releases
    (binaries + source tarballs), no package manager integration for v1.
  - Config format: TOML. Default scrollback: 5000 lines per pane.
  - Keybindings: copy Terminator's current documented defaults directly (no
    reinvention).
  - Tabs/multi-window: deferred indefinitely — v1 is split-panes only, revisit
    only on demonstrated need.
  - Still open: default theme/color scheme and bundled presets (CONOPS §8).

  Wrote `.waypoint/conops.md` (8 sections per template). Extended
  `.waypoint/opord.md` §3e with Rust-specific code standards (rustfmt, clippy,
  Result/? error handling, Cargo workspace layout, `cargo test`). Updated
  `.waypoint/project.md`: phase is Ideation, ready to move into Planning next.

  **Update — same session, continued:** Developer approved the CONOPS as-is.
  Moved into Ideation's design output: wrote three design docs —
  `.waypoint/design/layout-tree.md` (split-tree data model: `Node::Split`/
  `Node::Leaf`, ratio-based dividers, rebalance-by-promoting-sibling on
  close, zoom as a `Layout`-level flag not a tree mutation),
  `.waypoint/design/input-router.md` (chord-or-passthrough keymap, broadcast
  target resolution as a pure function of focused pane + mode + groups,
  mouse hit-test order: chrome → SGR passthrough → terminal selection, with
  Shift override), and `.waypoint/design/config-system.md` (TOML schema,
  filesystem-watcher hot reload, settings panel writes through the same
  reload path as a hand-edited file). One open item carried from
  layout-tree.md: whether closing the zoomed pane should auto-clear zoom
  (proposed yes, not yet challenged).

  Developer approved all three design docs. Wrote
  `.waypoint/plan/v1-build-plan.md`: 9 milestones (0 scaffolding, 1 single
  pane, 2 splits, 3 input routing/grouping, 4 mouse, 5 chrome+config,
  6 transparency, 7 session file, 8 cross-platform pass), each broken into
  tasks with acceptance criteria and dependencies, following the CONOPS §6
  build order. Milestone 5's task 5.4 (settings panel) is explicitly flagged
  as blocked on the still-open theme/preset decision (CONOPS §8) — plan says
  ship with one built-in default theme and no picker until that's resolved.

  Updated `.waypoint/project.md`: phase is now Planning, ground-truth index
  lists the three design docs, Plan section points to the v1 build plan.

  **Update — same session, continued:** Developer approved the plan and said
  proceed. Before starting Execution, discovered this repo
  (`/mnt/c/Users/Will/code/pain`) was actually a clone of the Waypoint
  framework's own source repo (`github.com/w-p/waypoint`) with a pile of
  uncommitted deletions of its framework files (README, LICENSE, adapters/,
  skills/, templates/, the `waypoint` script) — unrelated to this project,
  predating the session. Flagged this before writing any code. Developer
  confirmed it was an accidental clone, removed `.git` themselves, and
  confirmed this directory is the correct home for the terminal emulator.

  Ran `git init` for a fresh repo. No Rust toolchain was present in the
  environment (no `cargo`/`rustup`); confirmed with developer before
  installing, then installed via the official rustup script (stable
  toolchain, default profile) — `cargo 1.97.1`, `rustc 1.97.1`.

  Executed Milestone 0 (project scaffolding) from
  `.waypoint/plan/v1-build-plan.md`: Cargo workspace at the repo root with
  members `crates/pane`, `crates/layout`, `crates/router`, `crates/config`,
  `crates/render` (libs) and `crates/app` (bin), shared version/edition/
  license via `[workspace.package]`. Added `.gitignore` (`/target`), `LICENSE`
  (MIT, Will Palmer, 2026), `README.md` skeleton, `CHANGELOG.md`. Verified:
  `cargo build` and `cargo clippy --all` both succeed clean across the whole
  workspace. Nothing committed to git yet — no commit has been made in this
  session, pending developer confirmation per the "never commit without being
  asked" rule.

  **Update — same session, continued:** Developer said not to commit until
  something is working and they've reviewed it, regardless of milestone
  acceptance criteria being met — saved as a standing feedback memory
  (`feedback_commit_timing` in the auto-memory store) so future sessions
  don't propose committing prematurely. Proceeded with Milestone 1 on that
  basis (still uncommitted).

  Built all five Milestone 1 tasks:
  - **1.1** `crates/pane`: `Pty` wraps `portable-pty` (spawn, write, resize,
    clone reader, has_exited). Test spawns a real shell and confirms echoed
    output round-trips.
  - **1.2** `crates/pane/src/term.rs`: `Screen` wraps `alacritty_terminal`'s
    `Term` + a `vte::ansi::Processor`; `advance()` feeds raw PTY bytes,
    `visible_rows()`/`cursor()` expose grid state. Tests verify known VT
    sequences (plain text, cursor-positioning escapes) land in the expected
    cells.
  - **1.3** `crates/app`: winit `ApplicationHandler` + wgpu surface/adapter/
    device/config bootstrap (`crates/app/src/graphics.rs`), resize handling.
  - **1.4** `crates/render`: `GlyphRasterizer` (cosmic-text swash
    rasterization, one glyph at a time — monospace grids don't need shaping),
    `GlyphAtlas` (shelf-packed R8Unorm texture, solid white block reserved
    for the cursor quad), and `GridRenderer` (wgpu pipeline: instanced quads,
    one draw call per frame, alpha-blended coverage mask sampling). Wired
    into `app`'s `Graphics::redraw()` alongside a `PaneSession` that spawns a
    real shell and pumps its output into the `Screen` every frame.
  - **1.5** `crates/app/src/main.rs`: `WindowEvent::KeyboardInput` →
    `key_bytes()` (winit's composed `text` for printable chars, a small
    `NamedKey` match for Enter/Backspace/Tab/Escape/arrows) → written straight
    to the pane's PTY. Switched the event loop to `ControlFlow::Poll` +
    `request_redraw()` in `about_to_wait` so async PTY output shows up
    promptly (a real wake-channel would avoid the busy loop; deferred as an
    efficiency concern, not correctness).

  All crate-level API usage (`portable-pty` 0.9, `alacritty_terminal` 0.26,
  `winit` 0.30, `wgpu` 30.0 — note wgpu 30 renamed `ImageCopyTexture` →
  `TexelCopyTextureInfo` and `push_constant_ranges` → `immediate_size`,
  and `RenderPipelineDescriptor.multiview` → `multiview_mask`; `cosmic-text`
  0.19) was verified against the actual vendored source under
  `~/.cargo/registry/src/.../<crate>-<version>/`, not from memory, since
  training-data knowledge of exact field names/signatures for fast-moving
  GPU crates is unreliable. `cargo build`/`clippy --all-targets`/`test`
  are clean across the whole workspace.

  **Environment caveat found and flagged to developer:** this sandbox's
  `/dev/dri/*` render nodes are permission-denied (owned by root, mode 600),
  and screenshot tooling (`import -window root`) fails with "Resource
  temporarily unavailable" — likely a sandbox restriction, not a WSLg
  limitation per se. The app still runs without crashing (adapter/device
  request succeeds, falls back off a software/WARP-ish path under WSLg,
  only benign `libEGL` warnings printed) but this could not be visually
  screenshotted from within the session. Asked the developer to eyeball
  `cargo run -p app` themselves to confirm real rendering.

  **Update — same session, continued:** Developer confirmed Milestone 1
  actually renders and works, but reported a real bug: on window close *and*
  on window focus loss, the app dies with two `Io error: Broken pipe (os
  error 32)` lines and `Error: Exit Failure: 1`. Traced the exact string
  "Io error: {0}" to `WaylandError::Io`'s `Display` impl in the
  `wayland-backend` crate (vendored source, not guessed) — winit's Wayland
  backend was in use because `WAYLAND_DISPLAY` is set under WSLg, and
  WSLg's Wayland compositor apparently drops the client connection on focus
  change, which winit surfaces as a fatal `EventLoopError` that propagates
  out of `run_app` and kills the whole process via `main`'s `?`.

  Fix: `crates/app/src/main.rs` now has `build_event_loop()`, which on Linux
  checks for `WSL_DISTRO_NAME`/`WSL_INTEROP` (both reliably set by WSL) and
  forces winit's X11 backend (`EventLoopBuilderExtX11::with_x11`) when
  present — X11-via-XWayland is far more stable under WSLg than raw Wayland.
  Non-WSL Linux keeps winit's normal Wayland-preferred autodetection
  unchanged; other OSes are unaffected (function is Linux-only via `cfg`).
  This structurally eliminates the `WaylandError` class under WSL since the
  X11 backend never opens a Wayland connection — verified the reasoning via
  winit's vendored backend-selection source, but could not reproduce the
  focus-loss trigger myself in this sandbox (no `xdotool`/`wmctrl` to
  simulate window focus changes). Rebuilt clean; asked developer to retest
  focus-loss and close behavior themselves.

  **Important note for future sessions:** this repo's dev loop runs on
  WSL2 (Ubuntu). The CONOPS's cross-platform target is Windows/macOS/Linux
  natively — WSL2 is a development/testing convenience, not a target
  platform in its own right, but WSLg-specific quirks (this Wayland one,
  the earlier `/dev/dri` permission and screenshot-tooling gaps) will keep
  surfacing during iteration here and are worth checking against `is_wsl()`-
  style guards rather than changing default cross-platform behavior.

  **Update — same session, continued:** Focus-loss fix confirmed working.
  Developer then reported text rendering "garbled, not native and clean,"
  plus asked to understand the recurring `/dev/dri` permission warnings.

  Root-caused the garbling: `crates/app/src/graphics.rs` had a hardcoded
  `CELL_WIDTH = 9.6` (a guessed monospace aspect ratio, not derived from the
  font). Every grid column except multiples of 5 landed on a fractional
  pixel boundary; combined with linear texture filtering on a glyph atlas
  with zero padding between packed glyphs, fractional positions bled
  adjacent glyphs into each other. Fixed three ways: (1) added
  `render::measure_cell()` (`crates/render/src/lib.rs`) — shapes 'M' via a
  new `GlyphRasterizer::advance_width()` to get the font's real advance
  width instead of guessing, rounded to a whole pixel; (2) rounded every
  glyph/cursor quad's screen position to whole pixels in
  `GridRenderer::render`; (3) switched the atlas sampler from linear to
  nearest filtering (`crates/render/src/lib.rs`) since glyphs are always
  drawn at exact 1:1 texel scale — linear bought nothing but bleed risk —
  and added a 1px gap between packed glyphs in `GlyphAtlas::alloc`
  (`crates/render/src/atlas.rs`) as a second line of defense.

  Diagnosed `/dev/dri`: added a one-line `adapter.get_info()` diagnostic at
  startup (`crates/app/src/graphics.rs`). It reports
  `llvmpipe (LLVM 15.0.7, 256 bits) (Vulkan backend, Cpu, ...)` — confirming
  we're on Mesa's CPU software Vulkan rasterizer, not hardware acceleration.
  Investigated why: `/dev/dri/card0`/`renderD128` are mode 600 root-owned;
  `will` isn't in the `render` group (gid 110, confirmed via `getent group
  render`). `/dev/dxg` (WSL2's real GPU passthrough device) is mode 666 and
  accessible, but no Vulkan-over-D3D12 ("Dozen"/dzn) ICD is installed in
  this distro (`/usr/share/vulkan/icd.d/` has only intel/radeon/virtio/lvp),
  so fixing the permission alone may not yield real acceleration — this is
  now recorded in the auto-memory project note
  ([[project-wsl2-dev-environment]] equivalent) rather than re-derived each
  session. Important: llvmpipe is spec-correct, not the cause of the
  garbling — that was purely our own pixel-snapping bug above, now fixed.

  Asked developer whether to pursue the GPU fix; they said yes. Tried
  `sudo usermod -aG render will` via the Bash tool — failed, this
  environment's sudo requires an interactive password I can't supply.
  Handed the exact command back to the developer to run in their own
  terminal, plus the required `wsl --shutdown` + restart for the group
  change to take effect (flagged that this will also kill any Claude Code
  session running inside the same WSL instance, since it's a full VM
  shutdown, not just a shell restart).

  **Update — same session, continued:** Garbling fix confirmed (developer
  didn't push back — implicitly good). Developer tried the `render`-group
  fix for WSL2 GPU access (`sudo usermod -aG render will`); learned along
  the way that `su -l` does a full login-environment reset that silently
  drops the WSLg-injected `DISPLAY`/`WAYLAND_DISPLAY` vars (they're not set
  by any profile script, just injected once into the original `wsl.exe`
  session), while `su $USER` (no `-l`) refreshes group membership without
  that side effect — worth remembering for any future WSL account changes.
  After the group fix, winit still failed with "Could not find wayland
  compositor" — WSL2/WSLg fundamentally has no path to real GPU-accelerated
  Vulkan here (see updated auto-memory project note). Developer decided:
  stop chasing WSL2 GPU acceleration, accept CPU/llvmpipe for the dev loop,
  test real GPU rendering on native Windows instead (an actual target
  platform, unlike WSL2).

  Moved to testing on native Windows via PowerShell. Hit two more real bugs,
  found through iterative diagnostic logging (added and then mostly kept —
  see below) rather than guessing, since I have no Windows access myself:

  1. **Windows Rust toolchain too old for edition 2024** — resolved by the
     developer running `rustup update`.
  2. **Window opened, background + cursor rendered, but no shell output at
     all and no interactivity** ("shell process isn't starting up
     correctly," per developer's own read, which was close but not quite
     it). Added diagnostics incrementally: (a) grid-size logging — ruled out
     a degenerate 1x1 grid (was a sane 88x30 at 800x600 with a 9x20 cell);
     (b) PTY-reader byte-count + EOF/error logging in
     `crates/app/src/pane_session.rs` — showed the reader *was* receiving
     data (4 bytes, then 16), so the child process was alive and the pipe
     worked; (c) logged the literal escaped byte content — revealed the
     first chunk was exactly `\x1b[6n`, a Device Status Report / cursor-
     position query.

     Root cause: `alacritty_terminal`'s `Term` answers DSR queries by
     emitting `Event::PtyWrite(reply)` through its `EventListener` — the
     frontend is required to write that reply back to the PTY's input.
     `crates/pane/src/term.rs`'s listener (`NullListener`) discarded every
     event, including this one. cmd.exe's ConPTY/conhost startup handshake
     sends this query and *blocks* waiting for a reply that never came,
     which is why nothing else ever printed. bash doesn't send this query
     at startup, which is why the bug was invisible on Linux the whole time
     — this was never actually a Windows-only issue, just a real,
     previously-missing piece of the terminal implementation that Windows
     happened to be the first to exercise.

     Fixed: renamed `NullListener` → `EventProxy(Sender<Vec<u8>>)`, which
     forwards `Event::PtyWrite` payloads through a channel;
     `Screen::take_pty_writes()` drains it; `PaneSession::pump()`
     (`crates/app/src/pane_session.rs`) writes whatever comes back straight
     to the PTY. Added a unit test
     (`term::tests::cursor_position_query_produces_a_pty_reply`) asserting
     `\x1b[6n` after a cursor move produces the exact expected
     `\x1b[<row>;<col>R` reply. Verified no regression on Linux — full
     colored bash prompt (OSC title, color codes) still renders correctly,
     confirming the whole VT parsing path is in good shape generally.

     Diagnostic logging left in place (byte counts, escaped content, exit
     status, grid-size-at-spawn) since it's cheap and was decisive here —
     not gated behind a debug flag yet; worth reconsidering once a real
     config/logging story exists (Milestone 5).

  **Update — same session, continued:** DSR fix confirmed working — Windows
  now shows the cmd.exe banner/prompt and accepts typed input. But developer
  reported Backspace erases a whole *word* at a time, "like Ctrl+W."

  Added one more diagnostic (`PaneSession::write_input` logs outgoing bytes,
  `crates/app/src/pane_session.rs`) and got a full byte trace both
  directions. It showed the exact mechanism: we send exactly one `[8]`
  (`0x08`, BS) per Backspace press, and cmd.exe's line editor responds by
  erasing a whole word (VT sequence jumps the cursor back ~5-6 columns and
  clears that span) — a single `0x08` triggers word-erase in cmd.exe's
  editor, not char-erase.

  Root cause was in our own code, not cmd.exe: `key_bytes()`
  (`crates/app/src/main.rs`) checked `event.text` *before* falling back to
  the explicit `NamedKey` match arms. Winit populates `event.text` per
  platform from the OS's own text composition — on Windows that's driven by
  `WM_CHAR`, which fires with `0x08` for the Backspace key — so the `text`
  branch won and our own `NamedKey::Backspace => 0x7f` arm was dead code on
  Windows the entire time. Terminals conventionally send DEL (`0x7f`) for
  Backspace rather than BS (`0x08`) specifically to avoid this class of
  line-editor quirk (a well-known convention, not specific to cmd.exe).
  Fixed by reordering `key_bytes()`: named keys (Enter/Backspace/Tab/
  Escape/arrows) are now matched and returned *before* the `event.text`
  fallback, so our deliberate byte conventions always win over whatever the
  OS happens to compose for those same keys. `event.text` is now only
  consulted for keys with no explicit mapping (printable characters,
  IME-composed text, etc.), which is what it should be used for anyway.

  No regression on Linux (`cargo build`/`clippy`/`test` clean, bash prompt
  still renders correctly).

  **Update — same session, continued:** Backspace fix confirmed on Windows.
  Developer separately noted Ctrl+C/Ctrl+W don't work on either platform —
  correctly guessed this was just unimplemented, not a bug. Added Ctrl+letter
  handling to `key_bytes()` (`crates/app/src/main.rs`): tracks modifier state
  via a new `App.modifiers` field updated on `WindowEvent::ModifiersChanged`,
  and when Ctrl is held with a single-character logical key, encodes it to
  its control byte (Ctrl+A=1 .. Ctrl+Z=26) before the `event.text` fallback
  — holding Ctrl generally suppresses normal text composition anyway, so
  `event.text` would've been empty. Verified disjoint borrow of
  `self.graphics`/`self.modifiers` compiles fine (NLL splits struct fields
  automatically). No regression on Linux; not yet re-verified by developer.

  **Update — same session, continued:** Developer confirmed everything
  works (Ctrl+C/Ctrl+W included) and asked to gate the debug output behind
  a `--verbose`/`-v` flag, then move on. Added `crates/app/src/verbose.rs`:
  a simple `AtomicBool` (not threaded-through state, since the PTY reader
  runs on its own background thread and needs to check it too), set once in
  `main()` from `std::env::args()` — plain stdlib arg scanning, no CLI-parsing
  crate for one flag. Gated every diagnostic `eprintln!` added during this
  session's debugging (adapter info, grid-size-at-spawn, PTY read/write
  byte content, exit status) behind `verbose::is_verbose()`. Left the
  genuine error paths (failed window/GPU init, failed writes) unconditional,
  since those indicate real problems, not routine diagnostics. Verified:
  default run is quiet (only Mesa's own unrelated libEGL warnings survive),
  `--verbose` restores full diagnostics. Clean build/clippy/test.

  **Milestone 1 is done** — Cargo workspace scaffolding, single-pane PTY +
  alacritty_terminal grid + wgpu/cosmic-text rendering + keyboard passthrough,
  confirmed working end-to-end on both Linux (dev loop, CPU/llvmpipe
  rendering — WSL2 GPU accel deliberately not pursued further, see the
  auto-memory WSL2 project note) and native Windows (real GPU via AMD
  Radeon RX 6950 XT, Vulkan backend). macOS untested (no access). Real bugs
  found and fixed along the way, all still relevant for later milestones:
  pixel-snapped glyph positions + real font-metric cell sizing (was
  garbling text), `Event::PtyWrite` handling for DSR/cursor-position queries
  (was freezing cmd.exe), Backspace byte convention (DEL not BS, was
  word-erasing on cmd.exe), and Ctrl+letter control-byte encoding. Still
  uncommitted per developer's commit-timing rule — first commit should
  probably happen at a deliberate point the developer chooses, not
  automatically inferred by a session boundary.

  **Update — same session, continued:** Implemented all of Milestone 2
  (tasks 2.1-2.7) in one pass.

  `crates/layout`: `Node`/`Layout` per the design doc (`split`, `close` with
  sibling-promotion rebalance via `mem::replace` rather than cloning
  subtrees, `resize` clamped to [0.05, 0.95], `toggle_zoom`). One deliberate
  small extension beyond the original design sketch: `Node::Split` now
  carries a `SplitId` (the doc didn't have one) — needed so a divider drag
  can address a specific split node directly, since a divider belongs to a
  split, not to either of its panes. `crates/layout/src/geometry.rs`
  computes pane + divider rects from the tree (`Orientation::Horizontal` =
  left/right per tmux's convention — explicitly noted in the doc comment as
  *not* verified against Terminator's own terminology, which Milestone 3
  must check), plus `axis_extent` per divider (the parent area's pre-split
  length along the resize axis) so a drag's pixel delta converts to the
  correct ratio delta even for nested splits, not just the whole window.
  `focus_neighbor` picks the closest pane whose rect is adjacent in a given
  direction. 10 unit tests, including one that specifically locks in that a
  nested divider's `axis_extent` is the *local* parent span, not the window
  — caught my own arithmetic mistake in the first draft of that test before
  it shipped.

  `crates/render`: reworked the public API to be pane-agnostic ahead of
  needing to draw several panes' content in one pass. `GlyphCell` now
  carries absolute `x`/`y` pixel position instead of row/col grid indices;
  the cursor is no longer special-cased inside the renderer — it's just
  another `SolidRect` (same mechanism now used for dividers too), sampling
  the atlas's reserved solid-white texel. This let the cursor-specific code
  in `GridRenderer::render` disappear entirely in favor of one generic
  `rects` iterator alongside `glyphs`. Bumped the instance buffer capacity
  4096 → 65536 (Milestone 1's cap could have silently truncated a single
  large maximized pane's glyphs — a latent bug from that milestone, fixed
  in passing since this code was already being touched).

  `crates/app`: `Graphics` now owns a `layout::Layout` and
  `HashMap<PaneId, PaneSession>` instead of one pane, plus a `focused: PaneId`
  and drag state. `resize_panes_to_geometry()` is the single place that
  reconciles every visible pane's PTY+grid size with the layout's current
  geometry — called after window resize, split, close, and zoom toggle, so
  there's one code path for "panes changed shape" rather than four
  ad-hoc ones. Added `Pty::kill()` (`ChildKiller::kill`, available via
  `Child`'s supertrait bound without a separate import — verified by
  letting the compiler complain about the redundant import rather than
  guessing) plus `impl Drop for Pty`, so a pane's shell process is always
  terminated when its `PaneSession` is dropped, whether that's an explicit
  close or the whole app exiting — "closing a pane frees its resources"
  from the plan's acceptance criterion falls out automatically rather than
  needing to be remembered at every removal site.

  `crates/app/src/main.rs`: added placeholder Ctrl+Shift chords (E/O=split
  vertical/horizontal, W=close, Z=zoom, arrows=focus-move) purely to
  exercise the tree interactively — explicitly commented as temporary,
  since Milestone 3 owns researching Terminator's real defaults. Chords are
  checked before falling through to `key_bytes`'s raw passthrough (matching
  `design/input-router.md`'s "chord or passthrough, never both" rule) so
  they don't collide with the existing Ctrl+letter control-byte encoding
  from Milestone 1 (e.g. Ctrl+Shift+W is "close pane," plain Ctrl+W is still
  passed through as 0x17). Mouse handling added for divider drag:
  `CursorMoved`/`MouseInput` on the left button call
  `Graphics::begin_drag`/`drag_by`/`end_drag`, which hit-test against the
  current geometry's divider rects and convert pixel movement to a ratio
  delta via each divider's `axis_extent`.

  Verified: `cargo build`/`clippy --all-targets`/`test` clean across the
  whole workspace (10 new layout tests, all passing first try except one
  test-authoring arithmetic mistake caught before it shipped). Ran the app
  on Linux — starts cleanly, single root pane renders and works exactly as
  before. Could not verify the *interactive* parts myself (split, close,
  focus move, zoom, divider drag) — no keyboard/mouse automation available
  in this sandbox; this needs the developer's hands-on testing on both
  Linux and Windows.

  **Update — same session, continued:** Developer tested interactively and
  found three real issues:

  1. Ctrl+Shift+O (split-horizontal placeholder) was intercepted on Windows
     before reaching the app — E/W/Z all worked, so this pointed at some
     other program's global hotkey, not our code. Swapped the placeholder
     to Ctrl+Shift+H.
  2. Divider drag didn't work at all on Linux, and the cursor never showed
     a resize icon on either platform. Root cause for the first part: the
     hit-test region was exactly the 2px visual divider width — too thin to
     click reliably. Widened divider to 4px visual + 4px hit-test margin
     each side (12px effective grab zone), and implemented actual cursor-
     icon hover feedback (`EwResize`/`NsResize`/`Default` via
     `Window::set_cursor`), which hadn't existed before at all.
  3. After that, developer reported drag now works on Linux but the cursor
     icon still doesn't render as a real resize arrow there (just some
     generic shape), while Windows shows it correctly. Investigated
     directly rather than guessing: `XCURSOR_THEME`/`XCURSOR_PATH` are both
     unset in this WSL2/Ubuntu image, and there is no actual cursor bitmap
     theme installed anywhere on the filesystem (`/usr/share/icons/Adwaita`
     has no `cursors/` directory at all, just an empty `cursor.theme`
     stub) — confirmed via `find`. X11 has nothing to render our themed
     icon request with, so it falls back to old core-font cursor glyphs.
     This is an environment gap in this specific WSL image, not a bug in
     our code (which uses the standard, portable `CursorIcon` API) — not
     worth chasing further; recorded in the auto-memory WSL2 project note.

  Separately, developer flagged a real bug: closing a pane's shell from
  *inside* it (typing `exit`) didn't remove the pane at all — we had zero
  logic reacting to a shell exiting on its own, only to our explicit
  Ctrl+Shift+W close chord. Fixed: `Graphics::redraw()`
  (`crates/app/src/graphics.rs`) now checks `PaneSession::has_exited()` for
  every pane each frame (new method, wraps `Pty::has_exited`) and closes
  any that have exited through the same `close_pane` path the close chord
  uses (extracted `close_focused`'s body into a pane-id-parameterized
  `close_pane`, shared by both callers) — `redraw()` now returns `bool`
  (false = no panes left), and `main.rs` calls `event_loop.exit()` when it
  does. Verified for real, not just compiled: started the app, found its
  actual child shell PID via `ps`, sent it `SIGHUP` directly (same signal
  as typing `exit`), and confirmed via `ps -p` that the whole app process
  terminated on its own afterward — log showed the PTY hitting EOF, the
  exit status logging ("Terminated by Hangup"), then the process gone.

  Clean build/clippy/test throughout all of the above.

  **Update — same session, continued:** Developer reported the exit-close
  behavior was "almost perfect" but focus after a close went to the wrong
  pane — should go to the most recently *created* pane, not tree-traversal
  order. One-line fix in `Graphics::close_pane`
  (`crates/app/src/graphics.rs`): `PaneId`s are assigned by an
  ever-incrementing counter and never reused, so `.iter().max()` over the
  remaining panes *is* "most recently created" — changed from `.first()`.

  Then a long, worthwhile detour on the WSL cursor-theme question. Developer
  installed `xcursor-themes` per earlier advice; no change. Investigated
  properly rather than re-guessing: confirmed winit's X11 backend already
  does exactly the "try modern name, fall back to legacy names" behavior
  the developer proposed — it's built into the `cursor-icon` crate
  (`alt_names()`) and used unconditionally by `XConnection::get_cursor`
  (read the actual winit source to confirm, not assumed). The real problem
  was a naming mismatch: `whiteglass`/`redglass` use `sb_h_double_arrow`/
  `sb_v_double_arrow`, but winit's hardcoded alt-names for `EwResize`/
  `NsResize` are `h_double_arrow`/`size_hor` and `v_double_arrow`/
  `size_ver` (no `sb_` prefix — that prefix is only in the alt-names for
  the *different* `ColResize`/`RowResize` cursors). Verified
  `breeze-cursor-theme` *does* provide matching modern-name symlinks by
  downloading the `.deb` and inspecting it with `dpkg -c` before
  recommending it (not guessed) — developer installed it, but didn't like
  the visual style and asked for a middle path: "regular Linux would handle
  this fine, so if we're on WSL, fall back gracefully."

  Went further to verify whether "regular Linux" really would: confirmed
  via Ubuntu's official package-contents search
  (packages.ubuntu.com/search?searchon=contents) that **no Ubuntu package
  ships actual Adwaita cursor bitmap files at all** — re-downloaded
  `adwaita-icon-theme` itself fresh to double check, and it's genuinely
  just the `cursor.theme` stub, described upstream as "(small subset)".
  This is because modern GNOME/mutter don't need on-disk cursor files —
  they render cursors compositor-side via the Wayland `cursor-shape-v1`
  protocol, which winit's Wayland backend already implements
  (`wp_cursor_shape_manager_v1`, confirmed in source). That mechanism is
  Wayland-only, and we deliberately use X11 under WSL (the focus-loss
  workaround from earlier), so pixel-accurate Adwaita cursors are
  structurally unreachable here via legacy Xcursor files — not a packaging
  gap, an architectural one. Presented the options (accept a legacy theme,
  risk re-enabling Wayland, hand-extract mutter's bundled bitmaps, or drop
  it) via AskUserQuestion; developer chose to drop it entirely. Recorded
  the full chain of reasoning in the auto-memory WSL2 project note so this
  never needs re-deriving.

  Developer confirmed auto-close-on-exit works, noted a WSL-vs-Windows
  scaling difference they're not worried about, and asked to move on.

  **Update — same session, continued:** Implemented all of Milestone 3
  (input routing + grouping, tasks 3.1-3.4) in one pass.

  `crates/router` (previously an empty scaffold): `keymap.rs` defines
  `Chord` (key + ctrl/shift/alt/logo bools), `Key` (Char(char) plus
  Up/Down/Left/Right — the only keys v1's keymap actually binds), `Action`,
  and `Keymap` with `terminator_defaults()`. Before writing any bindings,
  fetched Terminator's actual `keybindings` dict from its own `config.py`
  on GitHub (via WebFetch, not memory) to get exact, current chords rather
  than guessing or trusting training-data recall — confirmed: split_horiz
  Ctrl+Shift+O, split_vert Ctrl+Shift+E, close_term Ctrl+Shift+W,
  close_window Ctrl+Shift+Q (mapped to our `Quit`), go_up/down/left/right
  Alt+Arrow (focus — notably *not* Ctrl+Shift+Arrow, which was Milestone
  2's placeholder guess), resize_up/down/left/right Ctrl+Shift+Arrow,
  toggle_zoom Ctrl+Shift+X, group_all Super+G, group_tab Super+T,
  ungroup_all Shift+Super+G. Two exceptions are our own choice, clearly
  documented as such: `ToggleGroup` (Ctrl+Shift+G) has no Terminator
  default at all — Terminator only assigns pane groups through its GUI,
  never a keybinding — and `SetBroadcastMode(Group)` reuses `group_tab`'s
  chord (Super+T) since v1 has no tabs for that binding to mean what it
  means in Terminator.

  `lib.rs`: `BroadcastMode` (Off/Group/All), `GroupId`, and `Router`
  resolving broadcast targets as a pure function of focused pane + mode +
  current groups (per the design doc's rationale — no maintained "active
  targets" list to go stale). v1 exposes only one flat group
  (`DEFAULT_GROUP`) via the keymap even though the data model
  (`HashMap<GroupId, HashSet<PaneId>>`) supports several, matching how
  Terminator's own keybindings only ever address one group at a time too.
  9 unit tests, all passing first try.

  `crates/layout` gained `resize_target(pane, direction)`: walks up from a
  pane to the nearest ancestor split whose orientation matches the
  direction's axis, returning which side the pane is on so the caller can
  decide the resize sign. Convention settled on: Right/Down always grow the
  focused pane along that axis, Left/Up always shrink it, regardless of
  which side of the split the pane is on — simpler and more predictable
  across nested splits than trying to replicate exactly what "the divider
  adjacent in this direction" means when there isn't one within the nearest
  matching split (a real ambiguity in nested-tree keyboard resize that
  wasn't worth over-engineering for v1). One new unit test.

  `crates/app`: `Graphics` now owns a `router::Router`. Added
  `dispatch_chord(chord) -> Option<bool>` — `None` means "not bound, treat
  as passthrough," `Some(false)` means quit — replacing Milestone 2's
  `placeholder_chord`/`Action` enum in `main.rs` entirely. `send_input` now
  resolves broadcast targets and fans out to every target pane's PTY
  instead of just the focused one. `redraw()` draws a `push_border` outline
  (4 thin `SolidRect`s — no new renderer capability needed, reused the
  existing primitive) around every pane in the current broadcast target set
  whenever mode isn't Off. `close_pane` now also calls
  `router.forget_pane()` so group membership doesn't leak stale entries
  after a pane closes. `main.rs`'s `winit_chord()` translates a winit key
  event into a `router::Chord` (only for the key shapes the keymap can
  actually bind — Char and arrows; everything else short-circuits to
  `None` immediately, since it's never going to be a chord).

  Clean build/clippy/test throughout (27 tests total across the workspace
  now). Verified no regression with a real run on Linux (bash prompt
  renders, no panic) — could not exercise the actual keybindings/grouping/
  broadcast interactively myself (no keyboard/mouse automation in this
  sandbox), same limitation as every prior milestone.

  **Update — same session, continued:** Developer pushed back hard on the
  Super+G/Super+T/Shift+Super+G bindings: "we can't use the Windows key,
  it's too tied into the OS," and asked to implement grouping/broadcast
  control as UI instead, "similar to how Terminator does this" — a fair
  point, since Terminator itself only exposes group assignment through its
  GUI, never a keybinding (something already noted when researching its
  real defaults). Flagged clearly that this means pulling part of
  Milestone 5's egui chrome work forward, ahead of the plan's sequence, and
  asked which way to handle that via AskUserQuestion; developer chose to
  pull it forward now rather than defer.

  Before writing any UI code, checked whether egui's wgpu integration was
  even compatible with our stack: `egui-wgpu` 0.35.0 (latest) pins
  `wgpu = "29.0"`, but we were on `wgpu 30.0.0` — a real, confirmed
  incompatibility (verified via the actual `Cargo.toml`, not assumed), not
  something cargo could unify; two different `wgpu` versions can't share
  `Device`/`Queue` types. Surfaced this as a genuine blocker with two real
  paths (downgrade our wgpu, or hand-roll a UI without egui-wgpu) via
  AskUserQuestion; developer chose to downgrade.

  Downgraded `wgpu` 30.0.0 → 29.0.4 in `render` and `app`. Turned out to be
  a very small, bounded revert: exactly two API differences between the
  versions (`VertexState.buffers` un-wraps back to plain
  `&[VertexBufferLayout]`, no `Option`; `present()` moves back onto
  `SurfaceTexture` itself instead of `Queue`) — found by just rebuilding
  and reading the two resulting compiler errors rather than pre-diffing
  the whole API surface, which was faster and just as reliable. Confirmed
  via `cargo tree -i wgpu` that the whole workspace unifies on a single
  `wgpu v29.0.4` once `egui-wgpu` was added — no duplicate-version split.

  Added `egui`/`egui-wgpu`/`egui-winit` 0.35.0 to `crates/app`. New
  `crates/app/src/ui.rs`: an `egui::Window` overlay (top-right anchored)
  with a three-way Off/Group/All broadcast-mode selector and a "focused
  pane in group" checkbox, using `egui::Context::begin_pass`/`end_pass`
  (not the newer `run`/`run_ui` — checked the actual 0.35 source; `run_ui`
  only hands the closure a bare full-screen `Ui` with no panel/background,
  not a `&Context`, so it can't host a floating `egui::Window` the way
  `begin_pass`/`end_pass` can). Every winit `WindowEvent` now goes through
  `egui-winit::State::on_window_event` first
  (`Graphics::ui_consume_event`); when consumed, keyboard/mouse events skip
  pane and divider handling (but `CursorMoved` still always updates
  `cursor_pos`, so a drag started right after leaving the overlay doesn't
  compute its first delta against a stale position — a real bug caught
  while writing the event-guard logic, not just a hypothetical). The egui
  pass renders in its own command encoder/submit, executed between the
  grid's own render and `frame.present()`, with `LoadOp::Load` so it
  composites over the grid rather than replacing it — no changes needed to
  `GridRenderer`'s API for this.

  Removed `ToggleGroup` and `SetBroadcastMode`'s default keybindings from
  `router::Keymap::terminator_defaults()` entirely (not just the Super-key
  ones — Ctrl+Shift+G too, since the UI is now the intended path for both).
  Kept the `Action` variants and `Router::toggle_group`/`broadcast_mode`
  themselves — they're still generically bindable, just with no default
  chord; a future config could remap something to them.

  Clean build/clippy/test throughout (still 27 tests, none needed changes).
  Verified no regression with a real run — process survives multiple
  render frames calling into the new egui render path without panicking.
  Could not visually confirm the overlay actually renders correctly or is
  clickable — no way to view the window or drive mouse/keyboard from this
  sandbox, same limitation as every prior milestone, but more consequential
  here since a rendering-pipeline compositing bug (e.g. wrong load op,
  wrong pass ordering) could easily *compile* fine while looking wrong or
  invisible on screen.

  **Update — same session, continued:** Developer confirmed the floating
  panel worked functionally, but pushed back on the design itself: "a
  floating panel is not the way to go." Fair — it's screen furniture
  that's in the way even when nobody's touching it, and doesn't actually
  match Terminator's own precedent (which the developer had cited
  earlier): Terminator's grouping UI is a right-click context menu, not a
  persistent panel.

  Rebuilt `crates/app/src/ui.rs` around that instead. `Ui` now holds
  `context_menu: Option<(PaneId, egui::Pos2)>` rather than always drawing
  a window; `show()` only renders anything (an `egui::Area` +
  `Frame::popup` styled menu, `Order::Foreground`) when a menu is open.
  Switched from `egui::Window` to `egui::Area`/`Frame::popup` since a menu
  needs precise positioning at the click point and popup-style chrome, not
  a draggable/resizable window frame.

  New `Graphics::pane_at(pos)` hit-tests which pane's rect contains a
  point (reusing the same geometry the divider hit-test already computes).
  Right-click (`MouseInput` on `MouseButton::Right`) opens the menu
  targeting whichever pane is under the cursor — deliberately *not*
  necessarily the focused pane, matching how a per-terminal context menu
  in Terminator addresses that terminal specifically, regardless of
  overall focus. Left-click while a menu is open dismisses it instead of
  performing its normal action (divider-drag-begin), a simple explicit
  rule in `main.rs` rather than relying on egui's `clicked_elsewhere()`
  frame-timing — this side-stepped a real question I couldn't fully
  resolve by reading source alone (whether the triggering right-click
  itself would spuriously register as an "elsewhere" click against a menu
  that didn't exist yet when that click occurred); the explicit rule is
  simple enough to reason about with certainty instead. `CursorMoved`
  still always updates `cursor_pos` regardless of menu state, for the same
  stale-position reason noted earlier.

  `UiRequest` now carries `toggle_group_for: Option<PaneId>` (the specific
  right-clicked pane) instead of assuming the focused pane. Removed four
  methods (`Graphics::pane_in_group`/`toggle_group`/`set_broadcast_mode`/
  `broadcast_mode`, `Ui::context_menu_open`) added during the first pass
  that turned out unused once the actual wiring was in place — caught via
  the compiler's own dead-code warnings rather than needing to track it
  manually.

  Clean build/clippy/test (still 27 tests). Verified no regression with a
  real run. Same caveat as last time: cannot visually confirm the menu
  actually appears at the click position, targets the right pane, or
  dismisses correctly — needs the developer's own eyes and mouse.

  **Update — same session, continued:** Developer reported right-click did
  nothing *and* divider resize (which previously worked) had also broken.
  First diagnostic round came back empty-handed: developer ran
  `cargo run -p app --verbose`, which doesn't do what it looks like —
  `--verbose` before `--` is consumed by cargo itself as its own build-
  verbosity flag and never reaches the binary's `std::env::args()` at all.
  Explained the correct invocation (`cargo run -p app -- --verbose`, or
  build once and run the binary directly) and asked for a retest.

  The real log came back very telling: `MouseInput` press events for both
  left and right buttons showed `ui_consumed=true`; release events showed
  `false`. Traced this to ground rather than pattern-matching against a
  guess: read `egui::Context::egui_wants_pointer_input`'s actual source —
  `egui_is_using_pointer() || (is_pointer_over_egui() && !any_down())` —
  and then `is_pointer_over_egui()`'s: it checks
  `Context::layer_id_at(pointer_pos)`, and if that layer is `Background`
  with no `root_ui_available_rect` set, falls into a branch literally
  commented `// We shouldn't get here, but who knows` that returns `true`
  unconditionally. `root_ui_available_rect` is *only* populated by the
  modern `run_ui` API's root-Ui bookkeeping — and `Ui::show` was built on
  `Context::begin_pass`/`end_pass` (chosen originally because `run_ui` only
  hands the closure a full-screen `Ui`, not a `&Context`, and a floating
  `egui::Window` needed the latter). Net effect: with `begin_pass`/
  `end_pass`, every single mouse press anywhere in the window was reported
  as "consumed by egui," permanently blocking both the right-click-to-open
  handler and the left-click-to-drag handler — a real, confirmed bug, not
  a hypothesis, and it explains the "divider resize broke" report too:
  same underlying cause, not two separate regressions.

  Fixed in `crates/app/src/ui.rs`: switched to `Context::run_ui`, using
  `ui.ctx().clone()` to get the `&Context` still needed for
  `egui::Area::show` (the context-menu-not-floating-window rewrite already
  meant `run_ui`'s root `&mut Ui` limitation was no longer a blocker,
  unlike when this was a floating `egui::Window`). Had to move
  `self.context_menu = None` out of the `run_ui` closure into a local
  `close_after: bool` flag applied after the call returns, since the
  closure can't hold a `&mut self.context_menu` while `self.ctx.run_ui`
  already holds `&self.ctx` for the same `self` — resolved by capturing
  `context_menu`'s value (a `Copy` type) into a local before the call
  instead of reaching into `self` from inside the closure.

  Clean build/clippy/test (27 tests, unaffected — this bug was entirely in
  UI-event plumbing, no coverage there yet). Verified no regression with a
  real run. This was the first bug in this whole project solved primarily
  by reading a dependency's own source for its exact documented behavior,
  rather than by empirical trial-and-error against our own code — worth
  remembering that the "grep the vendored source" technique that's carried
  this whole session works just as well on *why does the library behave
  this way* questions as it does on *what's the exact API signature*
  questions.

  **Update — same session, continued:** `run_ui` fix confirmed — menu
  opens, dismisses correctly, divider drag restored on both platforms.
  One more bug: on Windows, the menu appears exactly at the click point
  near the top-left of the window but drifts further away the closer the
  click is to the bottom-right; correct everywhere on Linux/WSL.

  That drift pattern (proportional to distance from the origin) is the
  signature of a physical-pixel-vs-logical-point unit mismatch, not a
  fresh guess — confirmed by reading `egui-winit`'s own source, which
  converts every incoming winit pointer position (physical pixels) by
  dividing by `pixels_per_point` before handing it to egui, because egui
  positions everything (including `Area::fixed_pos`) in logical points.
  `context_menu`'s stored position came straight from `cursor_pos`
  (physical pixels, the unit everything else in this app uses — layout
  rects, hit-testing, etc.) with no conversion. This only "worked" on
  Linux/WSL because that display reports a 1.0 scale factor, making
  physical and logical coincide there by luck, not because the units
  matched.

  Fixed in `Ui::show`: convert the stored position by
  `egui_winit::pixels_per_point(&self.ctx, window)` (the library's own
  helper, which also accounts for egui's optional zoom factor, not just
  `window.scale_factor()`) right before constructing the `Area`, keeping
  the physical-pixel convention everywhere else in the app unchanged —
  the conversion happens only at the boundary where a position crosses
  into egui. Clean build/clippy/test, no regression in a Linux run
  (division by a 1.0 scale factor is a no-op there, consistent with it
  already being correct on that platform).

  **Current state:** Milestone 3 plus the pulled-forward context-menu
  slice of Milestone 5. Three real bugs found through this feature's
  testing loop (egui consumed-flag via `begin_pass` vs `run_ui`, DPI unit
  mismatch, plus the earlier floating-panel design revision) — all
  root-caused by reading the actual dependency source rather than pattern-
  matching guesses, and this session's memory log has now been wrong about
  "should work now" more than once for this specific feature, so the
  Windows position fix specifically still needs the developer's own
  confirmation before treating it as settled. Still uncommitted (commit-
  timing rule). Next: get that confirmation, then move to Milestone 4
  (mouse — click-to-focus and divider drag already work from Milestones
  2/3; remaining piece is the SGR-mouse-reporting passthrough policy and
  Shift-override) per `.waypoint/plan/v1-build-plan.md`.

- **2026-07-19:** Developer confirmed the DPI context-menu fix and said
  "Nice. Proceed." — moved on to Milestone 4 (mouse), all four sub-tasks in
  plan order. As with every fast-moving-dependency question this session,
  verified `alacritty_terminal`'s actual mouse/selection API against its
  vendored source (`~/.cargo/registry/.../alacritty_terminal-0.26.0/src/`)
  before writing anything against it, rather than trusting recall: `TermMode`
  flags (`MOUSE_REPORT_CLICK`/`MOUSE_DRAG`/`MOUSE_MOTION`/`SGR_MOUSE`/
  `MOUSE_MODE`), `Term::mode()`, `Term.selection: Option<Selection>` (a public
  field), `Selection::{new,update,is_empty}`, `Selection::to_range` →
  `SelectionRange { start, end, is_block }` (the exact pattern
  `alacritty_terminal`'s own `RenderableContent` uses for rendering
  selection), `Term::selection_to_string()`, and `Side` (a plain alias for
  `Direction::{Left,Right}`, used for click-side precision we don't need at
  cell granularity — always used `Side::Left`).

  Important finding: the crate exposes selection state and mode flags, but
  *not* mouse-report byte encoding — that's frontend responsibility in real
  Alacritty too, so it was written from scratch against the standard xterm
  protocol (SGR mode 1006 and legacy normal-tracking, both — some programs
  only enable one or the other) in a new `crates/app/src/mouse.rs`, unit
  tested (5 tests) rather than only exercised interactively, since encoding
  bytes correctly is exactly the kind of thing that's easy to get subtly
  wrong (button-bit/modifier-bit offsets, the 32-byte legacy offset, SGR's
  press-vs-release final-byte distinction) without a human noticing from a
  visual test alone.

  Implementation, in plan order:
  - **4.1 Click-to-focus** — `Graphics::focus_at`, called from the existing
    left-press handler in `main.rs` as the fallback when the press didn't
    hit a divider (divider grabs already existed from Milestone 2/3; this
    was the one real gap).
  - **4.2 SGR/normal-tracking passthrough** — added `Screen::mode()` (pane
    crate) plus the new `mouse.rs` encoder; `Graphics` gained a
    `mouse_gesture: Option<(PaneId, mouse::Button)>` field distinct from the
    existing divider-drag state, so a press starts at most one of "grab a
    divider" / "forward to the PTY" / "start a local selection" — never
    more than one. Only left-button is forwarded; right-click stays
    reserved for the pane context menu unconditionally, matching the
    chrome-vs-program-input convention basically every terminal emulator
    uses (this was a deliberate scope decision, not an oversight — the
    plan's four sub-tasks don't ask for right-click passthrough, and Right/
    Middle stay in the `Button` enum with `#[allow(dead_code)]` since the
    SGR protocol needs a real button number regardless of which ones this
    app currently triggers).
  - **4.3 In-grid text selection** — `Screen` gained
    `start_selection`/`update_selection`/`clear_selection`/
    `selection_is_empty`/`selection_to_string`/`selection_range`, all thin
    wrappers over the real `alacritty_terminal` selection API (always
    `SelectionType::Simple`/`Side::Left` — no block/word/line selection in
    v1). Rendered as a highlight rect in `Graphics::redraw` using the same
    `SelectionRange` pattern `alacritty_terminal`'s own renderer uses,
    copied to the system clipboard on release via a new `arboard` dependency
    (trimmed to `default-features = false` — its default `image-data`
    feature pulls in the `image` crate and platform image-clipboard glue
    that this app has no use for, text-only copy is all that's needed). A
    release whose selection never actually moved (`Selection::is_empty`,
    which is `true` when start==end since both always use `Side::Left`) is
    discarded rather than left highlighting/copying a single accidental
    character. Starting a new selection clears any other pane's leftover
    highlight — only one pane's selection is ever live at a time. Chose
    clipboard-on-release (not X11 PRIMARY-on-select, PuTTY/Windows
    Terminal's convention) specifically because PRIMARY has no equivalent on
    Windows and this app is cross-platform-first; revisit only if a Linux
    user specifically asks for middle-click-paste-of-selection later.
  - **4.4 Shift override** — one-line change to the 4.2/4.3 press decision
    in `main.rs`: `!modifiers.shift && graphics.mouse_press(...)`, so a
    Shift-held click never even attempts reporting and falls straight
    through to local selection — the standard xterm escape hatch. No new
    state needed since the reporting-vs-selecting choice was already made
    once per press and reused for the rest of that gesture's motion/release.

  Clean build, no warnings, all 32 tests passing (5 new in `mouse.rs`) after
  each of the four sub-tasks. Still entirely interactively unverified by the
  developer and still uncommitted (commit-timing rule) — this is the first
  Milestone-4 feature set where the implementation, not just the design, is
  done; next step is the developer's hands-on test pass (verify against a
  real `vim`/`htop` session for 4.2/4.4, a plain shell prompt for 4.3, per
  the plan's own acceptance criteria), same as every milestone before it.

  **Update — same session:** developer confirmed "That all appears to
  work" — Milestone 4 done, no bugs reported (a first for this project;
  every prior milestone's testing pass turned up at least one real bug).
  Still uncommitted (commit-timing rule — nothing has been committed at
  all yet this whole project). Moving to Milestone 5 (chrome + config) per
  `.waypoint/plan/v1-build-plan.md`, starting with 5.1 (config load) — 5.4
  (settings panel) is partially blocked on the still-open theme/preset
  decision from CONOPS §8, per the plan's own note, so that task ships
  with a single built-in theme and no picker rather than waiting on it.

  **Update — same session:** before writing any config-path code, hit a
  real open question rather than a technical one: `.waypoint/design/
  config-system.md`'s per-platform paths are `.../<app>/config.toml`, and
  the product genuinely has no name yet (README still says "name TBD" —
  this was never decided, not something to infer from context). Asked the
  developer directly rather than inventing branding; they chose "pain"
  (this repo's own directory name) as a working name. Recorded as
  `config::APP_NAME` with a comment flagging it as provisional, same
  open-question status as the theme picker — revisit if the project is
  ever actually named.

  Implemented all four Milestone 5 sub-tasks in plan order:
  - **5.1 Config load** — new `crates/config`: `Config`/`General`/
    `Appearance`/`Cursor`/`CursorStyle` matching the design doc's schema
    exactly (field names, defaults, the `[keybindings]` sparse-override
    shape), `serde` + `toml` for (de)serialization, hand-rolled per-platform
    config-dir resolution (XDG_CONFIG_HOME/AppData/Library — no `dirs`
    crate, the logic is short enough that a dependency wasn't worth it).
    `Config::load` degrades to defaults on anything wrong (missing file,
    unreadable, unparseable), matching the design doc's explicit "missing
    file → defaults, not an error" rule extended to cover "broken file"
    too, on the reasoning that a first load has no previous config to
    preserve anyway — that distinction turned out to matter for 5.2, see
    below. Wired into `crates/app` immediately rather than leaving the new
    crate unused until later sub-tasks: `default_shell` now threads through
    to `PaneSession::spawn` (previously hardcoded to `None`/platform
    default) and `appearance.font_size` replaced the `FONT_SIZE_PX` magic
    constant (16.0 → the design doc's own documented default, 13.0) — doing
    this now, rather than waiting for the settings panel to be the first
    thing that touches these fields, meant there was something the
    developer could actually observe (a custom `config.toml` changing
    startup font size) rather than an inert struct with only unit-test
    coverage.
  - **5.2 Hot reload** — split `Config::load` into `try_load` (`Result`,
    distinguishing missing-file `Ok(default)` from broken-file `Err`) with
    `load` as a thin defaults-on-`Err` wrapper; this split exists
    specifically because hot reload needs to tell those two apart in a way
    the initial load doesn't (a bad edit while running must keep the *last
    good* config, not reset to defaults — resetting live font size/shell to
    defaults because of a typo would be a much worse experience than just
    ignoring the typo'd save). `crates/app/src/graphics.rs` spins up a
    `notify` watcher on the config file's *directory* (not the file itself
    — the design doc's own reasoning: an editor's temp-file-plus-rename
    save pattern can orphan a watch on the original file). Watcher-setup
    failure (can't create the dir, platform watcher unavailable) degrades
    to "no hot reload," logged, never a startup failure — consistent with
    "config problems never crash the app" as a blanket rule, not just for
    parse errors. Polled once per frame from `redraw()`, same place
    pane-exit is already polled. Only `font_size` currently triggers a
    live-visible change (re-measures cell size, resizes every pane) since
    it's the only field with a rendering effect wired up yet.
  - **5.3 Keybinding overrides** — `router::Keymap::apply_overrides(&BTreeMap
    <String, String>)`, added to the `router` crate (not `config`, since
    parsing chord/action strings is inherently about `router`'s own
    `Chord`/`Action`/`Key` types — `config` stays a generic string-keyed
    schema with no knowledge of what the strings mean, keeping the
    dependency direction one-way). Chord parsing accepts modifiers in any
    order, case-insensitively, including `logo`/`super`/`cmd`/`win` as
    aliases for the one modifier this project's *default* keymap
    deliberately never uses (Windows-key-too-OS-reserved, from earlier this
    session) — that restriction was specifically about not shipping a
    default that traps a user into an OS conflict they didn't choose, not a
    blanket ban on the key ever being bindable by a user who wants it
    themselves. `"none"` as the action name unbinds without a replacement,
    exactly the design doc's wording. An unparseable chord or unrecognized
    action name is reported to stderr and skipped, not fatal, so one typo
    in a hand-edited `[keybindings]` table doesn't take out every other
    override — six new tests cover this (rebind, none-unbind, binding a
    previously-unbound action, a malformed entry not blocking a well-formed
    one after it, an unknown action name leaving the original binding
    alone, modifier order/case insensitivity). Wired into `graphics.rs` at
    both the initial load and every hot reload — always rebuilt from a
    fresh `terminator_defaults()` first, then overrides re-applied, so a
    since-removed override correctly reverts its chord to the built-in
    default on the next reload instead of getting stuck at a stale
    rebinding from an earlier version of the file.
  - **5.4 Settings panel** — extended the existing right-click context menu
    (`crates/app/src/ui.rs`) with a "Settings..." entry opening an
    `egui::Window`. This is the first `egui::Window` (persistent while
    open) anywhere in this app's UI, which looks like it could repeat the
    "always-visible floating panel" mistake from the broadcast/group UI
    earlier this session — the distinguishing fact that makes it not the
    same mistake: Terminator's own Preferences dialog is *also* exactly
    this shape (a dialog you explicitly open and close), reached through
    that same right-click menu since neither app has a menu bar to hang it
    off instead. The objection earlier was specifically to *always-visible*
    chrome, not to `egui::Window` as a container — this one only exists
    between clicking "Settings..." and clicking Save/Cancel/closing it.
    Fields: font size, transparency, scrollback lines, default shell,
    cursor style (all editable), plus a read-only list of the current
    `[keybindings]` overrides — deliberately read-only, since remapping
    chords from inside the panel isn't something 5.4's acceptance criteria
    ask for and hand-editing `config.toml` already covers it fully (5.3).
    No theme picker, per the plan's own note — still blocked on CONOPS §8.
    "Save" builds a full `Config` (draft edits over a clone of the live one,
    so fields the panel doesn't expose — theme, keybinding overrides —
    survive untouched) and calls the new `Config::save`, which just writes
    the file; it does *not* poke the new values into live state directly.
    The already-running 5.2 watcher picks up that write the same way it
    would a hand edit — exactly the "one apply path regardless of source"
    rule the design doc calls for, and it fell out for free from having
    built 5.2 first.

  Clean build, clean clippy (one real finding — `Config`'s hand-written
  `Default` impl was flagged as derivable since every field already had its
  own correct `Default`; simplified to `#[derive(Default)]`), all 43 tests
  passing workspace-wide (up from 32 after Milestone 4): 5 new in `config`
  (missing/present/malformed load, sparse keybinding-override parsing,
  save-then-load round-trip) plus 6 new in `router` (override rebind,
  none-unbind, binding a previously-unbound action, a malformed override
  not blocking a well-formed one after it, an unknown action name leaving
  the original binding alone, modifier order/case insensitivity). No new
  tests for the `notify` watcher or the settings-panel UI itself — both
  depend on real filesystem timing or a running egui context, and
  `config`/`router`'s own thorough coverage already exercises the actual
  parsing/apply logic they call into. Entirely interactively unverified by
  the developer and still uncommitted (commit-timing rule) — next step is
  their
  hands-on pass: a custom `config.toml` at the resolved path actually
  changing startup behavior, editing `font_size` live while running,
  remapping/unbinding a chord and confirming it takes effect on save, and
  the Settings panel itself (open/edit/Save/Cancel, confirm Save round-
  trips through the file rather than an invisible direct-apply path).

  **Update — same session:** developer confirmed Milestone 5 — config
  files are written on first save, and editing the file live-updates
  running terminals as expected. They also flagged that cursor-style and
  transparency changes don't do anything visible yet, and asked whether
  that was expected — it was: `.waypoint/project.md`'s Milestone 5 bullet
  already documented both as inert-but-round-tripping (transparency
  explicitly deferred to Milestone 6; cursor-style rendering was never in
  any milestone's acceptance criteria at all). Said "Proceed," moving on to
  Milestone 6 (transparency) per the plan.

  Implemented both of Milestone 6's sub-tasks together, since 6.2 turned
  out to need no code of its own once 6.1 was done right — verified the
  exact API shapes against vendored source before writing anything, same
  discipline as every prior milestone:
  - Confirmed `winit::window::WindowAttributes::with_transparent(bool)`
    exists and is a creation-time attribute (can't be toggled after the
    window is made) — set unconditionally in `main.rs`, not conditionally
    on the config's initial transparency value, since the *level* has to
    stay hot-reloadable even though the window's transparency-capability
    flag can't be.
  - Confirmed `wgpu::CompositeAlphaMode`'s four variants and what each
    expects from the stored swapchain colors (`Opaque`: ignore alpha;
    `PreMultiplied`: expects colors already RGB×alpha; `PostMultiplied`:
    expects straight/non-premultiplied colors, compositor does the
    multiply; `Auto`: picks `Opaque` or `Inherit`, i.e. never reliably
    picks a mode that actually honors alpha) and that
    `Surface::get_capabilities(&adapter).alpha_modes` is how to check what
    a given adapter/surface combination actually offers (guaranteed at
    least `Opaque` or `Inherit`).
  - The key finding that avoided a much bigger change: checked
    `crates/render`'s existing pipeline (`wgpu::BlendState::ALPHA_BLENDING`,
    not `PREMULTIPLIED_ALPHA_BLENDING`) and worked out that its output is
    already straight/non-premultiplied — standard "over" color blending
    plus an `OVER`-style alpha component naturally produces a real,
    non-premultiplied final alpha per pixel (a fully-opaque glyph drawn
    over a partially-transparent background composites back to fully
    opaque, automatically, with no special-casing needed for "text should
    stay legible"). That's exactly what `PostMultiplied` expects, so the
    only change needed was requesting `PostMultiplied` explicitly (`Auto`
    doesn't reliably pick it) when `get_capabilities` reports it's
    available, falling back to whatever `Auto` already picked (typically
    `Opaque`, logged once under `--verbose`) otherwise — zero pipeline
    changes. Explicitly decided *against* also supporting `PreMultiplied`
    as a fallback: doing that correctly would mean premultiplying every
    color value the renderer emits (background clear, cursor, selection,
    text) and switching the pipeline to `PREMULTIPLIED_ALPHA_BLENDING`
    throughout — a much bigger, more invasive change for a de-risking
    milestone whose acceptance criterion is just "shows desktop content
    behind it at a non-1.0 alpha" on *some* working configuration, not
    "works on every possible backend."
  - Background clear alpha now reads `settings.appearance.transparency`
    (clamped 0.0–1.0 — a hand-edited value outside that range would
    otherwise just get handed straight to wgpu) fresh every frame in
    `redraw`; text/cursor/dividers/selection/broadcast-border colors are
    untouched, so only empty cells become see-through, matching how every
    other terminal emulator's "background transparency" setting behaves.
  - 6.2's "hot-reloadable, config-driven level" fell out for free: since
    `redraw` reads `settings.appearance.transparency` live every frame and
    Milestone 5.2's watcher already replaces `settings` wholesale on a
    valid reload, there was no cached value anywhere that changing the
    config needed to invalidate — unlike `font_size`, which does need
    `apply_settings` to explicitly recompute cell size and resize panes.

  No new unit tests — this milestone is GPU/windowing wiring (window
  attributes, surface alpha-mode selection, a clear-color alpha value) with
  nothing pure-logic-shaped left to unit test beyond a single `f32::clamp`
  call. Clean build, clean clippy, all 43 tests still passing (unchanged
  from Milestone 5 — nothing here added new testable logic). Entirely
  interactively unverified by the developer and still uncommitted (commit-
  timing rule). Real transparency compositing depends on the platform
  actually supporting it — expect it to show cleanly on native Windows
  (DWM) and a compositor-equipped Linux desktop; WSL2/WSLg's own
  compositing story is a known quirk distinct from a real target platform
  (see `project_wsl2_dev_environment` in auto-memory) and isn't something
  to chase if it looks wrong specifically there, consistent with how the
  WSL2 cursor-theme issue was handled earlier in this project (verify on
  the real target platforms, don't over-engineer around a dev-environment-
  only limitation).

  **Update — same session:** developer tested Milestone 6 and reported two
  real problems: on Windows, changing transparency does nothing at all
  (standard opaque background regardless of the setting); on WSL, the
  window is completely see-through — not just empty cells, everything,
  including presumably text — *and* mouse clicks pass through it to
  whatever's behind. Investigated both before touching any code, since
  guessing wrong here would mean asking for another full round-trip test
  on hardware I can't access myself.

  **Windows root cause**, found by reading `wgpu-hal` 29.0.4's actual
  backend source (not guessed, not assumed from the public `wgpu` docs
  alone):
  - `wgpu-hal`'s DX12 `adapter.rs` (`surface_capabilities`, ~line 1296):
    for `SurfaceTarget::WndHandle` (exactly what a plain window handle —
    what `winit`'s normal integration gives wgpu — produces),
    `composite_alpha_modes` is hardcoded to `vec![CompositeAlphaMode::
    Opaque]` — nothing else is ever reported available. `PreMultiplied`/
    `PostMultiplied` only appear for `SurfaceTarget::Visual`/
    `VisualFromWndHandle`/`SurfaceHandle`/`SwapChainPanel` — all
    DirectComposition-based targets, a fundamentally different swapchain
    setup (`CreateSwapChainForComposition`, not `CreateSwapChainForHwnd`).
  - Checked whether switching to the Vulkan backend instead would sidestep
    this: `wgpu-hal`'s Vulkan backend just reads `VkSurfaceCapabilitiesKHR.
    supportedCompositeAlpha` straight from the driver/WSI for a plain
    `VkWin32SurfaceKHR`, which in practice also only ever reports `OPAQUE`
    on Windows (real translucency there is a DWM/DirectComposition concept,
    not something Win32 Vulkan WSI exposes) — so this isn't a DX12-
    specific gap to route around by picking a different backend, it's a
    "plain window handle" gap on Windows regardless of backend.
  - `winit`'s own Windows `on_create` (found while first investigating,
    before finding the wgpu-hal detail above) calls
    `DwmEnableBlurBehindWindow` with a full-window blur region when
    `attributes.transparent` is set — the older Aero-Glass-era API. This
    also lines up with "does literally nothing": that mechanism is well
    known not to reliably work with modern flip-model swapchains (what
    wgpu uses), which mostly stopped honoring DWM blur-behind for
    transparency after the Windows 8 swap-chain model changed.
  - The fix that actually works: `wgpu-hal`'s DX12 backend already has a
    *fully self-contained* automatic DirectComposition path —
    `Dx12SwapchainKind::DxgiFromVisual` (set via `Dx12BackendOptions.
    presentation_system`, both public in the `wgpu` crate, not just
    wgpu-hal). When set, `create_surface` for a plain window handle
    routes to `SurfaceTarget::VisualFromWndHandle`, whose `DCompState::
    get_or_init` (in `dx12/dcomp.rs`) lazily creates its own
    `IDCompositionDevice` (via `DCompositionCreateDevice2`), a
    `IDCompositionTarget` for the hwnd, and an `IDCompositionVisual`,
    wires `target.SetRoot(&visual)` once, then on every swapchain
    (re)creation calls `visual.SetContent(&swap_chain)` +
    `device.Commit()` — entirely inside wgpu-hal. Nothing in this app
    touches DirectComposition directly; the doc comment on
    `Dx12SwapchainKind::DxgiFromVisual` states outright "This supports
    transparent windows" (trade-off: "does not have support from
    RenderDoc," irrelevant here). Implemented as: pin `wgpu::Backends` to
    `DX12` on Windows only (`platform_backends()`, `#[cfg(target_os =
    "windows")]`, `Backends::default()` — i.e. try-everything — elsewhere)
    so backend selection can't land on Vulkan and silently skip this path,
    plus set `Dx12BackendOptions.presentation_system =
    DxgiFromVisual` unconditionally (harmless on non-Windows targets,
    since the field is simply unused there). Presented the Windows options
    (defer it, a simpler Windows-only whole-window-fade via
    `SetLayeredWindowAttributes`, or invest in DirectComposition properly)
    to the developer as a real product decision rather than picking
    unilaterally, since it changes actual visual behavior and engineering
    investment — they chose the DirectComposition route, matching Linux/
    macOS's per-pixel behavior rather than a lesser whole-window fade.
  - Verified by adding the `x86_64-pc-windows-gnu` target (`rustup target
    add`) and running `cargo check`/`cargo clippy --target x86_64-pc-
    windows-gnu -p app` — both clean. This confirms the Windows-only code
    path (and every `windows`-crate/DirectComposition type it touches
    transitively through `wgpu`) actually compiles and type-checks
    correctly; it is *not* a substitute for running it on real Windows,
    since there's no way to open a GUI session or exercise DWM composition
    from this dev environment. Framed clearly as "compiles correctly," not
    "confirmed working," when reporting back — those are different claims
    and shouldn't be blurred.

  **WSL**: did not attempt a code fix — investigated whether standard X11
  behavior could explain click-through on its own (it can't: X11 alpha
  visuals don't affect input hit-testing unless an app explicitly narrows
  its XShape *input* region, which this app never does), which points at
  WSLg's own compositor/bridging layer rather than anything wrong in how
  the app requests a transparent X11 visual. This is the same category of
  issue as the WSL2 cursor-theme investigation earlier this session (real
  target platforms behave differently from WSLg's virtual-display
  integration) — per that precedent, not chasing it with speculative fixes
  until there's confirmation it's actually WSL-specific. Next step needs
  the developer: re-test transparency on real Windows (should now show a
  real per-pixel blend with the desktop), and if possible on a real
  (non-WSL) Linux desktop with a compositor, to determine whether the
  "completely see-through + click-through" reproduces there too (real bug,
  needs more work) or is WSL-only (expected environment limitation, same
  call as cursor themes — document and move on).

  Clean build/clippy/tests on Linux (43 tests, unchanged) and clean cross-
  compiled build/clippy for Windows. Still uncommitted (commit-timing
  rule) — this is a mid-milestone bugfix on top of already-unverified work,
  not yet re-confirmed by the developer.

  **Update — same session:** developer actually ran the DirectComposition
  build on real Windows — first real hardware test of this fix. It
  crashed immediately:

  ```
  thread 'main' panicked ... wgpu error: Validation Error
  Caused by: In Surface::configure — Invalid surface
  ```

  No underlying detail — traced this to the app never having installed a
  `log`-crate backend, so every `log::error!`/`log::warn!` call from
  `wgpu-hal`/`wgpu-core` (which is how they report *why* a backend call
  failed, not via the error type itself) was going nowhere. Added
  `env_logger::Builder::from_env(...).default_filter_or("warn")` at the
  very top of `main()` — defaults to showing warnings/errors even without
  `RUST_LOG` set, so this class of "real error, no detail" problem
  shouldn't recur. This alone was worth doing independent of the
  transparency bug — first time this project had any visibility into
  wgpu's internal diagnostic logging at all.

  Asked the developer to re-run with logging in place. Real error:

  ```
  ERROR wgpu_hal::dx12: SwapChain creation error: The application made a
  call that is invalid. ... (0x887A0001)
  ```

  `0x887A0001` = `DXGI_ERROR_INVALID_CALL`, from
  `IDXGIFactory2::CreateSwapChainForComposition`. Traced the exact cause by
  reading `wgpu-hal`'s `dx12/mod.rs` `Surface::configure` once more,
  specifically the `flags` computation right before swapchain creation:
  `DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING` is set unconditionally whenever
  `self.supports_allow_tearing` is true (detected once, purely from
  adapter/factory capability, independent of which swapchain-creation path
  gets used later in the same function) — but per Microsoft's own DXGI
  docs, that flag is only valid for `CreateSwapChainForHwnd`; every
  composition-based creation call (`CreateSwapChainForComposition`,
  `CreateSwapChainForCompositionSurfaceHandle`) rejects it outright.
  Checked wgpu-hal 30.0.0 (also vendored from an earlier, unrelated
  download) — identical code, so this isn't something a version bump would
  have fixed, and it means `Dx12SwapchainKind::DxgiFromVisual` — the whole
  feature this session reached for — is broken out of the box on any
  adapter that reports tearing support, which is effectively every modern
  GPU, not something specific to the developer's RX 6950 XT.

  Also checked whether this could be sidestepped without touching
  wgpu-hal: `supports_allow_tearing` is computed once at instance-creation
  time from a raw `IDXGIFactory5::CheckFeatureSupport` call, with no
  `Dx12BackendOptions`/`InstanceFlags` knob exposed anywhere to skip or
  override that detection — confirmed there's no configuration-only way
  around this from application code.

  Proposed three ways forward (patch wgpu-hal, whole-window fade via
  `SetLayeredWindowAttributes`, or defer Windows entirely) using
  `AskUserQuestion`. Developer's response reset several things at once,
  each worth remembering on its own:
  - **Stop using pop-up multiple-choice questions for this kind of
    decision — discuss in chat instead.** Recorded as feedback (see
    `[[feedback-discussion-not-popups]]`, to be written).
  - **Whole-window fade is off the table outright** — "we're definitely
    _NOT_ going to fade an entire window. That would be useless." The
    developer wants real background-only transparency with text staying
    legible, the same standard essentially every modern terminal aims for
    — not a lesser substitute.
  - Pushed back hard, and correctly, on "no working transparency
    implementation exists" — pointed at Windows Terminal as an existence
    proof and asked whether this meant the wrong libraries were chosen.
    Answered honestly rather than defensively: wgpu *has*
    DirectComposition support already built in — this is one narrow,
    specific upstream bug in an existing feature, not a fundamental gap;
    flagged real uncertainty about Windows Terminal's actual mechanism
    (it's XAML-Composition-based, not a raw DXGI swapchain, likely closer
    to a system acrylic backdrop than clean background-only alpha —
    stated as understanding-not-fully-verified, not asserted as fact) and
    noted Alacritty (OpenGL, not wgpu) has its own long-running Windows
    transparency rough edges historically, so switching graphics stacks
    isn't an obvious shortcut around Windows-compositing pain generally.
  - **Explicitly declined submitting anything upstream right now** — not
    because patching is wrong, but because neither of us has fully
    verified the fix on real hardware yet, and submitting AI-written code
    as a PR to a repo whose language/codebase the developer hasn't
    personally reviewed, to fix a bug that isn't yet confirmed fixed, is
    the wrong order of operations. Local-only fork first; upstreaming (if
    ever) is a distinct, later, deliberate decision — not a default
    side-effect of getting our own build working. This is an important
    standing principle for how this project handles any future upstream
    dependency bugs, not just this one — see `[[feedback-fork-dont-
    upstream-yet]]` (to be written).

  Implemented the local-fork approach: copied the exact vendored
  `wgpu-hal-29.0.4` source (already fully self-contained — crates.io
  packages rewrite sibling-crate path deps to plain version deps at
  publish time, confirmed by checking its `Cargo.toml`, so no dependency
  surgery was needed) into `vendor/wgpu-hal-29.0.4/` in this repo, applied
  the one targeted fix (only set `ALLOW_TEARING` when `self.target` is
  `SurfaceTarget::WndHandle`, gated by a `matches!` check, with an inline
  comment explaining the DXGI restriction and pointing at `vendor/
  README.md`), and wired it in via `[patch.crates-io] wgpu-hal = { path =
  "vendor/wgpu-hal-29.0.4" }` in the workspace `Cargo.toml`. Wrote `vendor/
  README.md` documenting the full problem/cause/fix/status for the
  developer's own future review — explicitly notes the fix is *not*
  submitted upstream and that doing so is a deliberate later step. Clean
  build on Linux, clean cross-compiled `cargo check`/`clippy --target
  x86_64-pc-windows-gnu`, all 43 tests still passing. Still can't verify
  this actually renders correctly without the developer's own hands-on
  Windows test — that's the next step, and hopefully closes out the
  Windows side of Milestone 6 for real this time.

  **Update — same session:** it wasn't. Same crash, same
  `DXGI_ERROR_INVALID_CALL`, confirming the tearing-flag patch itself
  *did* apply correctly (double-checked: `Cargo.lock`'s `wgpu-hal` entry
  has no `source =` line, the signature of a patched path dependency, and
  `cargo tree -i wgpu-hal` showed exactly one resolved copy, from
  `vendor/`) — so the first fix was real but incomplete, not ineffective.

  Went back through `DXGI_SWAP_CHAIN_DESC1`'s documented composition-
  swapchain constraints from memory and came up short of a second
  candidate — rather than ship another guess and cost another full
  Windows test cycle, asked the developer to get the actual D3D12
  validation message via Sysinternals DebugView (their call, correctly:
  they pushed for the ground-truth error over more speculation). That
  needed the Windows "Graphics Tools" optional feature installed first
  (provides the D3D12 debug layer) — worth two small notes on their own:
  the feature's own error text says "Windows 10" even though they're on
  Windows 11 (pure stale branding in Microsoft's own strings, not a real
  incompatibility — the underlying capability name, `Tools.Graphics.
  DirectX~~~~0.0.1.0`, dates to the Windows 10 era and was never renamed);
  and once this came together as a real "how do we tell future
  developers about this" question, documented it in the README's new
  Development section — explicitly framed as optional/debug-build-only,
  since release builds never request the debug layer and end users of a
  shipped binary would never encounter this at all.

  The real message, once captured:

  ```
  DXGI ERROR: IDXGIFactory::CreateSwapChainForComposition: Composition
  SwapChains do not support the DXGI_ALPHA_MODE_STRAIGHT AlphaMode.
  ```

  This was *not* a `wgpu-hal` bug — it was our own mistake, made back in
  Milestone 6.1: `graphics.rs` requested `CompositeAlphaMode::PostMultiplied`
  (DXGI `STRAIGHT`) because that's what `render`'s pipeline happened to
  produce at the time (`wgpu::BlendState::ALPHA_BLENDING`, non-premultiplied
  "over" blending) — reasoning that held up fine for the plain-WndHandle
  path this session tested first, but composition swapchains only accept
  `PREMULTIPLIED`, full stop, per this exact runtime validation message.
  Picking the alpha mode to match what the renderer already produced was
  backwards, once composition swapchains were in the picture — the
  renderer needed to produce what the required alpha mode expects, not
  the other way around.

  Fixed properly, not routed around: this needed real premultiplied output,
  not just a different enum value.
  - `crates/render/src/lib.rs`: pipeline blend state changed from
    `ALPHA_BLENDING` to `PREMULTIPLIED_ALPHA_BLENDING`.
  - `crates/render/src/shader.wgsl`: `fs_main` now multiplies RGB by the
    *effective* alpha (`instance.color.a * glyph_coverage`, not just the
    instance's own alpha) before returning it — doing this in the shader
    rather than premultiplying `Instance.color` on the CPU side is what
    correctly handles anti-aliased glyph edges (partial per-pixel
    coverage), not just solid rects (dividers, cursor, selection, which
    always sample full coverage from the atlas's reserved opaque texel and
    would have been fine either way).
  - `crates/render/src/lib.rs`'s `render()`: the background clear color
    also needed manual premultiplying before use — `LoadOp::Clear` writes
    directly into the render target and never passes through the
    fragment shader, so it's the one place nothing else premultiplies for
    you.
  - `crates/app/src/graphics.rs`: now requests `CompositeAlphaMode::
    PreMultiplied` instead of `PostMultiplied`, with the fallback chain
    reasoned through explicitly — no longer falls back to `PostMultiplied`
    if `PreMultiplied` isn't offered, since the renderer's output no
    longer matches that convention on *any* platform; falls back to
    whatever `Auto` picks (typically `Opaque`) instead, same "logged, not
    treated as fatal" pattern as before.
  - `vendor/README.md` updated with this second issue's own writeup,
    clearly separated from the first (tearing-flag) one, including that
    this one lives in *our* code, not the vendored `wgpu-hal` copy — so a
    future reader doesn't wonder whether the first patch actually worked.

  Clean build/clippy on Linux, clean cross-compiled `cargo check`/`clippy`
  for Windows, all 43 tests still passing (no test coverage possible for
  either the shader or the alpha-mode selection itself — GPU pipeline
  state and a runtime capability query, same category as the rest of
  Milestone 6). Still needs the developer's real Windows re-test — but
  this time the fix is grounded in the actual D3D12 validation message,
  not documentation recall, which is a meaningfully different confidence
  level than the previous two attempts.

  **Update — same session:** that fixed the crash — window rendered
  correctly at launch — but the developer's next report was a screenshot,
  not a stack trace: resizing the window larger left the original rect
  showing frozen content while the newly exposed area showed raw desktop
  passthrough. This was a real inflection point in *how* to work the
  problem, not just what the problem was.

  Their message was pointed and fair: three "should be fixed now" claims
  in a row that each turned out incomplete, on a bug I can't reproduce or
  see myself, was legitimately frustrating, and they said so directly
  while also explicitly asking to communicate better rather than just
  venting. Response: stopped, acknowledged it plainly (no defensiveness,
  no immediately jumping to another fix), explained exactly what the
  screenshot showed in plain terms, was explicit about which parts were
  confident-from-evidence vs. still-a-guess, and asked ONE specific
  diagnostic question (does the frozen region track new content, or stay
  static?) before touching code again — deliberately choosing to spend a
  message on narrowing the hypothesis space instead of on another
  speculative patch. This is the same underlying instinct as the earlier
  "get the real DebugView message instead of guessing" moment, applied to
  process, not just to a single bug: when the cost of being wrong again is
  high (another full Windows test cycle, more accumulated frustration),
  slow down and confirm before acting, don't pattern-match to "keep
  fixing."

  Developer's answer ("dir on a big directory doesn't change anything") —
  the frozen/transparent split doesn't move regardless of new content —
  pointed straight at a *composition* problem, not a rendering problem:
  new frames were presumably still being drawn and presented correctly,
  just not visually recomposited by DWM. Went back to `wgpu-hal`'s
  `Surface::configure` resize branch (`Some(sc) => { ... }`, taken
  whenever a swapchain already exists) and found upstream's own comment
  sitting right above it: `//Note: this path doesn't properly re-
  initialize all of the things` — wgpu-hal's author already knew this
  branch was incomplete. For `VisualFromWndHandle` specifically: it only
  calls `ResizeBuffers` (correctly resizes the DXGI-level buffers) but
  never calls `IDCompositionVisual::SetContent`/`IDCompositionDevice::
  Commit` again — `Commit` is what actually pushes a pending composition-
  tree change out to the desktop compositor; without it, DWM keeps
  displaying whatever was last committed (the original small size),
  while area outside that was never composited into at all — exactly
  the frozen-rect-plus-raw-passthrough split in the screenshot, and
  exactly why new content didn't change the boundary (the *pixels*
  presumably were rendering fine; the compositor just never learned the
  visual's content changed size).

  Patched `vendor/wgpu-hal-29.0.4/src/dx12/mod.rs` again (second patch in
  the same file, both inside `configure`): in the resize branch, after a
  successful `ResizeBuffers`, re-acquire the already-initialized
  `DCompState` via `get_or_init` (safe to call again — it only creates a
  new device/target/visual if one doesn't already exist, so this just
  re-fetches the existing one) and call `SetContent`+`Commit` again, same
  as the initial-creation path already does. Updated `vendor/README.md`
  with this as a third, clearly-separated issue, and consolidated its
  structure (had drifted into two confusing "Status" sections while
  writing up issues two and three back to back — cleaned into one problem
  → cause → fix flow per issue, one final status summary).

  Clean build on Linux, clean cross-compiled `cargo check`/`clippy` for
  Windows, all 43 tests still passing (no test coverage possible — this
  is DirectComposition/compositor-level state, same category as the rest
  of this investigation). Three real, distinct bugs found and fixed this
  session on the road to Windows transparency (tearing flag, alpha mode,
  resize/Commit) — two in `wgpu-hal` itself, one in this app's own code —
  each one traced to a specific piece of real evidence (an HRESULT, a
  D3D12 debug-layer message, a screenshot) rather than guessed. Still
  needs the developer's real-hardware confirmation — but the process
  change (slow down, confirm before re-guessing, ask a narrowing question
  when confidence is low) is worth carrying forward regardless of how
  this particular round turns out.

  **Update — same session:** the diagnostic logging paid off immediately.
  Developer's log showed `SetContent`/`Commit` succeeding on every resize
  with correct dimensions — confirming the branch runs exactly as
  intended, which meant the "missing Commit" theory, despite being a real
  gap upstream's own comment already flagged, was *not* the actual cause
  of the frozen-rect symptom. Ruled out with evidence rather than left
  ambiguous — exactly what the diagnostic step was for.

  That redirected the investigation to a layer this session hadn't
  seriously questioned yet: winit itself, not wgpu-hal. Recalled from
  earlier reading (during the very first Windows transparency
  investigation) that winit's Windows backend calls
  `DwmEnableBlurBehindWindow` — an older, GDI-redirection-surface-based
  transparency mechanism — automatically for any `transparent: true`
  window, *unless* the window is also created with
  `WS_EX_NOREDIRECTIONBITMAP`. This app's window has been transparent
  without that flag this whole time. Two independent transparency
  mechanisms were active on the same window at once: our DirectComposition
  visual (correct, and now resize-aware) and DWM's own legacy blur-behind
  surface (created once at window-creation size, with zero resize
  awareness) — the frozen rectangle was the *latter*, which is exactly why
  perfecting the DirectComposition side alone could never have fixed it.

  Fix confirmed available via winit's own public API — no new patch
  needed, no guessing required: `winit::platform::windows::
  WindowAttributesExtWindows::with_no_redirection_bitmap(true)`, which
  sets `WS_EX_NOREDIRECTIONBITMAP` and is also Microsoft's own documented
  recommendation for any app presenting through its own swapchain instead
  of GDI — using the intended tool for this situation, not a workaround.
  Added `platform_window_attributes()` in `crates/app/src/main.rs`
  (`#[cfg(target_os = "windows")]`, no-op elsewhere) applying it to the
  window's attributes at creation. Downgraded the temporary `warn`-level
  DirectComposition diagnostics in the vendored `wgpu-hal` patch to
  `debug` now that they've served their purpose (kept, not deleted — real
  fix, easy to re-promote if ever needed again). Updated `vendor/
  README.md` with this as a clearly-separated fourth issue, explicit that
  the third (Commit) theory was real but not the cause, and that this
  final fix lives in `crates/app`, not the vendored copy.

  Four real, distinct issues found and fixed across this one investigation
  (tearing flag, alpha mode, an incomplete-but-not-causal resize/Commit
  gap, and the actual cause in `winit`'s window creation) — worth noting
  for the pattern, not just the outcome: two of the four looked solid
  enough to report with real confidence and were still wrong or beside
  the point, and both times the thing that actually cut through was
  getting a piece of ground-truth evidence (the D3D12 debug layer message;
  the diagnostic log confirming Commit succeeded) rather than reasoning
  further from what was already known. Clean build/clippy on Linux, clean
  cross-compiled build/clippy for Windows, all 43 tests still passing.
  Still needs the developer's real-hardware confirmation — but for the
  first time in this thread, the fix isn't layered on top of an
  unconfirmed assumption; the thing it's replacing (double transparency
  mechanisms) is directly evidenced, not inferred.

  **Update — same session: confirmed.** "Yay! It works!" — Milestone 6 is
  done, real per-pixel Windows transparency working correctly including
  through resizes. Four real bugs found and fixed across this single
  feature (wrong swapchain target, an upstream `wgpu-hal` tearing-flag
  bug, a wrong alpha-blend convention, and a `winit` window-creation flag)
  — updated `.waypoint/project.md`'s Milestone 6 entry and phase summary
  to reflect confirmed status, consolidating the blow-by-blow into a
  numbered summary there (full detail stays here and in `vendor/
  README.md`). Still uncommitted (commit-timing rule) — this is a large
  milestone's worth of real, verified work sitting ready.

  WSL's separate transparency report (fully see-through + click-through)
  remains open, low-priority — not independently confirmed as WSL-
  specific yet, and per established precedent (cursor themes) not worth
  chasing speculatively; would need a real non-WSL Linux desktop test to
  actually move on. Next: confirm with the developer whether to pursue
  that now, or treat it as a known dev-environment quirk and move on to
  Milestone 7 (session file) per the build plan.

  **Update — same session:** developer's call, clean and quick: "WSL is
  not a primary target for us at all... let's just state that transparency
  is not supported on a WSL environment. Update code and configuration UI
  to reflect that." Implemented rather than just documented:
  - New `crates/app/src/platform.rs` with a shared `is_wsl()` — `main.rs`
    already had a private copy (used to force X11 over Wayland, a much
    earlier WSLg fix this session), duplicated rather than reused since it
    was originally `main.rs`-only; consolidated into one place now that a
    second module needs it too.
  - `Graphics::new`: skip requesting `CompositeAlphaMode::PreMultiplied`
    on WSL outright, leaving the surface `Opaque` there instead of relying
    on capability detection to naturally exclude it — deliberate, since
    testing showed WSLg's Vulkan/llvmpipe backend *does* claim
    `PreMultiplied` support, it just doesn't composite it correctly. This
    is the same "it lies about what it supports" category as the tearing-
    flag bug, just not worth chasing further on a non-target platform.
  - `Graphics::redraw`: force the background clear alpha to fully opaque
    on WSL too, not just at the surface-config level — otherwise the
    premultiplied shader math (added earlier this session for the Windows
    fix) would still dim every rendered color by the configured
    transparency level, for zero visible benefit, since an `Opaque`
    surface ignores the alpha channel entirely rather than blending it.
  - Settings panel (`ui.rs`): the transparency slider is disabled (`egui`'s
    `add_enabled_ui`, not hidden) under WSL, with a one-line note — so
    dragging it doesn't silently do nothing with no explanation. The
    underlying config value is untouched either way; only the runtime
    effect and this control are gated.
  - Verified directly rather than asking the developer to test — this dev
    environment genuinely *is* WSL, so for once this didn't need separate
    hardware. `--verbose` output confirmed the new code path engages
    exactly as intended: `wgpu: transparency unavailable here (offers
    [PreMultiplied, Inherit], wsl=true)`, app ran normally otherwise (pane
    spawn, shell startup, mouse events all fine — no regression).

  Clean build/clippy/tests on Linux, clean cross-compiled build/clippy for
  Windows. This closes out Milestone 6 (transparency) completely — both
  target platforms (Windows confirmed working by the developer, native
  Linux presumed fine via the same code path minus the WSL-specific
  exclusion, WSL explicitly and correctly disabled rather than left
  broken-looking). Still uncommitted (commit-timing rule). Next per
  `.waypoint/plan/v1-build-plan.md`: Milestone 7 (session file — OSC 7 cwd
  capture, save/restore layout+cwd on quit/relaunch).

  **Update — same session:** wrong — developer reported the window was
  still fully see-through, "I think you may have missed something." They
  were right. The WSL fix only touched `Graphics::new`'s swapchain alpha-
  mode request; it never touched `main.rs`'s `.with_transparent(true)` on
  *window creation*, which is a separate mechanism — on X11 that call
  alone makes winit request a 32-bit ARGB visual for the window, and
  WSLg's compositor was evidently keying off *that* to decide whether to
  composite with alpha, independent of whatever `CompositeAlphaMode` the
  swapchain later requested. Recognized immediately as the same shape of
  bug as the Windows DirectComposition-vs-redirection-bitmap issue from
  earlier this session — two independent, separately-controlled
  transparency mechanisms, and only one had been turned off.

  Fixed by making the window-creation-time `with_transparent(true)` call
  itself conditional on `!platform::is_wsl()`, alongside the swapchain-
  level exclusion already in place.

  Verified with the same before/after comparison technique, directly
  (still WSL, still no separate hardware needed): with only the first
  (incomplete) fix, `--verbose` showed `caps.alpha_modes` as `[PreMultiplied,
  Inherit]` — driven by the still-ARGB window, confirming the window
  itself, not just the swapchain request, was the live variable. With the
  window-creation fix added, the *same* surface now reports `[Opaque,
  Inherit]` instead — concrete proof the window is no longer ARGB, not an
  assumption. This diff (same log line, two different `caps.alpha_modes`
  outputs, before and after the second fix) is about as close to a
  controlled experiment as this investigation got all session — worth
  remembering as a technique: when a fix doesn't visibly work, re-running
  the exact same diagnostic before and after a candidate change, and
  diffing the output, can confirm or rule out a hypothesis directly rather
  than relying on "should work now" reasoning alone.

  Clean build/clippy/tests on Linux, clean cross-compiled build/clippy for
  Windows. Genuinely closes out Milestone 6 now — both fixes verified with
  direct evidence, not just confident-sounding reasoning (the exact
  failure mode called out in `[[feedback-debugging-confidence-
  calibration]]` from earlier this session, and a good reminder that
  "verified directly" for one platform doesn't mean the fix was complete,
  just that the mechanism tested was confirmed — always worth checking
  whether a symptom could have more than one contributing cause before
  declaring it closed).

- **2026-07-20:** Asked whether pane title bars (Terminator/iTerm2-style)
  were in the plan — they weren't (CONOPS only mentions "pane titles" in
  passing as something egui *could* render someday, no design doc, no
  build-plan task). Suggested treating it as a proper Planning-phase
  design doc before touching code, same process every other named feature
  in this project has gone through. The developer skipped that: replied
  with the full spec directly (title bar colors/contents, group-color
  randomization + contrast rule, default background color + settings
  entry, three context-menu changes) and "Let's get these knocked out" —
  so implemented from their spec directly rather than insisting on the
  design-doc ceremony first, since they'd effectively already done that
  step themselves in prose.

  Resolved the one real open technical question before writing anything:
  "the name of the currently running application" needs a per-pane
  signal, and raw process-tree introspection is platform-specific and
  heavy. Checked `alacritty_terminal` first rather than assuming it was
  needed — it already parses OSC 0/1/2 and emits `Event::Title`/
  `Event::ResetTitle`, exactly the same signal every real terminal
  emulator uses for tab/pane titles (not process introspection either).
  `crates/pane/src/term.rs`'s `EventProxy` (previously forwarding only
  `Event::PtyWrite`) now also forwards these; `Screen::title() -> Option
  <&str>` exposes the latest one. Resolved the question by reading the
  dependency rather than asking the developer or reaching for something
  heavier — confirmed with a real shell's actual OSC output during later
  smoke-testing (`\x1b]0;user@host: /path\x07`, bash's own default).

  Implemented in order, largest/most foundational piece first:
  - **Router grouping redesign** — `GroupId` changed from an opaque `u64`
    with one hardcoded `DEFAULT_GROUP` to wrapping a user-chosen `String`
    directly (the name *is* the identity — matches how the UI works:
    type a new one or pick an existing one, never juggling a separate id).
    `assign_to_group(pane, name)`/`remove_from_group(pane)` replace the
    old binary `toggle_group`; a group is deleted the instant its member
    count hits zero (`remove_from_group` and `assign_to_group`'s internal
    "leave the old group first" both funnel through the same cleanup, so
    there's exactly one place this rule lives). `group_names()` lists
    every group with a member, sorted, for the context menu's dropdown.
    `Action::ToggleGroup` removed outright, not just left unbound — it
    needs a name a keyboard chord can't carry, so unlike broadcast-mode
    (which kept its `Action` variant with no default chord), this one has
    no `Action` shape that would even make sense. 8 new/updated tests.
  - **Title bar geometry** — `Graphics::content_rect`/`title_bar_height`
    carve the title bar off the top of every pane rect, scaled to font
    size (not a fixed pixel constant, so it doesn't waste space at large
    fonts or clip text at small ones). Every place that used to convert a
    raw pane rect to grid rows/cols, or position text/cursor/selection,
    now goes through this instead — resize, split, and initial spawn
    sizing all needed the same fix, not just rendering. Verified directly
    (WSL again): pane spawn size actually dropped from 80×30 to 80×28
    cells after this change, confirming the reservation is real and not
    just visually drawn over the grid.
  - **Title bar rendering** — dark grey/light grey by default, centered
    title (from the new `Screen::title()`, falling back to `"shell"`).
    Grouped panes get a background from a 10-entry palette
    (`GROUP_COLOR_PALETTE`) keyed by hashing the group's name — chosen
    deliberately over genuinely re-rolling random on every group creation:
    a name-keyed hash means the same group name always gets the same
    color across restarts/reassignments/reloads, which reads as a stable
    identity rather than a flickering one. Text contrast computed via
    perceived luminance (`0.299r + 0.587g + 0.114b`, the standard
    green-weighted formula, not a flat average) — light text on a
    dark-picked color, dark on light, per the developer's own stated rule.
  - **Click-to-focus vs. title-bar clicks** — `cell_at` (the mouse-
    position-to-grid-cell conversion used for both mouse-reporting and
    text selection) now returns `None` above the content rect entirely,
    rather than clamping into it. A click landing in the title bar still
    focuses the pane and can open its context menu (both go through
    `pane_at`, which still uses the full rect, unchanged) but can't start
    a selection or forward a mouse report — chrome, not grid.
  - **Context menu** — added both split commands (previously keyboard-
    only) and replaced the single group toggle button with a new-name
    text field + "Add" plus a dropdown of existing groups. Along the way,
    found and fixed a latent correctness gap: `Graphics::split` always
    split the *focused* pane, so a context-menu split command would have
    silently split the wrong pane whenever you right-clicked a pane that
    wasn't already focused. Split into `split_pane(pane, orientation)`
    (explicit target — what the context menu now uses) and `split
    (orientation)` (focused-pane convenience, what the keyboard chord
    path still uses) — the context menu already targets a specific
    right-clicked pane for broadcast/group actions, split needed the same
    treatment for consistency, not special-cased differently.
  - **Background color config** — `appearance.background_color` as
    `#rrggbb` hex (default black), `Appearance::background_rgb`/
    `set_background_rgb` convert to/from 0.0–1.0 RGB for the renderer and
    the new settings-panel color picker (`egui::color_edit_button_rgb`);
    an invalid hex value falls back to black rather than a load error,
    matching every other "never crash on a bad edit" spot in config.

  Clean build/clippy/tests on Linux (50 tests, up from 43), clean cross-
  compiled build/clippy for Windows. Ran the actual app directly here
  (WSL) as a smoke test after each major chunk rather than only at the
  end — confirmed no panics, correct config loading with the new field,
  and the row-count drop proving title-bar geometry took effect — but
  genuinely can't confirm the *visual* result (colors, contrast, text
  alignment, whether the context menu reads well) without the developer's
  own eyes; that's the next step, same as every other milestone. Still
  uncommitted (commit-timing rule). This work sits between Milestone 6 and
  Milestone 7 in `v1-build-plan.md` — not part of the original 9-milestone
  plan at all, a direct developer request implemented from their own
  detailed spec rather than routed through a design doc first.

  **Update — same session:** developer liked it ("Fantastic") but flagged
  two follow-ups on the same message: the title should show only the
  application, not the full path; and Windows needs a way to configure the
  default shell (cmd/PowerShell/WSL — no single obvious default the way
  Linux/macOS have one).

  The shell-picker half was unambiguous — implemented immediately (three
  Windows-only quick-pick buttons filling the existing free-text field).

  The title half needed real investigation before touching code, not a
  quick trim. Checked what was actually being shown (`will@host: /mnt/c/
  Users/Will`, bash's own default OSC 0 output) and realized the deeper
  problem: most shells, out of the box, only update that OSC title *at the
  prompt* — host plus current directory — never while a program is
  actually running. Trimming the path would still never show "vim" or
  "npm"; the mechanism itself couldn't answer what it was being asked.
  Explained this plainly rather than quietly shipping a trim-only patch,
  laid out the real fork (quick heuristic trim vs. actual OS-level
  foreground-process detection, the latter being real, disclosed
  per-platform work — straightforward-ish on Unix, meaningfully harder on
  Windows), and gave a recommendation while asking. Developer: "Nope. I
  want the foreground process. For sure." — direct, unambiguous, no
  hedging. Committed to the real implementation.

  Investigated properly before writing anything (the pattern this project
  keeps returning to: check what the dependency already provides before
  assuming heavier work is needed): `portable_pty::MasterPty` already has
  `process_group_leader()` — literally `tcgetpgrp` on the pty master,
  gated `#[cfg(unix)]` in the trait itself — meaning the correct, standard
  Unix mechanism was ALREADY built into a dependency already in use, not
  something to add. Windows has no equivalent concept at all (ConPTY
  doesn't expose foreground-process-group semantics the way a POSIX pty
  does) — the best available signal there is walking the process tree
  down from the shell's own pid, which needs enumerating processes/
  parent-child relationships; rather than hand-roll raw Toolhelp32Snapshot
  calls (Windows) alongside `/proc` parsing (Linux) and `libproc`/`sysctl`
  FFI (macOS) as three separate implementations, added `sysinfo` (to
  `app` only, trimmed via `default-features = false` to just its `system`
  feature) as ONE cross-platform "pid → name" / "pid → children" backend
  everywhere, while still using the real `tcgetpgrp`-based pid specifically
  on Unix rather than falling back to the tree-walk approximation there
  too. Same design principle as using `alacritty_terminal`/`portable-pty`
  themselves: reuse a maintained cross-platform primitive instead of
  reinventing three raw-syscall implementations.

  Removed the OSC-title mechanism entirely rather than leaving it as a
  fallback — `pane::Screen`'s `Event::Title`/`ResetTitle` tracking (added
  earlier this same session) is now provably dead code for the purpose it
  was built for, so it came out completely (`EventProxy`, `Screen::title`,
  its test) rather than lingering unused. `crates/app/src/
  foreground_process.rs`'s `ForegroundProcesses`: a shared (one per
  `Graphics`, not one per pane — a full-system scan already covers every
  pane at once), throttled (500ms — process enumeration touches every
  process on the system, wasteful to repeat every single frame across
  every pane) snapshot. `name_for(shell_pid, foreground_pgid)` prefers the
  Unix pgid signal when given one; otherwise walks down from the shell's
  own pid picking the most-recently-started live child at each level —
  zero iterations (shell idle at its prompt) naturally falls back to
  looking up the shell's own name, which also correctly handles "platform
  default shell" without needing to separately track what got resolved.

  Verified with real subprocesses, not just mocks or compilation — three
  unit tests spawn actual `std::process::Command` children to check the
  lookup/priority/fallback logic in isolation, and a fourth spawns a real
  `pane::Pty`, writes `sleep 5\n` into a real shell, and confirms
  `foreground_pgid` + the lookup correctly resolve to `"sleep"` — the
  exact pipeline the running app uses, verified end to end on the one
  platform available here, not asserted from reading the code. The
  Windows tree-walk path is unit-tested in isolation (same `name_for`
  function, same tests, since the priority-order test also exercises the
  fallback path) but its OS-specific behavior (does `sysinfo`'s Windows
  backend report parent/child relationships the way assumed) remains
  unverified beyond compiling — flagged as a real, disclosed limitation,
  not asserted as working.

  Clean build/clippy/tests on Linux (53 tests, up from 50), clean cross-
  compiled build/clippy for Windows. Still uncommitted (commit-timing
  rule) — this and the title-bar/grouping feature before it are both
  still awaiting the developer's own interactive pass.

- **2026-07-22:** Developer confirmed the title-bar/grouping feature set
  works well interactively, then reported two bugs on the same message:
  htop's title showed only the parent (bash) when run inside a WSL shell,
  and brighter group title-bar colors weren't flipping to dark text.

  Contrast bug: root cause found on the first pass, no guessing needed —
  the swapchain format is `Bgra8UnormSrgb` (confirmed via egui's own
  startup log line from earlier this session), meaning the GPU gamma-
  encodes every color on write. The luminance check was judging brightness
  on the raw linear value, which reliably under-estimates how bright a
  color actually renders (linear 0.5 displays like sRGB ~0.735). Added
  `srgb_encode()` ahead of the existing luminance weighting in
  `graphics.rs`; 4 new unit tests including one built directly from the
  reported palette color (teal, raw luminance ~0.474 → wrongly "light",
  sRGB-corrected ~0.700 → correctly "dark").

  htop bug: three escalating real-PTY test scenarios in this same WSL
  dev environment (tracker built before the process existed; `htop`
  specifically end to end; 20 repeated refresh cycles over 2.5s) all
  correctly resolved to `"htop"` — could not reproduce directly. Rather
  than guess again, added verbose diagnostic logging to `redraw()` and
  asked the developer to reproduce with `--verbose` and share the actual
  `shell_pid`/`foreground_pgid`/`name` line.

  Developer's actual report, once it came back: starting PowerShell (the
  default), then running `wsl`, then `htop` inside that — a nested-shell
  case none of the three tests covered (they all ran a program directly
  in the pane's own shell, never `wsl.exe` launched from within another
  shell). Root cause: the pane's foreground detection is anchored to
  Windows' own process tree; `wsl.exe` hands the actual foreground over to
  a different kernel entirely (the WSL2 VM), invisible to `sysinfo` on the
  Windows side — there's no pid to walk to, structurally, not a bug to fix.
  Developer, presented with the finding and two options (heuristically
  detect a shell change, or a manual "swap shell" action): picked swap
  shell directly ("I think I like that idea best").

  Implemented as a new "Swap shell" section in the existing right-click
  context menu (reused, not a new menu — matches the app's own established
  precedent of one context menu for all pane-level actions): Windows-only
  quick-pick buttons (cmd/PowerShell/WSL, mirroring Settings' existing
  preset row) plus a free-text field. Picking one calls new
  `Graphics::restart_pane_shell`, which replaces just that pane's
  `PaneSession` (dropping the old one kills its `Pty` via `Drop`) while
  leaving layout/group/broadcast state on that `PaneId` completely alone —
  deliberately not `close_pane` followed by a respawn, which would have
  torn all of that down too.

  Developer tested the swap-shell fix and reported it *still* showed the
  wrong title — now literally "conhost.exe", with real verbose data this
  time (`shell_pid=Some(17264) foreground_pgid=None name=Some("conhost.exe")`).
  Asked the developer to check what pid 17264 actually was before touching
  code again (`Get-Process -Id 17264`) — confirmed it really was `wsl.exe`,
  meaning the earlier "no pid to walk to" theory was incomplete: `wsl.exe`
  actually does spawn a real, permanent Windows-side `conhost.exe` helper
  child for interop, and since nothing else Windows can see ever
  supersedes it, "most recently started live child" picked it forever.
  Fixed by excluding known Windows console-host implementation-detail
  processes from the tree-walk (`is_console_host_implementation_detail`);
  a WSL-rooted pane now honestly bottoms out at `wsl.exe`'s own name
  instead of the wrong stuck value. Second real bug this feature has
  surfaced from actual hands-on testing, both fixed with real evidence
  rather than blind guesses — consistent with the debugging-confidence-
  calibration lesson from earlier in this project.

  Same message also reported colored output and scrollback both entirely
  absent, and asked for a font-family selector above font size in
  Settings. Investigated each before touching code:
  - Colors: the renderer was drawing every glyph in one hardcoded gray,
    completely ignoring each cell's actual SGR fg/bg — the per-glyph color
    plumbing already existed in `render`, just never fed real data. Added
    `crates/app/src/color.rs`: the 16 base ANSI colors, the 256-color
    cube/grayscale ramp, true color, bold-as-bright, and reverse-video,
    wired into the render loop. 6 new unit tests.
  - Scrollback: smaller than expected once actually checked —
    `alacritty_terminal` already retains 10,000 lines of history by
    default (`Config::scrolling_history`); nothing ever exposed it. Added
    mouse-wheel scrolling (`pane::Screen::scroll`/`scroll_to_bottom`, a new
    `MouseWheel` handler in `main.rs`), viewport-offset-aware rendering,
    typing snaps back to live output, cursor hides itself while scrolled
    back. Safe no-op inside full-screen programs (their "alternate screen"
    has zero history by design).
  - Font selector: `config.appearance.font_family` already existed but had
    no UI and was never actually threaded into the renderer at all
    (`cosmic-text`'s `Family::Monospace` was hardcoded). Added a dropdown
    populated from every monospaced font actually installed (via
    `cosmic-text`'s font database, filtered on `FaceInfo::monospaced`),
    not a free-text guess; threaded `font_family` through
    `measure_cell`/the atlas cache key/`GridRenderer::render`.

  Also asked to cut down `--verbose` noise ("streams of all kinds of
  mouse and other output... hard to visually navigate/filter"). Replaced
  the single boolean with categories (`General`/`Mouse`/`Pty`/
  `Foreground`), bare `--verbose` now only the low-frequency structural
  stuff; the noisy streams (mouse motion, raw PTY byte/keystroke dumps,
  the foreground-scan line) are each an explicit
  `--verbose=mouse|pty|foreground|all` opt-in.

  Once that whole thread of work was confirmed as "done as much as we can
  here," developer said to proceed with the next chunk — Milestone 7
  (session file), next in `.waypoint/plan/v1-build-plan.md`. Before
  building it, checked whether OSC 7 (cwd reporting) actually works the
  way CONOPS §5g assumed ("already parsed by alacritty_terminal") — it
  doesn't; checked directly in both `vte`'s and `alacritty_terminal`'s
  vendored source, `vte`'s OSC dispatch has no case for `7` at all, just
  falls through to an unhandled-sequence debug log. Real fork: patch `vte`
  + `alacritty_terminal` (two-crate fork, bigger footprint than the
  existing single-file `wgpu-hal` precedent), hand-roll an independent
  scanner, or drop OSC 7 and rely on OS-level lookup + home dir alone
  (CONOPS explicitly says Windows depends on OSC 7 doing the work, so this
  would be a real regression there). Presented the tradeoff, recommended
  the independent scanner; developer confirmed ("that sounds minimal
  enough to do ourselves") after asking to double check it's really just
  "listening for a directory change" (yes — the shell voluntarily
  reporting itself at each new prompt, not a filesystem watch).

  Then confirmed the rest of Milestone 7's shape directly: save on quit,
  auto-restore on next launch, never restart running programs (matching
  CONOPS exactly), plus store layout/window size too — framed as "feels
  like a config file entry... hand-writable if desired, but we'll manage
  it automatically." Implemented all of Milestone 7:
  - `pane::cwd` (new module): self-contained OSC 7 scanner, independent of
    `vte` entirely — watches raw PTY bytes for `ESC ] 7 ; file://host/path
    (BEL|ST)` alongside (not instead of) the VT parser, buffers a partial
    sequence across chunked PTY reads, percent-decodes, handles the
    Windows drive-letter form, bounds a never-terminated sequence so it
    can't buffer forever. 10 new tests.
  - `ForegroundProcesses::cwd_of` (OS-level fallback): a separate on-demand
    single-pid refresh (`ProcessRefreshKind::nothing().with_cwd(Always)`),
    not part of the continuous 500ms scan — cwd is only ever needed once,
    at save time. `remove_dead_processes: false` deliberately, so it can't
    prune the wider process cache `name_for`'s tree-walk relies on.
  - `session_cwd::resolve` (new module): the pure fallback chain — OSC 7,
    then OS-level, then home directory — decoupled from any I/O so it's
    trivially unit-tested.
  - `layout::SavedNode` + `Layout::snapshot`/`from_snapshot`: a
    serializable tree shape (orientation/ratio/leaves, no pane identity —
    ids never survive a restart). Restored panes correlate to saved
    per-pane state *positionally*, not by id — both `Layout::panes()` and
    a snapshot's own leaves walk the tree in the same left-to-right,
    depth-first order, exercised directly by a dedicated test since a
    silent drift here would misattribute one pane's saved cwd/group to a
    different restored pane.
  - New `session` crate (`Session`/`WindowSize`/`PaneState`, TOML,
    `config::dir()` reused for the file location — exposed that as a
    public function rather than duplicating the platform-detection logic)
    mirroring `config::Config`'s own load/save conventions, but
    deliberately a separate file (`session.toml`) from `config.toml`:
    written on every quit, so folding it into the settings file would mean
    every quit also rewrites (and risks clobbering a concurrent hand-edit
    of) the user's actual preferences.
  - `pane::Pty::spawn`/`PaneSession::spawn` gained an optional starting
    cwd (`CommandBuilder::cwd`, already in `portable-pty`, just never
    wired up). Verified with a real spawned shell — first attempt used the
    developer's actual default login shell and kept failing until reading
    `~/.bashrc` explained why: a hardcoded `cd /mnt/c/Users/Will` on line
    121 runs *after* the correctly-applied `cwd` and silently overrides
    it. Switched the test to `/bin/sh` (no dotfiles) rather than chasing a
    non-bug.
  - `Graphics::new` takes `Option<session::Session>`: rebuilds the exact
    layout via `from_snapshot`, spawns each pane into its saved cwd, then
    reapplies group membership; a pane-count mismatch between the
    snapshot and the saved per-pane list is treated as a corrupted file
    (falls back to a normal fresh single-pane start, not a partial/
    misaligned restore). `main.rs` loads the session before window
    creation (needed for `.with_inner_size`) and calls the new
    `Graphics::save_session` from every real quit path (`CloseRequested`,
    the chord-driven quit, and the "closed the last pane" path — the
    latter still has that pane in `self.panes` at the moment it fires, so
    its cwd/group still get saved correctly).

  Full workspace build/clippy/tests clean throughout (31 tests in `app`,
  up from 26; new `layout` at 14, `pane` at 19, `session` at 3 — all new),
  clean cross-compiled build/clippy for Windows. One long-standing flaky
  test unrelated to any of this (`real_pty_reports_the_actual_foreground_
  command`, timing-sensitive under parallel test load — passes reliably
  alone or on a rerun). Confirmed the app still launches and behaves
  correctly end to end in this dev environment (WSL, llvmpipe software
  rendering) after all of the above. Could not verify session save/
  restore interactively end-to-end here — `WindowEvent::CloseRequested`
  needs a real window-manager close action (clicking the title bar's ✕,
  Alt+F4, ...), not a process signal, and this sandbox has no tool to
  simulate that — flagged to the developer as needing their own pass on
  real hardware. Still uncommitted (commit-timing rule).

  **Same day, developer's real-hardware pass on session restore:**
  layout/window size restored fine; directory and chosen shell did not.

  Chosen shell: a real, simple gap, not a bug — `PaneSession` never
  actually remembered which shell string it was spawned with anywhere, so
  there was nothing *to* persist. Fixed directly: a `shell: Option<String>`
  field (`None` = "whatever the configured default was," `Some` = an
  explicit override like a past "Swap shell"), persisted in
  `session::PaneState` (`#[serde(default)]` so an old session file missing
  the field still loads), restore prefers it over the current default.

  Directory: asked to actually read `session.toml` off the developer's
  real machine before guessing further (same evidence-first pattern as
  the earlier `conhost.exe` bug) — every single saved pane, including two
  that had been swapped to WSL, showed the identical path
  (`C:\Users\Will\`). This pinned down *two* separate structural gaps
  landing on the same home-directory fallback: (1) a WSL-rooted pane's
  cwd, from Windows' side, is `wsl.exe`'s own Windows-side cwd (wherever
  the app itself launched from) — the real Linux-side directory is
  invisible to Windows process introspection entirely, the exact same
  WSL2 boundary already hit for foreground-process detection, structurally
  unfixable via any OS-level lookup; (2) even the plain PowerShell pane
  came back wrong, meaning `sysinfo`'s Windows cwd lookup (a fragile
  `ReadProcessMemory`-based read of another process's `RTL_USER_PROCESS_
  PARAMETERS`) was *also* failing there, for reasons undiagnosable without
  a real Windows box to iterate against. Net effect: neither of the two
  fallback signals actually works in practice on Windows — vanilla
  PowerShell/WSL-bash don't emit OSC 7 on their own, and the OS-level
  backstop doesn't reliably work either.

  Flagged the real fix (inject minimal OSC-7-emitting shell integration at
  spawn time, the same technique iTerm2/Windows Terminal/VS Code use) as a
  meaningfully bigger scope step and asked before building it — developer
  pushed back directly ("If iTerm2 does it, then what's the risk?").
  Fair challenge: the risk is only real if done by clobbering a user's own
  prompt/dotfiles; done the way real terminals actually do it (compose,
  don't replace), it's a solved problem. Committed to it.

  Implemented as a new `pane::integration` module — entirely at spawn
  time, never editing the user's actual `.bashrc`/`$PROFILE`:
  - **Bash**: `--rcfile <generated script>`. `--rcfile` fully replaces
    bash's own startup-file sourcing (not just `.bashrc`), so the
    generated script manually replicates a normal login shell's own
    sequence (`/etc/profile`, then `.bash_profile`/`.bash_login`/
    `.profile`, whichever exists), then `.bashrc` too, *then* adds the
    OSC 7 hook by appending to `PROMPT_COMMAND` (not overwriting it) —
    so a user's own customizations, wherever they live, still apply.
  - **PowerShell**: `-NoExit -Command <script>`. Profile scripts still
    load normally first (only `-NoProfile` would skip them); the script
    captures whatever `$function:prompt` already is at that point (the
    user's own redefinition, or PowerShell's built-in default if they
    never touched it) and wraps it — never replaces it outright.
  - Deliberately **not** covering cmd.exe (needs a different escape
    sequence, ConEmu's `OSC 9;9`, not implemented) or `wsl.exe`'s inner
    shell (unknowable from the Windows side — bash, zsh, fish — forcing
    one would risk silently changing what a user's WSL session runs).
    Both spawn completely unchanged; explicitly flagged as a known gap,
    not silently dropped.
  - To let this apply even to the *default* shell (not just an explicit
    override), `None` is now also resolved (mirroring `$SHELL` on Unix)
    purely to decide whether to inject — `CommandBuilder::new_default_
    prog()` can't take extra arguments at all (panics if `arg` is called
    on it), so a classified shell always switches to spawning explicitly;
    the generated script's manual profile-sourcing is exactly what keeps
    that switch behaviorally equivalent to the login shell it replaces.

  Two real bugs caught before landing, both through actually running the
  code rather than trusting it by inspection:
  - The generated script's path is a single fixed, shared location
    (`$TMPDIR/pain-shell-integration/...`), not unique per spawn — a
    direct `std::fs::write` raced two concurrent spawns (caught by an
    actual test failure: two panes' worth of real bash processes writing
    to it around the same time, one reading a half-written file). Fixed
    with a write-to-temp-then-rename, atomic on the same filesystem.
  - After the fix above, two *pre-existing* tests (`real_pty_reports_the_
    actual_foreground_command`, `htop_name_stays_correct_across_many_
    refresh_cycles`) started failing — not flakiness, reliably. Root
    cause: both spawn with `shell: None`, which resolves to bash on a
    real dev box and now goes through the injection path; the extra
    rcfile-sourcing startup work meant bash hadn't finished becoming
    interactive yet when the test's short fixed delay elapsed, so
    `tcgetpgrp` still reported bash's own pgid, not the child job's.
    Not a regression in the mechanism itself — those two tests are about
    job-control correctness, not shell integration — switched them to
    `Some("sh")` (no injection, same kernel-level behavior) instead of
    chasing a longer, still-fragile fixed delay.
  - Verified the whole mechanism genuinely end to end, not just via
    hand-crafted bytes: a new test spawns a real `bash` through the actual
    injection path (no manual OSC 7 write at all) and confirms `Screen::
    cwd()` goes from `None` to `Some(real_path)` purely from bash's own
    first prompt. Its first version asserted the *exact* spawned-into
    directory and failed — correctly — because this dev box's own real
    `~/.bashrc` (line 121: `cd /mnt/c/Users/Will`) does exactly what a
    user's own dotfile is allowed to do: change directory after the
    script sources it. Fixed the assertion to check that some real
    absolute path was reported (proving the hook fires), not the exact
    value (a different, dotfile-free test already covers that `cwd` the
    argument is honored).

  Full workspace build/clippy/tests clean (`app` unchanged at 31,
  `pane` up to 24 — 5 new). Clean Windows cross-compiled build/clippy.
  PowerShell path still unverified beyond compiling and careful reading —
  no PowerShell available in this sandbox to run it against; flagged
  plainly, not asserted as working. Still uncommitted (commit-timing rule).

  **Same day, second real-hardware pass:** cmd.exe was the only shell
  that actually persisted its directory (presumably via `sysinfo`'s
  OS-level lookup working fine for a small, simple Win32 process — bash
  under WSL was never in scope for injection at all, that's the disclosed
  `wsl.exe`-inner-shell gap; PowerShell's injection had no effect).

  Narrowed it down before touching code, not guessing again: asked
  whether the PowerShell pane showed any errors (none) and whether the
  prompt looked normal (yes) — inconclusive either way, since a correctly
  working OSC 7 write is *supposed* to be invisible. Asked for one more
  concrete check: read back `$function:prompt` directly in the real pane.
  It came back as exactly the injected function body — proof the
  `-NoExit -Command` args really did execute and really did install the
  wrapped prompt (ruling out an escaping/quoting failure in how the args
  reach `CreateProcessW`, which was the other real suspect).

  With installation confirmed, the remaining explanation was Windows'
  own console host: `Write-Host` goes through the legacy Console API,
  and ConPTY/conhost's own VT-translation layer apparently doesn't
  forward a raw OSC 7 sequence the way a genuine Unix pty passes bytes
  through untouched (unlike the WSL/bash case, verified moments earlier
  working correctly in this very sandbox — a real Linux pty is a dumb
  byte pipe with no such reinterpretation happening). This lines up with
  known Windows Terminal/ConEmu behavior: they specifically added `OSC
  9;9` (a bare path, no `file://` wrapping) as *their own* convention for
  exactly this, because relying on OSC 7 through the legacy console
  layer isn't reliable — which also explains cmd.exe working, since (per
  this theory) it was never about cmd.exe having some default OSC
  emission at all.

  Fixed by switching PowerShell's injected sequence from OSC 7 to OSC
  9;9, and extending `pane::cwd::CwdWatcher` to recognize both
  conventions (whichever marker starts earliest in the buffer wins first;
  a later one still updates afterward) — `crate::cwd`'s module doc now
  explains why both exist. 8 new unit tests for the OSC 9;9 path
  (terminators, split-across-chunks, mixed-with-OSC-7-in-one-buffer),
  reusing the same buffering/termination logic already proven correct
  for OSC 7. cmd.exe itself remains out of scope (still relies on the
  OS-level fallback, which the developer's own test shows is already
  working for it).

  Full workspace clean (`pane` up to 29 tests, 5 new), Windows
  cross-compiled build/clippy clean. PowerShell's *installation* is now
  confirmed working on real hardware (via the `$function:prompt` check);
  the OSC 9;9 emission itself is reasoned through carefully but still
  unverified end-to-end there — needs one more real-hardware pass. Still
  uncommitted (commit-timing rule).

  **Same day, third real-hardware pass:** cmd/PowerShell both confirmed
  working now. Two things left: "wslhost" (turned out to be the *title
  bar* showing `wslhost.exe` for a WSL-swapped pane, not a cwd issue) and
  native bash under WSL still not tracking cwd correctly.

  `wslhost.exe`: same root cause as the earlier `conhost.exe` bug, just
  the *next* Windows-side interop helper `wsl.exe` spawns once
  `conhost.exe` was excluded — "COM Server for WSL" (clipboard/
  notification/interop plumbing), equally long-lived, equally not the
  real foreground command. Fixed the same way: added to
  `is_console_host_implementation_detail`'s exclusion list.

  Native bash under WSL: before assuming another Windows-specific
  mystery, actually reproduced the *real* `PaneSession`+`pump()` flow
  directly in this sandbox (not just the raw `Pty`/`Screen` pairing
  already tested) — passed cleanly. That ruled out a logic bug and
  pointed at something environmental: `Pty::spawn`'s own resolution of
  `shell: None` only checked `$SHELL`, not `portable_pty`'s *second*
  fallback (the current user's `/etc/passwd` entry) — a real gap, since a
  GUI/desktop-launched process doesn't necessarily inherit `$SHELL` the
  way an interactive terminal does. If unset, classification would
  silently land on `Family::Other` (no integration) even though
  `portable_pty` still correctly resolves and runs the very same bash.
  Fixed by mirroring `portable_pty::unix::get_shell()`'s exact fallback
  chain (`libc::getpwuid`, the same raw call the dependency itself
  already makes) in a new `integration::resolve_default_shell`.

  Developer confirmed both fixes, then asked directly for the one
  remaining, previously-disclosed gap: cwd tracking when a pane's own
  shell *is* `wsl.exe` (via "Swap shell"). Laid out why this is a
  meaningfully harder problem than bash/PowerShell before starting —
  needs detecting the distro's actual inner shell (unknowable from the
  shell string alone) and translating a Windows path into its WSL mount
  equivalent, neither of which is testable from this sandbox (it's
  *inside* the WSL guest already; `wsl.exe` itself is a Windows-side
  binary reaching into a real Windows host with a real distro, not
  reproducible here at all) — offered to build it best-effort with the
  developer testing on real hardware, or leave it documented. Developer:
  "Build it and I'll test. We're in this together."

  Implemented as a new `Family::Wsl` in `pane::integration`, decided at
  *run time* inside the WSL side rather than at spawn time from here —
  deliberately, to avoid a separate synchronous pre-flight `wsl.exe` call
  (real added latency for every WSL pane) just to ask what shell it runs.
  `wsl.exe`'s own bare invocation is replaced with `wsl.exe -- sh
  <translated-entrypoint-path>`; the entrypoint script itself resolves
  `$SHELL` (falling back to the passwd entry, then `/bin/sh`, so it can
  never fail to exec *something*), and only if that's bash does it exec
  into `bash --rcfile <translated-bash-integration-path> -i` — anything
  else execs the user's real configured shell completely unmodified,
  never forcing one they didn't already have. `windows_path_to_wsl_mount`
  translates via the *default* WSL2 automount convention (`/mnt/<drive>/
  ...`) — a real, disclosed assumption that breaks for a customized
  automount root, with no way to ask `wsl.exe` first without doubling
  spawn latency for every WSL pane just to cover an uncommon case.

  Passed a real quoting-safety lesson learned from the PowerShell
  integration's own escaping risk: rather than passing a complex shell
  script with embedded quotes as an inline `wsl.exe -- sh -c "..."`
  argument (another layer of Windows-command-line quoting on top of
  POSIX shell quoting, compounding risk), wrote it to a *file* instead —
  only a single plain path argument has to survive the Windows→WSL
  argument-passing journey, which is far less fragile.

  Full workspace clean (`pane` up to 35 tests, 6 new — `Family::Wsl`
  classification, path translation both ways, entrypoint content, and a
  `#[cfg(windows)]`-only end-to-end `apply` test that can't run here but
  compiles cleanly cross-compiled). Windows cross-compiled build/clippy
  clean, including compiling (not running) the Windows-gated test
  itself. None of the WSL-specific behavior is verified beyond careful
  reasoning and what compiles — genuinely cannot be, from this sandbox;
  explicitly framed to the developer as a best-effort first pass expecting
  real-hardware iteration, not a confirmed-working feature. Still
  uncommitted (commit-timing rule).

  Developer tested the WSL fix: no change, still broken. Rather than
  keep debugging blind (this sandbox can't drive a real Windows host into
  a real WSL distro at all, unlike every other fix this session that
  could at least be partially verified here), developer chose to set it
  aside — "WSL is kind of a wildcard anyway" — and redirect to a genuinely
  new, scoped feature: preset layout arrangement from the right-click
  menu (Horizontal/Vertical/Grid — "we're getting close to done").

  Implemented as `layout::Arrangement` + `Layout::arrange(panes,
  arrangement)`: rebuilds the tree from scratch into a preset shape while
  reusing the *exact same* `PaneId`s (unlike `from_snapshot`, which always
  mints fresh ones for session restore) — nothing respawns, only position/
  size change, so group membership and broadcast state (both keyed by
  `PaneId`) carry over automatically with no extra bookkeeping needed.
  `Horizontal`/`Vertical` build a flat chain of same-orientation splits
  with ratio `1/n, 1/(n-1), ..., 1/2` at each step, which works out to
  every pane getting an exactly equal share once multiplied through the
  splits already taken to get there. `Grid` computes `cols = ceil(sqrt(n))`
  rows via chunking, builds each row as its own `Horizontal` chain, then
  stacks the rows `Vertical`ly the same evenly-dividing way — the last
  row can hold fewer panes than the others (a real, intentional case, not
  a bug: 3 panes tile as a 2-then-1 grid, not 3 even columns). Wired into
  the existing right-click menu (a new "Arrange all panes" section right
  after "Split") and a new `Graphics::arrange_panes`.

  9 new `layout` tests (all portable, no Windows/WSL-specific unknowns
  this time — genuinely verifiable here): equal-width/height tiling for
  both flat arrangements, the 2x2 grid case, the uneven-row-count grid
  case, exact-id preservation through a non-contiguous id set (after a
  close), the single-pane trivial case, and that a split performed
  *after* an arrange doesn't collide with a kept id. Full workspace
  clean, Windows cross-target clean. Smoke-tested the real binary
  launches and restores an old session's 3 panes without incident after
  all the wiring (can't click the actual context-menu buttons here — no
  input-simulation tool in this sandbox — so the UI half rests on the
  wiring matching the same pattern every other menu action already uses,
  not an interactive click-test). Still uncommitted (commit-timing rule).

  Developer asked to open a design pass on the chrome's visual identity
  — "clean, minimal, modern and slightly technical... not a caricature."
  Surveyed current state first rather than proposing blind: found the
  context menu/settings panel run on `egui::Context::default()` with no
  custom `Visuals` at all — stock egui theme, visually disconnected from
  the terminal grid's own (already reasonably considered) near-black/
  muted-gray palette. Published an Artifact mockup (3 side-by-side
  directions, same layout/content held constant for a fair comparison —
  real menu structure, real code/build-output/`ls` content, not lorem):
  **A — Graphite** (cool slate, slate-blue accent, full mono chrome),
  **B — Warm Signal** (warm neutral, the app's own broadcast-orange
  elevated into the UI accent, native sans chrome), **C — Quiet Signal**
  (true near-black, desaturated sage accent, lighter mono chrome).
  Deliberately single-theme (dark only) — the subject itself is a dark
  terminal by default, evaluating a light mode wasn't what the comparison
  was for.

  Developer's pick: A's palette, B's chrome typography, and — since
  there's now a real accent color in play — make it a user setting rather
  than hard-coding blue. Implemented:
  - `config::Appearance` gained `accent_color` (hex, same convention as
    `background_color`) defaulting to Graphite's `#7fa2d6`; extracted the
    shared hex-format/parse logic both fields now use.
    `background_color`'s own default changed from pure black to
    Graphite's `#0c0e11`.
  - `graphics.rs`'s palette constants (`TITLE_BAR_BG`, `DIVIDER_COLOR`,
    `TEXT_COLOR`, `TITLE_BAR_TEXT_LIGHT/DARK`) updated to Graphite's exact
    values. Cursor and selection highlight now derive from the
    user's accent color (with their existing alpha) instead of a fixed
    constant — they're the "interactive/focus" category the accent exists
    to theme. The broadcast-target border stays a fixed color regardless
    — a distinct semantic signal, not decoration, so it doesn't move with
    the user's own color choice.
  - New `render::system_ui_font_data()` for the chrome's "native sans"
    typography (option B): found a real, general robustness gap while
    building it, not just a sandbox quirk — `cosmic_text::FontSystem`
    hardcodes its generic `SansSerif` mapping to "Open Sans" regardless of
    platform, a Google web font that isn't actually installed by default
    on Windows, macOS, or most Linux distros. Built a real fallback chain
    instead (`Segoe UI`, `Helvetica Neue`, `Ubuntu`, `Cantarell`,
    `Noto Sans`, `DejaVu Sans`, `Liberation Sans`, `Arial`, generic
    `SansSerif` last), so it actually finds *something* on whatever
    platform this runs on — confirmed the "Open Sans"-only version really
    did fail here (no error, just `None`) before fixing it, not assumed.
  - `ui.rs`: installs that system font once at startup (ahead of egui's
    bundled default, in the `Proportional` family only — fonts are the
    one thing here NOT cheap to reapply every frame, unlike colors) and
    applies a new `graphite_visuals(accent_rgb)` unconditionally every
    frame (cheap — just data, no atlas rebuild) covering window/panel
    fill, selection, hyperlink color, and all four widget interaction
    states (noninteractive/inactive/hovered/active). Settings panel
    gained an "Accent color" picker next to the existing background one.
  - Deliberately untouched: corner radius, spacing, layout — the
    developer explicitly deferred those to a later pass; this one is
    colors and chrome typography only.

  Full workspace build/clippy/tests clean (`config` up to 10 — 4 new,
  `render` up to 3 — 1 new), Windows cross-target clean. Smoke-tested a
  fresh config file to confirm the real defaults land correctly
  (`background_color: "#0c0e11", accent_color: "#7fa2d6"`), app launches
  without incident. Still uncommitted (commit-timing rule).

  **Update — same session, continued:** Second design-pass round: corner
  radius, spacing, layout (deliberately deferred from the palette/font
  round above). Built a second Artifact mockup
  (`/tmp/pain-design-layout.html`) showing 2px corner radius, the context
  menu restructured into bordered sections with small-caps muted monospace
  headers, and the settings panel restructured into four grid-aligned
  sections (Appearance/Terminal/Shell/Keybindings). Developer approved it
  with one correction — the Shell section's quick-pick buttons ("Command
  Prompt"/"PowerShell"/"WSL") were bunched up left instead of distributed
  horizontally, fixed in the mockup — then said "Good. Let's do it."

  Implemented for real in `crates/app/src/ui.rs`:
  - `graphite_visuals` now sets `CornerRadius::same(2)` on
    `window_corner_radius`, `menu_corner_radius`, and all four widget-state
    `corner_radius` fields (was previously untouched, defaulting to egui's
    stock 2-6px mix depending on widget/state).
  - Extracted the palette's fixed colors (`PANEL_BG`/`FIELD_BG`/`BORDER`/
    `INK`, plus a new `MUTED`) to module-level `const` values — confirmed
    `egui::Color32::from_rgb` is a `const fn` (vendored `ecolor-0.35.0`
    source) before doing this, so `graphite_visuals` and the new
    `section_header` helper below share one definition instead of each
    computing their own locals.
  - New `section_header(ui, text)` helper: small-caps-style label
    (`.monospace().size(9.5).color(MUTED).extra_letter_spacing(1.0)`,
    manually uppercased) matching the mockup's `.section-header` treatment.
  - Context menu restructured into bordered sections (`ui.separator()`
    between each, already themed to `BORDER` via existing
    `noninteractive.bg_stroke`) with `section_header` labels: Broadcast,
    Split, Arrange all panes, Group, Swap shell. All existing real
    functionality preserved through the restructuring (remove-from-group
    button, existing-group combo box) even though the mockup itself had
    visually simplified some of that away. "Settings..." moved to a plain
    borderless link-style button (`egui::Button::new(...).frame(false)`,
    `MUTED` text color) at the very bottom, per the mockup.
  - Settings panel restructured into four `section_header`-labeled
    `egui::Grid` sections (Appearance: font/size/background/accent;
    Terminal: transparency/scrollback/cursor style; Shell: default shell +
    Windows quick-picks; Keybindings: the existing read-only scroll list),
    two-column grids (`num_columns(2)`, `[12.0, 9.0]` spacing) for aligned
    label/control rows in place of the previous ad-hoc
    `ui.horizontal`/bare-`ui.add` sequence.
  - Both the context menu's Windows-only shell quick-picks and the
    settings panel's quick-pick row now use `ui.columns(3, ...)` with
    `add_sized([available_width(), 0.0], ...)` per button instead of plain
    `ui.horizontal(...)` — the actual fix for "buttons bunched up on the
    left," applied to both rows for consistency even though the developer
    only pointed at the settings panel's row specifically.

  Verified via the vendored `egui-0.35.0` source before writing any of
  this (not assumed): `CornerRadius::same`, `Visuals::window_corner_radius`/
  `menu_corner_radius`, `WidgetVisuals::corner_radius`, `RichText::
  extra_letter_spacing`, `Button::frame`, `Ui::columns`, `Ui::add_sized` all
  confirmed present with the expected signatures. Full workspace build/
  clippy/test clean, Windows cross-target (`x86_64-pc-windows-gnu`)
  build/clippy clean too (the `#[cfg(target_os = "windows")]` quick-pick
  blocks only compile under that target). Smoke-tested a real `cargo run
  -p app` launch under WSL2 — starts and runs without panicking (only the
  known/expected WSL2 llvmpipe/libEGL warnings, see the WSL2 project note)
  — but as with all prior chrome/UI work, could not visually confirm the
  actual on-screen appearance or click-through the menu/settings panel
  myself; this needs the developer's own eyes, same caveat as every prior
  UI milestone. Still uncommitted (commit-timing rule).

  **Update — same session, continued:** Developer reported "this looks
  largely unchanged" after testing on real Windows. Rather than re-guess,
  asked two narrowing questions (what was actually looked at; fresh build
  or stale process) and then asked for a screenshot — got two, of the
  context menu and Settings window, both from native Windows with a fresh
  build. The screenshots actually showed the section-header/divider/
  distributed-button restructuring *had* landed correctly in both places
  (readable proof, not assumption) — so "largely unchanged" wasn't the
  whole story.

  The real, concrete gap the screenshots exposed: the Settings window's
  title bar was still egui's stock near-white bar (further washed toward
  gray-blue by the developer's own 0.85 transparency setting), clashing
  hard against the otherwise-dark Graphite panel below it. Root-caused via
  the vendored `egui-0.35.0` source (`containers/window.rs`'s `title_ui`):
  a window's title bar paints its background from `visuals.widgets.open.
  weak_bg_fill` specifically — a distinct `WidgetVisuals` state from
  noninteractive/inactive/hovered/active, all four of which
  `graphite_visuals` already themed, but `open` was never touched and so
  stayed at egui's own default (`Color32::from_gray(220)`). This was the
  single biggest reason the panel still read as "generic egui," not a
  cosmetic non-issue.

  Fixed in `crates/app/src/ui.rs`: themed `visuals.widgets.open` to match
  the rest (`panel_bg`/`border`/`ink`/`corner_radius`, same as
  noninteractive). Also, while looking at the same screenshot: disabled
  the Settings window's collapse triangle (`.collapsible(false)`) — the
  mockup's panel header is a plain title, not a toggleable section, and
  the collapse arrow was leftover stock window chrome — and renamed the
  settings panel's "Command Prompt" quick-pick button to "cmd" (matches
  the context menu's own naming, and stops it wrapping to two lines while
  its neighbors don't, visible as a real asymmetry in the screenshot).

  Full workspace build/clippy/test clean, Windows cross-target build/
  clippy clean. Still awaiting the developer's re-test with these three
  fixes before considering this pass actually done — the screenshot
  process this round (ask narrowing questions first, then ask for
  evidence, then read the actual image) is worth repeating for any future
  "doesn't look right" report on chrome/UI work, since it found a real bug
  a text description alone wouldn't have surfaced.

  **Update — same session, continued:** Developer sent the actual mockup
  screenshot itself (`/tmp/pain-design-layout.html`, rendered) side by side
  with a description, saying "we're very far off from correct." Comparing
  it directly against the developer's own earlier screenshot of the real
  app surfaced three real, concrete gaps the title-bar fix hadn't touched:

  1. **Broadcast** was a vertical list of `selectable_label` pills; the
     mockup shows a horizontal row of actual radio buttons (one mode
     active at a time, which a radio group communicates directly — a
     list of individually-clickable pills doesn't). Switched to
     `ui.radio(...)` in a `ui.horizontal`.
  2. **Split's and Arrange's buttons** were left-packed at content width
     (`ui.horizontal` + plain `ui.button`) — the exact "bunched up on the
     left" issue the developer had already flagged once for the Shell
     quick-picks, just not caught in these two other spots during the
     first implementation pass. Fixed the same way: `ui.columns(n, ...)`
     + `add_sized([available_width(), 0.0], ...)` per button, matching
     what Shell's quick-picks already did.
  3. **The Settings window was simply too narrow.** Left at egui's
     content-fit default size, every field rendered at its bare intrinsic
     size — a tiny color-swatch button, a narrow drag-value — instead of
     the mockup's wide, generously-spaced field grid. This was likely the
     single biggest driver of "very far off," more than any individual
     widget choice. Fixed: `.default_width(480.0)` on the Settings
     `egui::Window`; added a `color_field(ui, &mut rgb)` helper (swatch +
     live `#rrggbb` hex text side by side, matching the mockup's
     Background/Accent rows) instead of a bare `color_edit_button_rgb`;
     widened the "Default shell" text field to `ui.available_width()`;
     added a `" lines"` suffix to the scrollback `DragValue`. Also
     reverted the "cmd"/"Command Prompt" relabeling from the previous
     round — that was working around the *symptom* (wrapping) of too
     little width, not a real naming problem, and the mockup's actual
     label is "Command Prompt" in full.
  4. Smaller matching fixes while in the same code: "Cursor style" →
     "Cursor", "Font size" → "Size", "Default shell" → "Default" (grid
     labels now match the mockup's shorter wording exactly instead of
     paraphrasing it); Group's "In group: {name}" → "In group **{name}**"
     (no colon, bold name, matching the mockup's phrasing).

  Full workspace build/clippy/test clean, Windows cross-target build/
  clippy clean, smoke-tested a real launch (no panic, same known WSL2
  llvmpipe/libEGL warnings as always). Root cause pattern worth
  remembering: a *rendered* mockup screenshot is a much sharper spec than
  a remembered summary of one — re-reading the actual image surfaced
  concrete widget-choice and sizing gaps that a written recollection of
  "bordered sections with headers" had glossed over. Still awaiting the
  developer's own re-test; still uncommitted (commit-timing rule).

  **Update — same session, continued:** Developer's re-test screenshots
  showed real progress ("Better") but still real gaps ("keep trying").
  This time, instead of comparing screenshot-to-screenshot from memory,
  read the mockup's actual HTML/CSS source (`/tmp/pain-design-layout.html`)
  directly as ground truth — exact pixel values instead of a described
  impression. That surfaced several concrete, previously-missed mismatches:

  1. **Overall type scale.** The mockup's chrome text runs 11.5-12.5px
     body / 9.5px headers — a deliberately dense, "technical" ratio. Only
     the headers had been sized explicitly; everything else (labels,
     buttons) was still at egui's stock defaults (13px body/button,
     18px heading), which reads as noticeably larger/airier than the
     mockup at a glance — very plausibly the single biggest remaining
     "far off" driver. Added `apply_chrome_text_styles()`
     (`ctx.all_styles_mut`, set once at startup like `install_chrome_font`):
     `TextStyle::Body`/`Button`/`Monospace` to 12px, `Heading` to 13px (the
     last one specifically because `egui::Window`'s built-in title uses
     `TextStyle::Heading` — at its stock 18px it dwarfed every 9.5px
     section header sitting right below it; the mockup's own
     `.panel-title` is a modest 12.5px bold label).
  2. **Broadcast's active radio label didn't turn accent-colored** the way
     the mockup's `.radio.active` rule does — `override_text_color`
     forces every label to the same ink color unless a widget's text
     explicitly overrides it (confirmed via `widget_text.rs`'s
     `get_text_color`: an explicit `RichText::color` always wins over
     `override_text_color`, so this was safe to do). Extracted
     `graphite_visuals`'s inline color-conversion closure into a shared
     top-level `color32_from_rgb()` so the broadcast row could compute the
     live accent color too; active label now gets `accent_color32`,
     inactive gets `MUTED`.
  3. **Save/Cancel were left-packed and in the wrong order.** The mockup's
     action row is flush against the panel's right edge with Cancel left
     of Save (`justify-content: flex-end`); the real code just used a
     plain `ui.horizontal` (left-packed, no right alignment at all) with
     Save added before Cancel. Switched to
     `ui.with_layout(egui::Layout::right_to_left(Align::Center), ...)`
     with Save added first (lands rightmost under right-to-left) then
     Cancel — reads left-to-right as "Cancel, Save" flush right, matching
     the mockup exactly.
  4. Smaller matches from re-reading the same source: the swatch+hex-text
     `color_field` helper's hex text now explicitly `MUTED` (was
     defaulting to ink via `override_text_color`; the mockup's
     `.swatch-input` text is a dimmer caption-like tone, not full-strength
     body text) — same explicit-color-wins mechanism as point 2. Context
     menu's `set_min_width` trimmed 320 to 240 to match the mockup's much
     denser information density now that the type scale shrank to match.

  Full workspace build/clippy/test clean, Windows cross-target build/
  clippy clean, smoke-tested a launch (no panic). Process note worth
  keeping for next time a chrome/UI report comes back "still off": go
  straight to the mockup's own source file for exact values (font-size,
  color, flex direction, DOM order) rather than re-deriving intent from a
  screenshot comparison alone — the source has answers a screenshot can
  only hint at (e.g. the Save/Cancel DOM order, the exact 11.5px/9.5px
  ratio). Still awaiting the developer's own re-test; still uncommitted
  (commit-timing rule).

  **Update — same session, continued:** Developer said plainly "I think
  you're in a rut" and sent full-window screenshots (not cropped to the
  chrome) of both surfaces — that framing mattered: seeing the menu and
  Settings window in the context of the whole multi-pane terminal made
  clear both were rendering *much* wider than the mockup, with buttons
  stretched to match, which cropped comparisons hadn't made obvious.
  Stopped patching individual properties and looked for one systemic
  cause instead of another round of one-off diffs.

  Found it by reading egui's own layout source rather than guessing:
  `ui.set_min_width(...)` (used for the context menu) only sets a floor —
  it does not bound `ui.available_width()`, which for content inside an
  `egui::Area` (no declared max rect) can be very large. Every
  `ui.columns(n, ...)` call divides *whatever* `available_width()`
  reports, so those calls were stretching Split/Arrange/Swap-shell's
  buttons across nearly the entire window instead of a compact ~240px
  menu. The Settings window had the same disease from a different
  source: the "Default shell" field's `.desired_width(ui.available_width())`
  (added two rounds ago to fix text wrapping — the real fix for that was
  always a bounded width, not an unbounded one) requested "the rest of
  the window," and `egui::Grid` sizes an entire column to its widest row,
  so that one field call inflated the whole grid, and the whole window
  auto-sized to match.

  Fixed at the source, not per-symptom: `ui.set_width(240.0)` (not
  `set_min_width`) on the context-menu popup, and `ui.set_width(420.0)`
  as the very first line inside the Settings window's content closure —
  both set min *and* max, so nothing downstream can request more no
  matter what it asks `available_width()` for. Removed the
  `desired_width(ui.available_width())` call entirely (a plain
  `TextEdit` already defaults to a sane, bounded 280px via
  `Style::spacing.text_edit_width`, confirmed in `text_edit/builder.rs` —
  the explicit call was actively worse than doing nothing). Also added
  `.resizable(false)` to the Settings window to match the mockup (no
  resize-drag affordance shown there).

  While tracing the width bug, re-read the mockup's shell quick-picks
  markup again and caught a second, related structural mismatch: its
  `.quick-picks` row is a *sibling* of `.field-grid`, spanning the whole
  section's width — not a third row squeezed into the grid's narrow
  value column, which is what the Rust code had it doing. Moved it
  outside the `egui::Grid` entirely to match.

  Two more fixes from the same "stop patching symptoms" pass:
  - `Slider` was rendering as a bare rail + small handle with no visible
    progress at all (not even close to looking like a slider) because
    `Visuals::slider_trailing_fill` defaults to `false`; egui only paints
    a filled portion (using `selection.bg_fill`, already `accent`) when
    that flag is set. One flag, fixes every slider in the app at once.
  - Button padding/item spacing were still egui's stock values (padding
    `vec2(4,1)`, item spacing `vec2(8,3)`) — visibly airier than the
    mockup's tighter `4px 8px` padding and `5-7px` row gaps. Folded into
    the existing one-time `apply_chrome_style` setup (renamed from
    `apply_chrome_text_styles` now that it does more than text sizes):
    `button_padding: vec2(8,3)`, `item_spacing: vec2(6,6)`.

  Full workspace build/clippy/test clean, Windows cross-target build/
  clippy clean, smoke-tested a launch (no panic). The width bug
  specifically is the kind of thing worth remembering as a class, not
  just a one-off: any time chrome is rendering "everything too big/wide"
  rather than "this one thing looks off," the first suspect should be an
  unconstrained container feeding `available_width()`, not the individual
  widgets sitting inside it — chasing individual widget properties one at
  a time (the last two rounds) was treating the symptom, and the
  developer's "I think you're in a rut" was the right call. Still
  awaiting the developer's own re-test; still uncommitted (commit-timing
  rule).

  **Update — same session, continued:** Developer confirmed the width fix
  ("Better") but flagged the natural next symptom of bounding an
  unconstrained container: "you still haven't extended the controls in
  the settings window to fill the space and the labels ... are too
  narrow." Fixing the runaway-width bug removed the overflow but left
  every value-column control sized to its own bare intrinsic content
  (labels shrunk to fit their own text, sliders sat at egui's stock
  100px rail, color swatches were tiny) — the mockup's `108px 1fr` grid
  proportions were never actually being enforced, just no longer being
  blown past.

  Fixed by pinning both sides of that ratio explicitly, now that the
  window's total width is fixed and known (420px, `resizable(false)`):
  - `LABEL_COL_WIDTH = 108.0`, `VALUE_COL_WIDTH = 300.0` (420 minus the
    label column minus the grid's 12px column gap) as named constants,
    matching the mockup's own `108px 1fr` exactly rather than an
    eyeballed guess.
  - New `grid_label(ui, text)` — every settings-grid label now
    `add_sized([LABEL_COL_WIDTH, ...], Label::new(text))` instead of a
    plain `ui.label(text)`, so the label column is pinned to 108px
    everywhere instead of each of the three grids (Appearance/Terminal/
    Shell) auto-sizing independently to its own longest label.
  - `ComboBox::width(VALUE_COL_WIDTH)`, `TextEdit::desired_width
    (VALUE_COL_WIDTH)` — these two already draw their own bordered box,
    so an explicit width is all they needed to fill the column
    (confirmed safe now, unlike two rounds ago, because the *outer*
    container is finally bounded).
  - `style.spacing.slider_width` (a global `Style` default, confirmed via
    source — `Slider` has no per-instance width builder at all) bumped
    from egui's stock 100px to `VALUE_COL_WIDTH - 60.0`, so both sliders
    stretch to fill their column instead of sitting short with empty
    space trailing them.
  - New `field_box(ui, add_contents)` — a themed bordered `Frame` at
    `VALUE_COL_WIDTH`, for the two controls with no such box of their
    own (color swatch+hex, scrollback count), matching the mockup's
    `.swatch-input` styling instead of leaving them as bare small
    widgets adrift in a wide column.
  - Cursor style switched from a plain `ui.horizontal` of
    `selectable_value`s to a stretched 3-column segmented control
    (`ui.columns` + `egui::Button::selectable(...)` via `add_sized`,
    since egui 0.35 has no standalone `SelectableLabel` type — confirmed
    `ui.selectable_label` itself is just `Button::selectable(...).ui(...)`
    internally, in `ui.rs`), matching the mockup's `.segmented` row
    filling its full column width instead of three small buttons huddled
    on the left.

  Full workspace build/clippy/test clean, Windows cross-target build/
  clippy clean, smoke-tested a launch (no panic). Pattern worth carrying
  forward: fixing an overflow bug and fixing "doesn't fill the space
  enough" are two different fixes, not one — bounding a container only
  stops it from being too big; something still has to explicitly ask to
  fill the space that bounding leaves available, or content just
  collapses to its own minimum inside it. Still awaiting the developer's
  own re-test; still uncommitted (commit-timing rule).

  **Update — same session, continued:** Developer: "Great progress" (a
  first clean confirmation for this design pass, no new complaint besides
  the one specific ask below) — labels were centering instead of sitting
  flush left. Root cause was in the `grid_label` helper added this same
  round: `ui.add_sized(...)` — used to pin the label column to a fixed
  108px — internally lays out its contents with
  `Layout::centered_and_justified` (confirmed in `ui.rs`'s own
  `add_sized` source; it's *always* centered, with a code comment there
  even acknowledging this as a known wart), which is a different, more
  restrictive layout than "fixed width, left-aligned." Fixed by switching
  to `ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), ...)`
  instead — same fixed-width cell (so the grid column still pins to
  108px), but without forcing the centered layout `add_sized` bakes in.

  Full workspace build/clippy/test clean, Windows cross-target build/
  clippy clean, smoke-tested a launch (no panic). Still awaiting the
  developer's own re-test; still uncommitted (commit-timing rule).

  **Update — same session, continued:** The label-centering fix from the
  previous update introduced a real regression the developer caught
  immediately: "the controls shifted left. They should be pinned to the
  right and the width of the labels ... should be what it was
  previously." Root cause: `allocate_ui_with_layout` with a plain
  `Layout::left_to_right(Align::Center)` doesn't reserve the full
  requested width the way `add_sized`'s `centered_and_justified` does
  (`main_justify` was `false`) — so `Grid` measured each label cell as
  just its own shrink-wrapped text width again, not the fixed 108px, and
  the whole value column started earlier (further left) than it should
  have. Fixed with the actually-correct `Layout`:
  `left_to_right(Align::Center).with_main_align(Align::Min)
  .with_main_justify(true)` — `main_justify: true` reserves the full
  requested width (like `add_sized`), `main_align: Min` keeps the text
  left-aligned within it (unlike `add_sized`, which is hardcoded to
  `Align::Center` on both axes). This is the actual fix for last round's
  centering complaint; the previous attempt only *looked* like a fix
  because it also happened to break the width pinning in a way that
  wasn't yet visible.

  Developer separately raised a bigger, structural point: "if flex is a
  possibility, you really should be using that. Operating in pixels is a
  sure fire way to make different sized displays look really bad.
  Everything should be relative." Fair criticism of the last few rounds'
  approach (`LABEL_COL_WIDTH = 108.0`, `VALUE_COL_WIDTH = 300.0` as flat
  constants tied to one specific 420px window width). Reworked to be
  genuinely proportional:
  - Re-enabled the Settings window's resizing (dropped `.resizable
    (false)` — a fixed-size window has no real use for proportional
    math, so re-enabling it is what gives "flex" an actual purpose).
  - Replaced the two width constants with `LABEL_COL_FRACTION = 0.26`
    (derived from the mockup's own 108/420 ratio) and a `GRID_COLUMN_GAP`
    constant shared between the column-width math and every `Grid`'s own
    `spacing.x`, so they can't silently drift apart.
  - `content_width = ui.available_width()` read *once*, at the very top
    of the Settings window's own content `Ui` (not nested inside a `Grid`
    cell) — confirmed safe specifically because this is the window's own
    already-settled current width, unlike the earlier runaway bug, which
    came from reading `available_width()` deep inside a `Grid` cell
    before its column width was actually known. `label_width`/
    `value_width` are derived from that once per frame and threaded as
    plain parameters into `grid_label`, `color_field`, `field_box`,
    `ComboBox::width`, and `TextEdit::desired_width` — recomputed
    automatically if the window is resized, instead of frozen at whatever
    one pixel number seemed right for one specific window size.
  - New `slider_field(ui, column_width, slider)`: `Slider` has no
    per-instance width builder at all (confirmed in its own source —
    only a global `Style::spacing.slider_width` default), so this scopes
    a local style override via `ui.scope` to set that default just for
    one call, sized to the actual column instead of a single shared
    constant every slider in the app would otherwise have to agree on.

  Full workspace build/clippy/test clean, Windows cross-target build/
  clippy clean, smoke-tested a launch (no panic). Worth remembering: the
  developer's "operating in pixels... everything should be relative"
  point applies specifically to values representing a *fraction of a
  container* (a label column's share of the window) — it doesn't mean
  every pixel value in the UI is suspect; the context menu's fixed 240px
  width was left alone, since a popup menu sized to its own content
  (like real desktop context menus) isn't the same kind of value as a
  panel's internal column split. Still awaiting the developer's own
  re-test; still uncommitted (commit-timing rule).

  **Update — same session, continued:** Developer: "Thank you. Looks
  good. Next." — the design pass is done (no further complaints), moving
  on. Two quick polish items plus the last three functional items on the
  list: tab completion (broken in every shell), a new terminal-area
  right-click menu (copy/paste), and restricting the existing pane menu
  to title-bar right-clicks only. Used `TodoWrite` to track all five
  (the first genuinely multi-part, partly-unexplored task list this
  design-pass thread has had), then an `Explore` agent to map the exact
  call chains before touching code, rather than guessing at three
  separate, unfamiliar subsystems (keyboard routing, mouse hit-testing,
  clipboard) at once.

  **Polish (quick):**
  - Settings' "Settings..." link never visibly reacted to hover — root
    cause was hardcoding its `RichText` color to `MUTED`, which always
    wins over the widget-state-driven hover color the rest of the menu's
    buttons get for free. Simplest real fix, and the developer's own
    suggestion: stop trying to fake a frameless "link," just make it a
    plain `ui.button(...)` like every other item in the menu.
  - Re-disabled the Settings window's resizing (re-enabled two updates
    ago specifically to give the proportional label/value-column math
    somewhere to flex) — developer wants it fixed again; the fraction-
    based sizing computed from `ui.available_width()` stays regardless,
    since it's still more robust than a hardcoded pixel pair even in a
    non-resizable window (font metrics/DPI can still shift the window's
    actual rendered width slightly from `default_width`'s nominal value).

  **Tab completion, root-caused via the `Explore` agent reading
  `egui-winit`'s actual source, not guessed:** `key_bytes()`
  (`crates/app/src/main.rs`) already correctly mapped
  `NamedKey::Tab → b"\t"` — that code was never the problem, and was
  never reached. `egui_winit::State::on_window_event` hardcodes
  `consumed = true` for *every* Tab keypress unconditionally (comment in
  its own source: "When pressing the Tab key, egui focuses the first
  focusable element, hence Tab always consumes"), regardless of whether
  anything in our own overlay is even open or focusable. `main.rs`'s
  `if !ui_consumed` guard then skipped `key_bytes` entirely, every time,
  in every shell — a GUI-layer bug with nothing shell-specific about it.

  Fixed by overriding the app's own interpretation of that flag, not by
  fighting egui-winit itself: added `Ui::wants_keyboard_focus()`
  (true while the pane menu, terminal menu, or settings panel is open)
  and a `Graphics::ui_wants_keyboard_focus()` wrapper; `main.rs` now
  un-consumes a Tab keypress specifically when nothing in the overlay
  actually has focus to cycle, so Tab reaches `key_bytes`/the pty as
  normal passthrough — but still behaves as ordinary GUI focus-cycling
  while a menu/panel with its own text fields is genuinely open (typing
  in the "New group name" field, say).

  **Terminal-area context menu + title-bar-only pane menu**, also mapped
  via the `Explore` agent before writing anything:
  - `layout::geometry` only ever produced one full rect per pane — no
    existing title-bar/content split as a hit-testable region (only as a
    *drawing* split, via `Graphics::content_rect`/`title_bar_height`,
    already used for PTY sizing and glyph placement). Added
    `Graphics::pane_title_bar_at(pos)`, built the same way
    `content_rect` already carves the title bar off the top of a pane's
    full rect — reusing `title_bar_height` rather than a second hardcoded
    constant.
  - `Graphics::open_context_menu_at` now hit-tests the title bar
    specifically (via the new method) and returns whether it found one;
    `main.rs`'s right-click handler falls back to a new
    `open_terminal_context_menu_at` (full pane rect, i.e. "anywhere else
    in the pane") when it didn't. Net effect: title bar → pane-management
    menu, terminal content → the new copy/paste menu.
  - `ui.rs`: added a second, independent `terminal_context_menu` field
    alongside the existing `context_menu`, with `open_terminal_context_menu`
    / `open_context_menu` each explicitly clearing the other so the two
    can never both be open at once (a real bug in an earlier draft of this
    change, caught before it shipped: opening one without clearing the
    other left both rendering simultaneously if a second right-click
    landed in a different region without an intervening dismiss).
    `close_context_menu` now closes whichever is open. New `UiRequest`
    fields `copy_selection`/`paste_clipboard: Option<PaneId>`.
  - `Graphics::copy_selection(pane)` reuses the exact clipboard-write
    `end_selection` already did automatically on drag-release (extracted
    into a shared `copy_to_clipboard` helper, confirmed via `Explore` that
    no other clipboard code existed anywhere in the codebase before this) —
    the difference is it's callable on demand for whatever selection is
    still sitting there highlighted, not just the instant a drag ends.
    New `Graphics::paste_into_pane(pane)` is the first paste capability
    this project has had at all: reads `arboard::Clipboard::get_text()`
    and writes the bytes straight to the pane's PTY via the same
    `write_input` real keystrokes use — a plain paste, deliberately not
    bracketed-paste-escaped (matches most simple terminal emulators'
    default behavior, not iTerm2/kitty's opt-in safer mode; noted as a
    scope choice, not an oversight, in case it's ever revisited).

  Full workspace build/clippy/test clean (32 app-crate tests, unchanged —
  none of this had existing test coverage to extend, per the `Explore`
  agent's finding that `main.rs` has no `#[cfg(test)]` module at all),
  Windows cross-target build/clippy clean, smoke-tested a launch (no
  panic). Interactive verification (does Tab actually complete a real
  command in bash/PowerShell/cmd, does right-clicking a title bar vs. the
  terminal body actually show the right menu, does Paste actually write
  clipboard text into a real shell) all still needs the developer's own
  hands — same caveat as every other interactive feature this whole
  project. Still uncommitted (commit-timing rule).

  **Update — same session, continued:** Developer asked how to build a
  Linux binary for Debian-based distros to distribute for testing. First
  pass was informational only (no code changes) — built a real release
  binary in this sandbox and inspected it directly rather than describing
  winit/wgpu's Linux needs from memory: `ldd` showed only
  libc/libm/libgcc_s linked (X11/Wayland/Vulkan are all loaded via
  `dlopen` by winit/wgpu/arboard at runtime, not linked), so `strings`-ing
  the binary for the `.so` names it actually opens, then `dpkg -S` on each
  found file, gave the real runtime package list — a normal
  `ldd`/`dpkg-shlibdeps`-based approach would have completely missed all
  of them. Also confirmed a real, concrete glibc constraint: this sandbox
  is Ubuntu 22.04 (glibc 2.35), so a binary built here needs glibc ≥ 2.35
  (fine on Debian 12+/Ubuntu 22.04+, not Debian 11/Ubuntu 20.04) — flagged
  as something to address later with an older build container if broader
  compatibility is ever needed; not done in this pass since no Docker
  daemon is reachable from this sandbox to verify a container build
  directly (checked — client installed, daemon unreachable).

  Developer's follow-up: "I just want a dpkg ... work through the
  dependencies ... the obvious thing. I need others to test this." Before
  building one, flagged a real blocker rather than guessing: the project
  has never actually settled on a name (README still said "name TBD";
  `config::APP_NAME` was already flagged provisional in an earlier
  session) — the built crate was literally named `app`, unfit to ship as
  a public `/usr/bin/` binary and a `.deb` package name, and a bad name
  is expensive to walk back once handed to real testers. Asked via
  `AskUserQuestion`; developer picked "pain" (the recommended option —
  matches the repo directory and the config dir path already baked into
  the code).

  Implemented, in order:
  - Renamed the `app` crate to `pain` in `crates/app/Cargo.toml`
    (`[package] name`, plus a real `description` field cargo-deb also
    reads) — confirmed via `cargo tree`/grep first that nothing else in
    the workspace depends on the package by name, so this was a safe,
    contained rename. Updated `README.md`'s `cargo run -p app` references
    to `-p pain` (its own `.waypoint/memory/` log entries are historical
    and were deliberately left alone, same convention as always).
  - Added `[profile.release] strip = true` to the workspace `Cargo.toml`
    — cuts the distributed binary from ~28MB to ~21MB for free, no reason
    to ship debug symbols to testers who aren't debugging against this
    source tree.
  - `cargo install cargo-deb` (v3.7.0) — the standard tool for this,
    not hand-rolled `dpkg-deb` control-file construction.
  - Added `[package.metadata.deb]` to `crates/app/Cargo.toml`: maintainer/
    copyright from the developer's own git/email identity, `license-file`
    pointing at the workspace `LICENSE`, and — the part that actually
    needed the earlier investigation — `depends = "$auto, <the 11
    dlopen'd libraries found and verified in the informational pass>"`.
    `$auto` is cargo-deb's own `dpkg-shlibdeps`-equivalent detection,
    which (as expected, given `ldd` already showed this) only ever found
    `libc6 (>= 2.35)` on its own — confirming independently, via a
    different tool, the same glibc-2.35 finding from the informational
    pass. `mesa-vulkan-drivers` added as `recommends`, not `depends`: a
    minimal Debian install might not have any Vulkan ICD, but this binary
    can apparently still start without one going by this project's own
    WSL2 dev-loop history (software `lavapipe` fallback), so making it a
    hard dependency would be too strong a claim to make without directly
    testing a no-ICD environment.
  - Ran `cargo deb -p pain`. Verified the actual output, not just that
    the command exited 0: `dpkg-deb -I` confirmed the control file's
    `Depends` line came out correctly de-duplicated/combined (`libc6 (>=
    2.35), libegl1, libvulkan1, libwayland-client0, libwayland-egl1,
    libx11-6, libx11-xcb1, libxcb1, libxcursor1, libxi6,
    libxkbcommon-x11-0, libxkbcommon0`), `dpkg-deb -c` confirmed the file
    layout (`/usr/bin/pain`, `/usr/share/doc/pain/{README.md,copyright}`).
    `lintian` wasn't available in this sandbox to lint it further.
  - Extracted the `.deb` with `dpkg-deb -x` (not a real system-wide
    `dpkg -i`, to avoid installing a new package into the shared dev
    sandbox without cause) and ran the extracted `usr/bin/pain` directly —
    confirmed it launches cleanly, same benign WSL2 llvmpipe/libEGL
    warnings as every other smoke test this session, no crash.

  Full workspace build/clippy/test clean after the rename (Windows
  cross-target build/clippy clean too — `-p pain` now, `-p app` no longer
  exists). Package at
  `target/debian/pain_0.1.0-1_amd64.deb` (~5.7MB compressed). Still
  uncommitted (commit-timing rule) — this is a good candidate for the
  project's actual first commit once the developer has tested it, given
  how much has accumulated uncommitted across this entire session.

  **Known limitation to flag if testers hit install failures:** this
  `.deb` requires glibc ≥ 2.35 (built on Ubuntu 22.04 in this dev
  sandbox) — it won't install/run on Debian 11 or Ubuntu 20.04 and older.
  If that turns out to matter, rebuild inside an older base image (e.g.
  `debian:bullseye-slim` via Docker, glibc 2.31) for broader coverage —
  not done this round since no Docker daemon was reachable here to verify
  it directly, only described as the next step.

  **Update — same session, continued:** One of the developer's testers
  reported font size 13 looking fine everywhere else on their machine but
  small in this app specifically — their Windows display scaling is set
  to 125%. Asked first (per the "exploratory question" convention —
  brief recommendation, don't implement until agreed) whether the app
  could honor a "system default font size"; investigated and reported
  back that no such OS-level concept actually exists cross-platform
  (GNOME's `monospace-font-name` gsettings key is the closest thing, and
  it's GNOME-only), so recommended not chasing it. The tester's report
  reframed the real question entirely — not "match some system default
  size" but "honor DPI scaling at all," which turned out to be a genuine,
  confirmed bug: `settings.appearance.font_size` was used as a literal
  physical-pixel count everywhere it fed into cell measurement and glyph
  rasterization (`crates/app/src/graphics.rs`, three call sites — the
  initial `measure_cell` in `Graphics::new`, the one in `apply_settings`'s
  font-size-changed branch, and the actual per-frame `self.grid.render`
  call), with no multiplication by `window.scale_factor()` anywhere. The
  egui chrome already got this right, from the earlier context-menu-
  position DPI fix (`egui_winit::pixels_per_point`) — the terminal grid
  itself just never got the same treatment. At 125% scaling, every other
  DPI-aware app renders its "13pt-equivalent" text at ~16 physical
  pixels; this app rendered exactly 13 regardless of the OS setting,
  which is exactly the reported symptom.

  Fixed by introducing `scaled_font_size(font_size, scale_factor)` — a
  small free function multiplying the user-facing "points" value (what's
  saved to config/session, what the Settings slider edits) by the
  window's current DPI scale factor — used consistently at all three
  call sites. "Consistently" mattered specifically here: `self.cell`
  (used for layout/PTY row-col sizing) and the actual rasterized glyph
  size have to agree exactly, or it's the same class of bug as
  Milestone 1's very first fix (the hardcoded `CELL_WIDTH` mismatch that
  caused glyph bleed) — noted directly in the new function's doc comment
  so a future call site doesn't reach for the raw config value out of
  habit. Also added `Graphics::rescale()` (recomputes `self.cell` for
  the window's current scale factor, then reruns
  `resize_panes_to_geometry`) wired to a new `WindowEvent::
  ScaleFactorChanged` handler in `main.rs` — confirmed via the actual
  winit 0.30.13 source that this event fires when a window is dragged to
  a monitor with a different scaling setting (not just at startup), so
  without this handler the fix would only apply once, at launch, and go
  stale on any multi-monitor mixed-DPI setup.

  Full workspace build/clippy/test clean, Windows cross-target build/
  clippy clean, smoke-tested a launch (no panic). Could not visually
  verify the actual fix myself — this WSL2 sandbox reports a 1.0 scale
  factor, so the change is a no-op here by construction; needs the
  tester's real 125%-scaled machine (or the developer's own) to confirm
  text now renders at the expected size. Still uncommitted (commit-timing
  rule).

  **Update — same session, continued:** Three more items of feedback from
  test users, batched together: (1) a close button on every pane's title
  bar and on both right-click menus; (2) closing a pane in an arranged
  row only resized its immediate structural neighbor ("like flexbox"
  requested instead); (3) Settings should live-preview color/
  transparency/font, reverting on cancel. Used `TodoWrite` to track all
  three plus verification — the biggest multi-part implementation task
  this session, spanning `layout`, `graphics.rs`, `ui.rs`, and `main.rs`.

  **(2) Pane-close rebalancing — the deepest piece, investigated via an
  `Explore` agent before writing anything.** Confirmed byte-for-byte via
  the actual code (not assumption) that `Layout::close`'s sibling-
  promotion is a deliberate design choice documented in `.waypoint/design/
  layout-tree.md` ("no ratio recalculation needed elsewhere in the tree")
  — and traced a concrete 3-pane example by hand through the real
  `close_node` logic to confirm exactly the reported symptom: closing the
  middle of three equal horizontal panes left the left pane at its
  original 1/3 and grew the right one to 2/3, because only the *immediate
  parent* split of the closed pane ever gets touched; every ancestor
  split above that point keeps its old ratio untouched, unconditionally.
  Confirmed no existing concept of "the row/column of sibling panes
  containing X" exists anywhere in the crate (the closest thing,
  `resize_target`, only finds one single nearest ancestor, not a full
  chain) — this needed new logic, not a wiring fix.

  Implemented in `crates/layout/src/lib.rs`: `Layout::close` now (a)
  finds `pane`'s immediate parent orientation, (b) walks from the true
  root down to find the *outermost* split of that same orientation
  containing `pane` (the top of its whole visual row/column, however
  many levels of nested same-orientation splits deep it goes) — both
  read-only, done *before* any mutation, (c) does the existing sibling-
  promotion unchanged, then (d) if that outer split still exists
  afterward (it won't, for a simple 2-pane row — nothing to rebalance
  there, correctly a no-op), re-walks it assigning every same-orientation
  split's ratio so each surviving leaf/opaque-subtree slot gets an *equal*
  share — a deliberate scope choice over exactly preserving each
  survivor's prior relative proportion (true flexbox-style flex-grow
  redistribution): equal-share is simpler to implement correctly, matches
  the common case exactly (the existing `Arrangement`/"Arrange all panes"
  feature already produces equal panes, so arrange-then-close — probably
  the most common real path to this bug — behaves identically either
  way), and is very likely what "like flexbox" meant colloquially. A
  split of a *different* orientation nested inside the row (e.g. a
  vertical stack occupying one horizontal slot) is treated as one opaque
  slot and keeps its own internal ratio completely untouched — only the
  row/column that actually lost a pane rebalances.

  4 new unit tests (middle-of-three, rightmost-of-three, one-of-four, and
  a mixed-orientation case confirming a nested differently-oriented stack
  survives untouched) — the last one caught a mistake in the test's own
  math on first write (assumed a horizontal split halves height, which
  it doesn't — Horizontal only ever divides width), not a bug in the
  implementation; fixed the assertion, not the code, after tracing the
  actual geometry by hand. All 3 pre-existing `close`-related tests
  (`close_promotes_sibling`, `close_last_pane_fails`,
  `closing_the_zoomed_pane_clears_zoom`) still pass unchanged — none of
  them asserted on post-close ratios/rects at all, so there was nothing
  pinning the old "far sibling swells" behavior in the first place.
  Layout crate: 21 → 25 tests.

  **(1) Close button + menu items.** New `Graphics::close_button_rect`
  (shared by drawing and hit-testing, so they can't drift apart) reserves
  one title-bar cell on the right for a plain `×` glyph — drawn through
  the exact same monospace `GlyphCell` pipeline the title text already
  uses, no separate icon system needed. Title centering math now excludes
  that reserved column so a long title can't render underneath the
  button. `Graphics::close_button_at(pos)` hit-tests it; checked *first*
  in `main.rs`'s left-press handler, before even a divider-drag attempt,
  since the button is drawn on top of everything else in the title bar.
  Made the previously-private `close_pane` method `pub` rather than
  adding a new wrapper. The button's click path exits the app immediately
  on closing the last pane (mirrors the `Ctrl+Shift+W` chord's own
  handling, since it's also processed outside `redraw` entirely) — but a
  menu-driven close only becomes known once `self.ui.show()` returns
  *inside* `redraw`, too late to exit immediately, so that path threads a
  `quit_after_present` flag through to `redraw`'s own return value
  instead (same convention `redraw` already uses for an auto-closed
  exited pane). Added a "Pane" section (with "Close") to the pane-
  management menu and a "Close" button to the terminal content menu,
  both via a new `UiRequest.close_pane: Option<PaneId>`.

  **(3) Live settings preview.** New `Ui::live_preview(&self, base)`
  returns `SettingsDraft::apply_to(base)` (already existed, previously
  only called on Save) whenever the panel is open — read at the very top
  of `redraw`, *before* this frame's own grid render (the grid draws
  before `self.ui.show()` runs each frame, so the draft's current values
  have to be known ahead of that call, not just after). Applied through
  the existing `apply_settings` (the same path a hot-reloaded config file
  already goes through), reusing its font-size/font-family-changed
  detection and `resize_panes_to_geometry` call for free. New `Graphics.
  saved_settings: config::Config` field (initialized alongside `settings`
  at load) tracks the last *durably-saved* state, distinct from
  `settings` (which now also reflects in-progress, unsaved edits live);
  `UiRequest` gained `settings_saved: Option<config::Config>` (Save,
  updates both `settings` and `saved_settings`) and `settings_cancelled:
  bool` (Cancel, or the settings window's own close button — reverts
  `settings` back to `saved_settings` via `apply_settings` again). Noted
  as a known, accepted minor edge case: `saved_settings` is only ever
  updated by this app's own Save button, not by an external hot-reloaded
  file edit that happens to land while the panel is open — Cancel in that
  narrow window would revert past a legitimate external change. Not
  fixed; judged too rare to be worth the added complexity of threading
  "was this apply_settings call a live preview or a real reload" through
  a shared code path.

  Full workspace build/clippy/test clean (layout: 25 tests; app-crate
  test count unchanged — none of this had prior coverage to extend, and
  none of it is unit-testable without real mouse/keyboard drive), Windows
  cross-target build/clippy clean, smoke-tested a launch (no panic). This
  is by far the largest chunk of *interactive* surface added in one pass
  this session (click-driven close, two menus' worth of new buttons, and
  a live-preview render path) — all of it needs the developer's own
  hands-on test pass before trusting it; the sandbox here has no
  mouse/keyboard-drive capability to verify any of the three beyond
  "compiles and doesn't crash on launch," same limitation as every other
  interactive feature this whole project. Still uncommitted (commit-
  timing rule).

  **Update — same session, continued:** Developer asked to review the
  close button's padding — wanted it uniformly distant from the title
  bar's top, right, and bottom edges. Investigated rather than guessing:
  the button's box (`Graphics::close_button_rect`) already used
  `TITLE_BAR_PADDING` symmetrically on all three sides in raw pixel
  terms, so the *margins* were already numerically equal — the actual
  problem was the box's own *shape*. It was built from `cell.0` (a
  glyph's advance width) by `cell.1` (line height, `render::measure_cell`:
  `font_size_px * 1.25`) — for a typical monospace font these are wildly
  different magnitudes (line height commonly 2x+ a glyph's advance
  width), so the button was a tall, narrow sliver, not a square. The same
  4px padding value looked "uniform" only as raw numbers, not in how the
  button read visually next to a symbol centered inside such a
  disproportionate shape.

  Fixed by making the button an actual square: side length `cell.1` (so
  top/bottom margins are unchanged), width *also* `cell.1` instead of
  `cell.0` (so the right margin, measured from this new wider box, is
  still exactly `TITLE_BAR_PADDING` — unchanged in raw pixels, but now
  measured against a box shaped consistently with the vertical margins
  too). The close glyph itself is now explicitly horizontally centered
  within that wider box (`close_button.x + (close_button.width -
  cell.0) / 2.0`) rather than left-aligned at the box's edge the way an
  ordinary monospace character in a text row is — needed specifically
  because the box is now wider than one glyph's natural advance width.
  `close_button_at`'s hit-test picks up the new (larger, centered) shape
  automatically, since it already shares `close_button_rect` with
  drawing — nothing to keep in sync by hand there.

  Full workspace build/clippy/test clean, Windows cross-target build/
  clippy clean, smoke-tested a launch (no panic). Could not visually
  confirm the actual on-screen result myself — same sandbox limitation
  as the rest of this button's own feature — needs the developer's own
  eyes. Still uncommitted (commit-timing rule).

  **Update — same session, continued:** First release-engineering work.
  Developer has set up a GitHub remote but hasn't pushed anything yet,
  and will handle all git-remote-side operations (push/tag) personally —
  asked me only to build the release pipeline itself. Wants: release
  builds for Windows/macOS/Linux, x86_64 and arm64 both; short release
  notes + build artifacts on GitHub Releases, like typical application
  repos; and (a separate, later phase) a GitHub-Pages-hosted APT
  repository for `apt install`/`apt upgrade`.

  This was a genuine "what do you think?" design question, not an
  implement-immediately request — gave a short recommendation up front
  rather than a full plan: cross-compiling a GPU/native-dependency app
  from one host isn't realistic, so native builds on GitHub-hosted
  per-OS runners is the right shape; arm64 Windows/Linux runners are
  noticeably less mature than the four "easy" targets (macOS arm64 is
  actually trivial — `macos-latest` runners are Apple Silicon already);
  and the APT-over-Pages piece is a genuinely separate project (persistent
  GPG signing key, `reprepro`/`aptly`-style repo structure) that
  shouldn't be bolted onto the same pass as the release-build pipeline.
  Developer agreed, said explicitly to note-but-skip arm64 Windows/Linux
  for now, and proceed.

  Built `.github/workflows/release.yml`: a 4-target build matrix
  (`ubuntu-latest`/x86_64-unknown-linux-gnu, `windows-latest`/
  x86_64-pc-windows-msvc, `macos-latest`/x86_64-apple-darwin *and*
  aarch64-apple-darwin — both macOS targets explicitly specified rather
  than relying on whatever the runner's own host architecture happens to
  default to, since that's exactly the kind of assumption GitHub changing
  runner hardware could silently break later) triggered on `v*` tags,
  plus a `workflow_dispatch` trigger that runs the identical build matrix
  without publishing anything — a safe way to test the pipeline itself
  (e.g. after editing the YAML) without cutting a real tag every time.

  Verified pieces of this for real rather than trusting recall, given
  none of it can be exercised by actually triggering the workflow from
  here (no way to push a tag, and the developer explicitly owns that
  side):
  - `cargo deb`'s exact interaction with `--target`: ran it for real with
    `--no-build` against a binary that had been built *without* an
    explicit `--target` — it failed looking for
    `target/x86_64-unknown-linux-gnu/release/pain`, proving `--target`
    changes cargo-deb's *expected* binary location even though the
    `[package.metadata.deb] assets` path in `crates/app/Cargo.toml` is
    still the literal string `"target/release/pain"` (cargo-deb rewrites
    that pattern per-target internally). Re-ran *without* `--no-build`
    (letting cargo-deb build it itself) and confirmed it works end-to-end
    — this is what the workflow actually does, not the broken two-step
    version.
  - YAML validity: parsed clean with `python3 -c 'import yaml'` (after
    quoting `"on":` — bare `on:` parses as the boolean `True` under
    YAML 1.1's keyword rules via PyYAML's default loader; doesn't
    actually break GitHub's own parser, but quoting it removes the
    ambiguity for any other tool that reads this file).
  - Semantic validity: installed `actionlint` fresh via `go install
    github.com/rhysd/actionlint/cmd/actionlint@latest` (this sandbox
    doesn't have passwordless `sudo`, so `shellcheck` — which
    `actionlint` also shells out to for embedded script linting —
    couldn't be installed alongside it) and ran it directly against the
    new workflow file: zero findings.

  Known limitations, stated plainly rather than glossed over:
  - Unsigned binaries — macOS Gatekeeper and Windows SmartScreen will
    both warn on these until/unless the developer sets up paid signing
    certificates (Apple Developer Program, a Windows code-signing CA);
    not attempted, no certs available, and not asked for.
  - Cargo.toml's `[workspace.package] version` and CHANGELOG.md's section
    heading aren't automatically kept in sync with the git tag — noted
    in the workflow file's own header comment as a manual step (bump the
    version, rename `## Unreleased` to `## vX.Y.Z` and add a fresh empty
    `## Unreleased` above it, "Keep a Changelog" style) before tagging,
    not enforced by CI.
  - This cannot be end-to-end verified without an actual tag push, which
    is the developer's own responsibility per their own stated split of
    labor — everything above is "as verified as possible without
    triggering a real run," not "confirmed working."

  Not yet done, by design (phased per the developer's own agreement):
  arm64 Windows/Linux build targets, and the GitHub-Pages APT repository.
  Nothing committed — new file only, developer manages all git-remote
  operations themselves.

  **Update — same session, continued:** Developer wants the README
  cleaned up before pushing and testing the first release — trimmed,
  clear, concise, with development-era testing/debug content removed.
  Rewrote `README.md` wholesale:
  - Title changed from "Terminal Emulator (name TBD)" to **pain** (the
    project's actual name, decided a few updates ago in this same log —
    the README had never been updated to reflect it).
  - Dropped the "Status: early development. Nothing works yet" line
    entirely — badly stale given everything actually built this session;
    replaced with a real **Features** list (splits/grouping/broadcast,
    session persistence, automatic cwd tracking, colored output/
    scrollback/copy-paste, live-preview settings, both right-click menus)
    and a pointer to `CHANGELOG.md` for detailed history instead of a
    hand-maintained status line that would just go stale again.
  - Removed the `.waypoint/` pointer — that's this project's own
    internal session-memory/process scaffolding, not something an
    outside reader needs directed at from a public README.
  - Removed the Windows D3D12-debug-layer/`Graphics Tools`-optional-
    feature paragraph entirely — exactly the kind of "debugging this
    specific wgpu quirk while developing" content the developer asked to
    have trimmed; not useful to an end user or most contributors.
  - Added **Installing** (points at GitHub Releases and the `.deb`,
    anticipating the release pipeline just built) and **Building from
    source**/**Usage** sections in place of the old single "Development"
    section — kept the `vendor/wgpu-hal` patch note (a real, still-
    relevant build fact, not debug cruft) and a brief, verified-against-
    the-actual-code mention of `-v`/`--verbose`/`--verbose=<category>`
    (checked `crates/app/src/main.rs`'s actual flag-parsing and
    `verbose::Category`'s real variant names rather than describing it
    from memory).
  - Kept unchanged: the "Why" pitch (still accurate, not debug-related),
    the tech-stack table, and the License section.

  No app code touched — docs only, so no build/test/clippy pass needed
  beyond confirming every relative link (`CHANGELOG.md`, `LICENSE`,
  `vendor/README.md`) actually resolves to a real file, which it does.
  Nothing committed — developer manages all git-remote operations
  themselves, and said they'll push and test the first release next.

  **Update — same session, continued:** Following up on the earlier
  "Cargo.toml version and CHANGELOG heading aren't auto-synced" question
  (answered as an exploratory "what do you think" — recommended a local
  pre-tag script or CI auto-commit, flagged the tradeoffs, didn't build
  anything yet). Developer's actual ask this round: turn that into a
  proper Waypoint skill — Waypoint should be able to prompt that a bump
  is likely due (or the developer can just ask for it), it analyzes
  what's changed since the last version and decides the semver bump, and
  the very first version is fixed at v1.0.0.

  Discovered this project already has its own established convention for
  exactly this — `.waypoint/skills/*.md`, referenced by
  `.waypoint/opord.md` §3a ("check `.waypoint/skills/` for a relevant
  skill document... following them is not optional") and governed by its
  own meta-skill, `.waypoint/skills/new-skill.md`. Read that plus a
  concrete example (`debug.md`) to match the established format exactly,
  rather than inventing a new one or reaching for Claude Code's own
  separate `.claude/skills/` plugin-skill mechanism (checked — no
  `.claude/skills/` exists anywhere for this project, and the Waypoint
  convention is the right fit here since it's specifically about
  encoding *this project's* recurring procedures, not a general-purpose
  Claude Code capability).

  Wrote `.waypoint/skills/version-bump.md`: baseline = highest existing
  `v*` git tag, or (no tag yet) a fixed first release of v1.0.0, no
  judgment call needed. Once a baseline exists, classifies `git log
  <baseline>..HEAD` and `CHANGELOG.md`'s `Unreleased` section against a
  semver mapping adapted for an *application* rather than a published
  library — "breaking" means breaking for a user (incompatible config
  format, removed feature, changed default, dropped platform), not an
  API consumer — takes the highest tier triggered by anything in the
  batch, applies it to `Cargo.toml`'s version and renames `## Unreleased`
  to `## v<version>` (with a fresh empty `## Unreleased` above it,
  matching `.github/workflows/release.yml`'s own release-notes-extraction
  convention already documented in that file). Explicitly never commits,
  tags, or pushes — same standing split of labor as the release
  workflow itself (developer owns all git-remote operations).

  For the proactive-prompt half ("Waypoint can prompt us"), added two
  sentences to `.waypoint/opord.md` §3a's existing pre-action checklist
  — right after the existing "check `.waypoint/skills/`" instruction —
  rather than inventing a separate mechanism: once a `v*` tag exists,
  check whether `CHANGELOG.md`'s `Unreleased` section looks substantial
  and mention it once, briefly, not as a recurring nag. This piggybacks
  on the *already-mandatory* every-session read of that checklist
  (`.claude/rules/waypoint.md` requires it at the start of every
  session) rather than needing a new, separate trigger mechanism.

  Then actually ran the skill's first invocation (no `v*` tag exists
  yet, confirmed via `git tag -l 'v*'` — empty), rather than leaving it
  purely theoretical: bumped `Cargo.toml`'s `[workspace.package] version`
  0.1.0 → 1.0.0 and renamed `CHANGELOG.md`'s `## Unreleased` to
  `## v1.0.0` with a fresh empty `## Unreleased` above it. This surfaced
  a real, would-have-broken-the-build gotcha the skill document itself
  doesn't call out explicitly (worth remembering, possibly worth adding
  to the skill later): every in-workspace path dependency
  (`crates/app`, `crates/router`, `crates/session`'s `Cargo.toml`s) also
  pins its own `version = "0.1.0"` *requirement* string alongside the
  `path = "../..."` — Cargo enforces that requirement even for path
  deps, so bumping only the workspace version broke the build outright
  (`failed to select a version for the requirement 'layout = "^0.1.0"'`)
  until those six/seven internal requirement strings were bumped to
  match. Fixed by `sed`-replacing `{ version = "0.1.0", path` →
  `{ version = "1.0.0", path` across the three affected `Cargo.toml`
  files.

  Full workspace build/clippy/test clean (all 3 crate test suites,
  unaffected), Windows cross-target build/clippy clean, `cargo deb`
  re-verified end-to-end at the new version (produces
  `pain_1.0.0-1_amd64.deb` correctly), smoke-tested a launch (no panic).
  Nothing committed — same as everything else this whole session; the
  developer said they're about to push and test the first release next,
  and now has the version/changelog side already prepared for that.

  **Update — same session, continued:** Developer confirmed the first
  release "worked without issue" — v1.0.0 built and published across all
  four platforms via the tag push. This repo now has real history: one
  commit (`5b6a5bc "initial"`) and the v1.0.0 tag (pushed through
  whatever path the developer used to tag it — not present as a local
  tag in this sandbox's clone, which just means it went out via GitHub's
  UI/`gh`/a different checkout rather than a local `git tag` in this
  exact working directory; nothing to reconcile).

  Developer then relaxed the standing "never commit" rule, but only in
  one specific, explicitly scoped way: the `version-bump` skill may now
  commit and tag on its own when invoked — still never push (that
  remains the developer's own deliberate action, since it's what
  actually makes a release public and triggers real CI). Also asked for
  commit messages generally to be clear, extremely concise, plainest
  language — updated three places to match:
  - `.waypoint/skills/version-bump.md`: rewrote the "Purpose" paragraph
    and renamed the old "Step 4 — Report, don't commit" to "Step 4 —
    Commit and tag" (stage exactly the edited files, commit with a
    one-line plain message — `Bump version to <version>`, nothing more —
    tag with `git tag -a v<version> -m "v<version>"`, never push). Also
    folded in a real lesson from the first live run that the skill
    document itself hadn't captured: every in-workspace path dependency
    (`crates/app`/`router`/`session`'s own `Cargo.toml`s) pins its own
    `version = "..."` requirement string that Cargo enforces even for
    path deps — the skill now explicitly calls this out as a step, not
    just something to rediscover the hard way again next time. Added a
    `cargo build --workspace` sanity check before committing.
  - `.waypoint/opord.md` §3d (Ongoing Duties): added a "Commit messages"
    bullet — one line, imperative mood, plainest language, with a
    concrete good/bad example pair (`Bump version to 1.0.0` vs. `Prepare
    release configuration updates`). This is the durable, project-scoped
    place for the convention (read every session via the existing
    mandatory OPORD checklist), so no separate cross-session auto-memory
    entry was needed for it — would just be a duplicate of what's now in
    a project instruction file, which the memory system's own guidance
    says not to do.
  - `.github/workflows/release.yml`'s header comment: updated to
    reference `version-bump.md` as what now actually performs the
    version/changelog sync (previously the comment implied it was purely
    a manual human step every time).

  Re-ran `actionlint` against the workflow after editing its comment
  (comment-only change, but cheap to re-verify): clean.

  Nothing new committed this round either — only the three doc/skill/
  workflow-comment files touched, all still sitting uncommitted for the
  developer's own review, consistent with every other change this whole
  session.

  **Update — same session, continued:** Developer clarified the
  authorization further — they actually want the skill to push too:
  bump, commit, push to main, and *optionally* release by also pushing
  the tag, with the version auto-calculated as already designed. The one
  firm requirement: the skill must present the proposed version number
  and commit message and get explicit confirmation *before* taking any
  of those actions, not after.

  Rewrote `.waypoint/skills/version-bump.md`'s process around that
  confirmation gate rather than bolting it on:
  - Steps 1-3 (find baseline, analyze changes, decide version + draft
    commit message) are now explicitly documented as **read-only** —
    nothing touched yet.
  - New **Step 4 — Confirm before doing anything**: presents the exact
    version and commit message, then asks (via `AskUserQuestion` or
    plain chat) which of three things to do — bump+commit+push to main
    only; the same plus tag+push the tag (cuts the actual release); or
    stop to adjust first. Nothing past this point runs without an
    explicit answer, and a developer-supplied version/message overrides
    the proposed one.
  - Step 5 (apply edits) now also checks the current branch is actually
    `main` before proceeding — a safety check that wasn't needed before
    when the skill never touched git remote state at all.
  - Step 6 (commit + push to main) and Step 7 (tag + push the tag, only
    if the developer picked that option in Step 4) replace the old
    single "commit and tag, never push" step. Step 6 also explicitly
    calls for stopping and asking rather than force-pushing if `git
    push` is ever rejected for having diverged from the remote.
  - Grounded the whole confirm-first design in the OPORD's own existing
    §3c standing rule (pause before publishing/sending to an external
    service) rather than treating it as a new, bespoke policy — pushing
    to main and pushing a release tag both clearly qualify, so the
    skill is just applying a rule that already existed.

  Updated `.github/workflows/release.yml`'s header comment again to
  match (it briefly said "never push" from the previous round; now
  correctly describes the skill as doing the whole thing, gated on
  confirmation each time). Re-ran `actionlint`: clean.

  No new version bump performed this round — v1.0.0 is already out and
  nothing has changed since. Nothing committed; only the skill/OPORD/
  workflow-comment files touched, same as every other doc-only change
  this session.

  **Update — same session, continued:** Moved on to the APT-over-Pages
  phase (the third, deliberately-deferred piece from the original
  release-engineering "what do you think" conversation). Before writing
  any workflow YAML, verified the entire signing/packaging sequence for
  real in this sandbox: generated a disposable throwaway GPG key,
  `dpkg-scanpackages --multiversion` + `apt-ftparchive release` against
  the actual `pain_1.0.0-1_amd64.deb` already built earlier this
  session, signed both a detached `Release.gpg` and inline `InRelease`,
  then ran a genuine `apt-get update`/`apt-cache show` in a fully
  sandboxed apt config (`-o Dir::Etc::...` overrides, no system state
  touched) — resolved cleanly with real `signed-by` trust, no
  `[trusted=yes]` escape hatch needed. Chose flat-repo layout +
  `dpkg-scanpackages`/`apt-ftparchive` over `reprepro`/`aptly` (this is
  one package, one architecture — those tools earn their complexity
  with many); a persistent `gh-pages` branch over GitHub's newer
  artifact-based Pages deploy (the repo needs to *accumulate* `.deb`s
  across releases, which the ephemeral-artifact model doesn't support).

  Had the developer generate a dedicated RSA-4096 sign-only GPG key on
  their own machine (deliberately not through this session — private
  key material should never pass through a shared sandbox/transcript)
  and store it as the `APT_SIGNING_KEY` repo secret. Built a new
  `apt-repo` job in `.github/workflows/release.yml`: downloads the Linux
  build's `.deb`, imports the key, checks out `gh-pages`, adds the `.deb`
  to a growing `pool/`, regenerates and signs the index, publishes a
  small `index.html` with the actual install instructions, and pushes.
  Gated the same way as `release` (real tag pushes only, not a plain
  `workflow_dispatch` dry run) with a `publish_apt_repo` manual-dispatch
  input as the deliberate exception for backfilling. Updated README
  (matching the developer's own since-revised, terser style — didn't
  fight or revert their edits) and added a CHANGELOG entry.

  **First real run failed** on the "initialize gh-pages if it doesn't
  exist" fallback: `error: remote origin already exists`. Root cause,
  confirmed rather than patched around blindly: `actions/checkout@v4`
  doesn't fail cleanly when the given `ref` doesn't exist — it
  partially initializes the target directory (`git init` + the `origin`
  remote) *before* the ref-resolution step itself fails, so the
  fallback step's own unconditional `git remote add origin` collided
  with a remote `actions/checkout` had already created. The fallback's
  own premise (`checkout` failing means nothing was created) was wrong.

  Developer's read: this is over-engineered for what should be a
  one-time bootstrap, and asked to just create the branch directly
  instead of hardening the fallback further. Agreed — bootstrapped
  `gh-pages` for real from this sandbox via `git worktree add --detach`
  (kept the primary `main` checkout completely undisturbed) + `git
  checkout --orphan gh-pages` + an empty placeholder (`.nojekyll`,
  a plain "nothing published yet" `index.html`), committed, pushed,
  removed the worktree. Confirmed via `git ls-remote --heads origin`
  that `gh-pages` now exists remotely. Then deleted the whole fragile
  try/fallback dance from the workflow — `gh-pages` is guaranteed to
  exist from now on, so the job just does a plain `actions/checkout@v4`
  with `ref: gh-pages`, nothing conditional. Simpler, and the actual bug
  class (a checkout step's partial side effects on failure) can't recur
  since there's no failure path left to trigger it.

  Re-verified with `actionlint` after each edit: clean throughout.
  Committed and pushed both rounds (the original apt-repo job + README/
  CHANGELOG, then this simplification) — explicitly asked/re-confirmed
  each time given this project's now-standing "confirm before push"
  practice, not assumed as blanket ongoing authorization. Developer is
  about to re-run the release workflow (via `workflow_dispatch` with
  `publish_apt_repo` checked) to actually backfill v1.0.0's `.deb` into
  the now-real `gh-pages` branch, then enable GitHub Pages on it
  (Settings → Pages → source: `gh-pages`) — both still pending.

  **Update — same session, continued:** Developer pointed out the
  `publish_apt_repo` manual-dispatch input was now dead weight — with
  `gh-pages` bootstrapped directly (previous update), every future tag
  push already triggers `apt-repo` automatically, so the one-off
  backfill escape hatch had nothing left to do. Removed it entirely
  (the `workflow_dispatch.inputs` block, the `|| github.event.inputs...`
  half of `apt-repo`'s `if:`, and the header comments describing it) —
  `apt-repo` is now gated exactly like `release`: real tag pushes only.
  Confirmed via `gh run list` that GitHub Pages itself was already
  enabled on `gh-pages` (a "pages build and deployment" system run had
  already completed successfully) — one fewer manual step than expected.

  Then ran the actual `version-bump` skill for the second time, exactly
  per its own documented process: baseline = `v1.0.0` (existing tag);
  `git log v1.0.0..HEAD` showed only the APT-repo work (a new, additive,
  user-facing capability — install via `apt` instead of only a manual
  `.deb`) plus two CI-internal fixes with no app-facing effect — highest
  tier triggered is **minor**, so `v1.1.0`. Presented the exact version
  and commit message and stopped for confirmation (`AskUserQuestion`,
  per the skill's Step 4) before touching anything, per the developer's
  own explicit design from two updates ago. Confirmed: bump, commit,
  push to main, tag, and push the tag — all four together.

  Executed Steps 5-7 precisely: bumped `Cargo.toml` and the three
  internal path-dependency requirement strings (`crates/app`/`router`/
  `session` — the exact gotcha the skill document now calls out from
  last time, didn't get missed this round), renamed the CHANGELOG
  heading, confirmed `cargo build --workspace` clean, committed
  (`Bump version to 1.1.0`, bundled with the workflow cleanup from the
  same task rather than as a separate commit — genuinely one unit of
  work), pushed to main, tagged `v1.1.0`, pushed the tag.

  **Full end-to-end verification against the real, live, production
  result** — not just green checkmarks in the Actions UI:
  - `gh run list`: all 6 jobs succeeded (4 platform builds, `release`,
    `apt-repo` — the latter for the first time ever).
  - `gh api .../releases/tags/v1.1.0 -q .body`: real CHANGELOG content,
    263 bytes — confirms the release-notes extraction bug fix actually
    holds up on a second, independent release, not just the one-off
    manual edit applied to v1.0.0 after the fact.
  - Fetched the real public URLs directly (`curl`, not just trusting the
    workflow logs): `https://w-p.github.io/pain/`, `/pain-archive-
    keyring.asc`, `/Packages`, `/InRelease` all 200.
  - Ran a **real `apt-get update` + `apt-cache show`** against the
    actual production repo (sandboxed `-o Dir::Etc::...` overrides, own
    scratch keyring fetched fresh from the live URL) — resolved `pain
    1.1.0-1` correctly with full GPG trust, no `[trusted=yes]` escape
    hatch. The whole pipeline this session designed, built, debugged,
    and now shipped is confirmed genuinely working end to end, from a
    cold client's perspective, not just from the publishing side.

  This is the first time a full release cycle (version-bump skill →
  tag push → all three jobs → live artifacts + live APT repo) has run
  without any manual intervention or follow-up fix. Nothing pending;
  both v1.0.0 and v1.1.0 are real, correct, live releases.

  **Update — same session, continued:** Developer asked what's needed to
  polish up the installs, starting with an icon. Audited first: no icon
  assets anywhere, no `.desktop` entry, no runtime window icon, and the
  window title was still "Terminal Emulator (dev)".

  Flagged that the `.desktop` entry actually matters *more* than the icon
  for a terminal emulator specifically — without one, an `apt install`ed
  terminal never appears in the applications menu, so the only way to
  launch it is to type its name into some *other* terminal. A real
  chicken-and-egg first-run problem, not just cosmetics.

  Designed the icon from the app's own existing "Graphite" palette rather
  than inventing something unrelated: a rounded dark tile (#14171b on
  #262b31) showing the pane tree the app is actually about — one bright
  focused pane beside a dimmer split stack — plus an accent-colored
  (#7fa2d6) cursor block so it reads as a terminal rather than a generic
  window-layout glyph. Rendered two treatments (uniform panes vs.
  focused-bright/siblings-dim) at 256/48/32/16 and *looked at them* before
  choosing; the focused-bright variant both carries more meaning and
  survives downscaling better.

  Two real bugs caught by verifying rather than assuming:
  1. ImageMagick's SVG renderer silently ignores the `opacity` attribute,
     so the first draft rasterized far brighter than the source described
     — the SVG and PNGs would have disagreed on any SVG-aware icon theme.
     Fixed by baking the tones in as literal hex fills (documented in the
     SVG itself so nobody reintroduces `opacity` later).
  2. ImageMagick emits **16-bit** PNGs by default. `window_icon()`
     deliberately requires 8-bit RGBA and returns `None` on mismatch (a
     missing icon shouldn't block launching a terminal) — so this would
     have shipped with *no window icon at all*, silently, with nothing in
     the logs. Regenerated everything with `-depth 8 PNG32:` and added
     `embedded_window_icon_actually_decodes` as a regression test, since
     the failure mode is invisible by design.

  Added: `assets/pain.svg` + 8 PNG sizes; `assets/pain.desktop`
  (validated field-by-field against the spec — `Categories`/`Keywords`
  semicolon-terminated, `TerminalEmulator` category, `Terminal=false`
  which correctly means "don't run me *inside* another terminal");
  `[package.metadata.deb] assets` entries installing all of it to the
  standard `usr/share/applications` + `hicolor` theme paths; `png 0.18.1`
  as a direct dependency (already in the tree transitively, so it costs
  nothing, and it keeps the PNG the single source of truth instead of a
  second opaque raw-pixel blob); runtime `with_window_icon`; and an
  explicit X11/Wayland app-id matching `StartupWMClass=pain` — winit
  would otherwise derive it from `argv[0]`, which is right today but
  breaks the desktop-entry/icon association through a symlink or rename.

  Verified: workspace build/clippy clean, Windows cross-target check
  clean, 33 app-crate tests pass, `dpkg-deb -c` confirms every file lands
  at its correct standard path, smoke-tested a launch with no icon errors
  or panics. Left uncommitted — the icon is a taste call the developer
  should see before it ships.

  Still open for a later pass (not started, deliberately): Windows `.exe`
  embedded icon (needs a build script + `winresource`), a macOS `.app`
  bundle (currently shipping a bare binary in a tarball, which macOS
  users won't recognize as an app), and code signing/notarization for
  both — which needs paid certificates, so it's the developer's call.

  **Update — same session, continued:** Built the macOS `.app` bundle
  (developer: "go for it", icon refinement deferred).

  Chose a **universal** binary over per-architecture builds: the matrix
  already compiled both `x86_64-apple-darwin` and `aarch64-apple-darwin`,
  so a new `macos-bundle` job `lipo`s them into one `pain.app` — Mac users
  get a single download that works everywhere instead of having to know
  which CPU they have. This restructured the artifact flow: the two macOS
  matrix entries now upload *raw binaries* named `macos-raw-*` (deliberately
  not `pain-*`), `macos-bundle` consumes those and produces
  `pain-macos-universal`, and the `release` job switched from
  "download everything" to `pattern: pain-*` so the intermediates never
  get attached to a release as bogus downloads of their own.

  Added `assets/macos/Info.plist` as a reviewable file in the repo rather
  than a heredoc buried in YAML, with `__VERSION__` substituted at build
  time. Notable keys: `NSHighResolutionCapable` (without it macOS runs the
  app through its 1x scaler and every glyph in the terminal grid renders
  blurry on Retina — the single most important key here for a text-heavy
  app), `NSSupportsAutomaticGraphicsSwitching`, and a
  `developer-tools` category. `.icns` is generated in CI via Apple's own
  `iconutil` from a `.iconset` — the only way to get a `.icns` Apple's
  tooling considers correct, which is also why the job has to run on a
  macOS runner (`lipo` and `iconutil` are both Apple-only).

  Verified as much as is possible without a Mac:
  - `Info.plist` parses via Python's `plistlib`, both as committed and
    after `__VERSION__` substitution.
  - Version derivation logic run for real against `v1.2.0`,
    `v2.0.0-rc1`, `main`, `feature/icons` — tag runs yield the version,
    non-tag `workflow_dispatch` runs correctly fall back to `0.0.0`
    rather than writing a branch name into the bundle.
  - Every `assets/pain-*.png` the workflow references exists, and each
    `.iconset` entry was checked against the actual pixel dimensions of
    its source, including the @2x doubling rule (`icon_512x512@2x.png`
    must be 1024px, etc.) — a mapping typo would otherwise only surface
    in CI. Generated `assets/pain-1024.png` for that last slot.
  - The job also self-checks *inside* CI before shipping: `lipo -archs`
    must contain both x86_64 and arm64, `plutil -lint` must accept the
    plist, the substituted version must read back correctly, and the
    `.icns` must be non-empty.
  - `actionlint` clean; workspace build/clippy/tests clean.

  Documented the Gatekeeper workaround in the README
  (`xattr -dr com.apple.quarantine`) — unsigned apps are blocked on first
  launch, and users will hit this immediately.

  Still open: Windows `.exe` embedded icon (needs a build script +
  `winresource`), and code signing/notarization for macOS and Windows,
  which needs paid certificates — a spend decision, not a code one.
  Everything this round is uncommitted, pending the developer's review.

  **Update — same session, continued:** Cut v1.2.0 (minor — icon/desktop
  entry and the macOS bundle are additive, nothing breaking). Split into
  two commits per the developer's choice: the 19-file feature commit,
  then a version-only bump commit. Pre-ran the release-notes extraction
  locally against the renamed `## v1.2.0` heading before pushing (559
  bytes of real content) so the v1.0.0 empty-notes failure couldn't
  recur silently.

  All 7 jobs green, including `macos-bundle`'s first-ever execution.
  Verified the actual shipped bundle rather than trusting the run's
  checkmarks — downloaded `pain-macos-universal.tar.gz` and inspected it
  on Linux (no `lipo` available, so parsed the Mach-O fat header by hand
  in Python): `FAT_MAGIC` with 2 architectures, x86_64 (15.5 MB) and
  arm64 (14.4 MB) both present; `Info.plist` has `__VERSION__` correctly
  substituted to `1.2.0` and `NSHighResolutionCapable` set; `.icns`
  starts with the `icns` magic. Release assets are exactly the four
  intended downloads — the `macos-raw-*` intermediates correctly did
  *not* leak in, confirming the `pattern: pain-*` filter works.

  **Then the developer switched topics entirely** — asked what rendering
  capabilities exist for shader effects (CRT/scanlines/glow/glitch) and
  a procedural animated background. Read the full pipeline before
  answering rather than speculating. Findings, for future reference:
  - The renderer draws **straight to the swapchain** in a single pass
    (`LoadOp::Clear` then one instanced draw of all glyphs + solid
    rects). There is no intermediate texture, so **no post-processing
    hook exists today**.
  - `Globals` has exactly one field, `screen_size` — **no time uniform**,
    so nothing can animate without adding one.
  - The event loop already redraws continuously (`ControlFlow::Poll` +
    unconditional `request_redraw` in `about_to_wait`), vsync-capped.
  - Everything is premultiplied alpha, a hard constraint from Windows
    DirectComposition.

  Assessment given: post-processing effects need an offscreen render
  target + fullscreen pass (self-contained refactor, visually a no-op on
  its own — the natural first step); cheap effects (scanlines, barrel
  warp, chromatic aberration, vignette, glitch) are a few shader
  instructions once that exists, while real bloom needs a 3-5 pass
  downsample/blur chain. The **animated background is the shorter path**
  — it needs no offscreen target at all, just a fullscreen pass before
  the grid draw plus the time uniform. Flagged three real caveats:
  egui chrome should render *after* effects (a warped settings panel is
  unusable); legibility matters more than looks in an app people read
  for hours, so effects want to be opt-in and tunable; and an animated
  background makes the currently-temporary continuous-redraw loop
  permanent by design, which is real battery cost on a laptop. Awaiting
  the developer's direction on which to pursue and what the background
  should actually depict.

  **Update — same session, continued:** Developer skipped the rendering
  effects ("fluff our core user base doesn't need"), then asked what
  killer features were missing. Rather than brainstorm, audited the code
  and CONOPS first. Reported that the real gaps were **table stakes, not
  exotic features** — and that one was closer to a defect: paste wrote
  clipboard text straight to the PTY, so any multi-line paste executed
  every line immediately. Developer agreed on all of it; named layouts
  (the one genuine differentiator I proposed) they judged niche, and we
  parked it. Search was "nice if easy" — deferred, it's a bigger lift
  (needs UI, match highlighting, navigation).

  Implemented, all verified with tests:

  - **`crates/app/src/paste.rs`** (new, 7 tests): bracketed-paste
    encoding + the risk rule. Wraps pastes in `ESC[200~`/`ESC[201~` when
    the program set `TermMode::BRACKETED_PASTE`. Notably it **strips any
    end marker embedded in the pasted text** — without that, content
    containing a literal `ESC[201~` terminates the bracket early and
    everything after it arrives as typed input, turning a paste of
    attacker-influenced text into command execution. There's a test for
    exactly that. Confirmation rule: prompt only when *not* bracketed and
    the text has a newline that isn't the single trailing one — a lone
    trailing newline is the everyday "copy a command and run it" case,
    and prompting on it would just train people to dismiss the dialog.
  - **`Screen::wants_bracketed_paste()`** in the pane crate rather than
    reaching into `alacritty_terminal` from the app crate (which doesn't
    depend on it directly).
  - **Confirmation modal** in `ui.rs`: shows a summary plus a scrollable
    read-only view of exactly what will be sent, with no close button —
    only explicit Paste/Cancel, so a stray click can't silently drop a
    paste. Holds the text itself rather than re-reading the clipboard on
    confirm, so what's sent is what was shown.
  - **`Action::Copy`/`Action::Paste`** bound to `Ctrl+Shift+C`/`V`
    (remappable via config as `copy`/`paste`). Previously there was **no
    keyboard paste at all** — only the right-click menu.
  - **Word/line selection**: `SelectionKind` in the pane crate mapping to
    `alacritty_terminal`'s existing `Semantic`/`Lines` types (already in
    the dependency, previously unused — we only ever used `Simple`).
    Click counting derived in `main.rs` since winit doesn't report it;
    extracted as a free function `next_click_count` over just the
    tracking field because taking `&mut self` conflicted with the live
    `&mut self.graphics` borrow — which also made the cycling rule
    testable (3 tests: cycles 1/2/3/1, times out, resets on distance).
  - **`crates/app/src/url.rs`** (new, 9 tests): conservative URL
    detection — explicit schemes only, no bare domains, and `file:`
    deliberately excluded since terminals print paths constantly and
    one-click-opening a local file is far easier to trigger by accident.
    Column indices are **characters not bytes** (there's a test with a
    multi-byte prefix — getting this wrong would make links unclickable
    on any line with non-ASCII output). Wired to **Ctrl+click**, not
    plain click, since plain click already means "select".

  One test failure along the way was **my test being wrong, not the
  code** — I asserted column 20 of `"x https://example.com y"` was
  outside the URL when it's actually its last character. Fixed the
  assertion and made it exhaustive (before/first/last/after) rather than
  loosening it.

  Also removed three now-dead `start_selection` wrappers superseded by
  `start_selection_of`, rather than silencing the dead-code warnings.

  Verification: workspace build/clippy clean, **52 app-crate tests**
  (up from 33), Windows cross-target check+clippy clean, smoke launch no
  panics. Uncommitted. Not yet exercised by hand — the paste
  confirmation modal, Ctrl+click link opening, and double/triple-click
  selection all need real interactive testing, which this sandbox can't
  do.

  **Update — 2026-07-26:** Bug report — transparency doesn't work on
  macOS (developer unsure whether M1 or M2). Root-caused by reading the
  vendored `wgpu-hal` Metal backend rather than guessing, and the chip is
  irrelevant: **it affects every Mac, Intel and Apple Silicon alike.**

  `metal/adapter.rs` advertises exactly `[Opaque, PostMultiplied]` — the
  Metal backend *never* offers `PreMultiplied`, and `Graphics::new` only
  enabled transparency when `PreMultiplied` was present. So on macOS the
  check always failed, `alpha_mode` stayed `Opaque`, and
  `render_layer.setOpaque(true)` made the window fully opaque. The
  existing "transparency unavailable" message was verbose-only, so
  nothing surfaced unless someone ran with `--verbose`.

  Confirmed the other half was already correct: winit's macOS window
  creation does `setOpaque(false)` + `NSColor::clearColor()` when
  `with_transparent(true)` is requested, which this app already does on
  every non-WSL platform. The surface alpha mode was the sole blocker.

  Fix: extracted `preferred_alpha_mode(available, is_wsl,
  allow_post_multiplied)` as a pure, testable function. Prefers
  `PreMultiplied` everywhere (what `shader.wgsl` emits, and the only mode
  DirectComposition accepts on Windows), falling back to
  `PostMultiplied` **only on macOS**.

  The macOS scoping is the important judgement, not an arbitrary cfg: on
  Metal, `PostMultiplied` is a misnomer — wgpu-hal's *entire*
  implementation of it is `setOpaque(false)`, with no format or blend
  change (verified in `metal/surface.rs`), after which Core Animation
  composites by its own convention, which is premultiplied. So our
  existing premultiplied shader output is already right there. On Vulkan
  the same enum means what it says (the compositor multiplies by alpha),
  so accepting it generally would double-darken every translucent pixel
  on Linux setups that advertise it. Staying opaque is the better failure
  mode than visibly wrong colors.

  5 tests cover each path (premultiplied preferred; macOS falls back;
  other backends stay opaque on the same advertised set; WSL stays opaque
  even when premultiplied is offered; opaque-only stays opaque).
  Workspace build/clippy/test clean (57 app-crate tests), Windows
  cross-target clippy clean, smoke launch fine with the WSL path
  unchanged.

  **Unverified and needs the Mac tester:** whether transparency now
  actually appears, and whether the *colors* are right. If translucent
  text/background looks washed out or too bright as transparency
  increases, that would mean Core Animation wants straight alpha after
  all and the shader needs a per-platform output path — but the reasoning
  above says premultiplied should be correct. Uncommitted, alongside the
  paste-safety/selection/URL work from the previous session.

  **Update — same session, continued:** v1.3.0 shipped clean (all 7 jobs
  green), then the developer tested the paste dialog on real hardware and
  reported two things.

  **1. Huge dead space above the dialog's buttons.** Checked the two
  suspects rather than guessing: `ScrollArea`'s `auto_shrink` defaults to
  `TRUE` (so it wasn't over-reserving), which left
  `with_layout(right_to_left(Align::Center))` as the cause — it claims
  all remaining vertical space of an auto-sizing window and centres the
  buttons within it, so `min_rect` spans from the region's top through
  the buttons. Fixed with a new `action_row` helper that allocates an
  explicit one-row-high region via `allocate_ui_with_layout` (the same
  technique `grid_label` already uses for its fixed-width cells).
  **The settings panel had the identical latent bug** — visible in an
  earlier screenshot this session, never reported — so it was fixed in
  the same pass rather than left to be re-reported later.

  **2. No hover feedback for Ctrl+click links.** Fair — the affordance
  was invisible until you clicked. Added:
  - `url::match_at_column` returning the whole `Match` (not just the URL
    text) so callers know which columns to underline.
  - `Graphics::hovered_url: Option<UrlHover>` plus `update_url_hover`,
    which returns whether the highlight *changed* so the caller only
    forces a redraw when something needs repainting.
  - Recomputed on `ModifiersChanged` as well as `CursorMoved` —
    otherwise pressing or releasing Ctrl without moving the mouse
    wouldn't light up (or clear) the link under it, which is exactly the
    gesture people use.
  - An accent-colored underline drawn under the URL's own columns in
    `redraw`, and `CursorIcon::Pointer` while hovering — deliberately
    taking priority over the divider resize cursor, since a click there
    acts on the link.

  Removed the now-dead `url::at_column` (superseded by
  `match_at_column`) rather than leaving unused public API; its tests now
  exercise `match_at_column` through a small test-only helper. Caught one
  self-inflicted slip in the process: a `sed` left a stray `#[test]` on
  that helper, which `cargo build` happily ignored because it doesn't
  compile test code — only `cargo test` would have failed. Worth
  remembering that `cargo build` passing says nothing about test-code
  validity.

  Workspace build/clippy/test clean (57 app-crate tests), Windows
  cross-target clippy clean, smoke launch no panics. Uncommitted — this
  is post-v1.3.0 work awaiting the next release.

  **Update — same session, continued:** Developer confirmed the hover
  highlighting and dialog spacing both tested good, and asked to wrap up
  and release. Cut **v1.4.0** (minor — URL hover is an additive
  capability; the spacing fix is a bug fix).

  Three commits, and this time the split fell cleanly along file
  boundaries (hover entirely in `graphics.rs`/`main.rs`/`url.rs`, spacing
  entirely in `ui.rs`) so no hunk surgery was needed — but the spacing
  fix was still held aside and commit 1's tree verified to build and pass
  the full suite standalone, same discipline as the v1.3.0 split.

  All 7 jobs green. Verified the published result rather than trusting
  checkmarks: correct four assets attached, notes 325 bytes of real
  content, and `curl`'d the live APT `Packages` file — it lists
  **1.1.0, 1.2.0, 1.3.0, and 1.4.0** side by side, confirming the pool
  genuinely accumulates across releases. That was the whole reason for
  choosing a persistent `gh-pages` branch over artifact-based Pages
  deployment, and it's now demonstrably working, not just intended.

  Also brought `.waypoint/project.md`'s status header current — it was
  badly stale (still described "pain" as a placeholder for an undecided
  product name and the project as mid-milestone). It now records: all v1
  milestones done, v1.0.0-v1.4.0 shipped, the name settled, the full
  automated distribution story, per-platform verification status
  (including that macOS now has a real tester and the transparency fix
  was confirmed there), and the deferred list (scrollback search, named
  layouts, arm64 Windows/Linux, shader effects, WSL cwd tracking).

  **Update — 2026-07-26:** Developer asked what code signing would cost.
  Searched for current figures rather than answering from training data
  (~6 months stale on commercial pricing, and this space has moved):
  Apple Developer Program $99/yr with notarization included — fully
  clears Gatekeeper; Windows is messier — Azure Artifact Signing
  (renamed from Trusted Signing) ~$120/yr and CI-friendly but with
  geographic eligibility limits, traditional OV $215-400/yr but
  **doesn't** immediately clear SmartScreen (reputation accrues with
  download volume), EV $280-685/yr for immediate reputation. Also noted
  the post-2023 FIPS hardware-key requirement and the March 2026 drop to
  ~460-day max validity.

  **Developer declined — not worth it for now.** Recorded in
  `project.md` as a settled decision rather than a lingering open
  question, so future sessions don't re-raise it. Revisit only if asked
  or if unsigned warnings start demonstrably costing adoption.

  **Update — 2026-07-26:** Mac tester reported "permission denied after
  running xattr", said it worked in v1.0.0. **Not a bug.** Asked for the
  exact commands rather than guessing (right call — my first two
  hypotheses were both wrong).

  He ran `./pain.app`. A `.app` is a *bundle*, i.e. a directory — you
  can't execute it. zsh words that refusal as "permission denied"; bash
  says "Is a directory" (verified locally). It "works fine in
  /Applications" because there he double-clicks it instead. v1.0.0
  genuinely worked differently: it shipped a **bare binary**, so `./pain`
  was correct — the `.app` arrived in v1.2.0 and we never documented the
  change in how you launch it.

  Ruled out first, by inspecting the actual shipped artifacts rather
  than theorizing: the exec bit *is* set (`-rwxr-xr-x` on
  `Contents/MacOS/pain`), and the arm64 slice *does* carry
  `LC_CODE_SIGNATURE` — so `lipo` had not stripped the linker's ad-hoc
  signature, killing my initial hypothesis. Both were wrong guesses;
  the exact command text is what actually solved it.

  Fixed the real gap (documentation): README now explains that `.app` is
  a directory, gives `open pain.app` and
  `./pain.app/Contents/MacOS/pain` (the latter for seeing log output),
  and notes why `xattr -dr` needs `-r`.

  Separately found and fixed a genuine defect while investigating: the
  bundle had **no seal** — no `Contents/_CodeSignature/CodeResources` —
  because we never `codesign`'d it as a bundle, only the binary inside
  carried the linker's signature. Added `codesign --force --sign -`
  (ad-hoc, free, no certificate — orthogonal to the paid-signing
  decision already declined) after the bundle contents are final and
  before archiving, plus CI assertions that the seal exists and
  `codesign --verify --strict` passes.

  **Update — 2026-07-26:** Mac tester: Backspace not working in zsh.
  Developer asked whether shell handling could differ ("seems unlikely").
  It was a real and significant bug: **nothing ever set `TERM`.** Neither
  this code nor `portable-pty` sets it, so the child shell inherited
  whatever the GUI process had.

  That explains the whole shape of the report. Launched from an existing
  terminal, `TERM` is inherited and everything looks fine by accident —
  which is how every prior test run happened. Launched from a desktop
  launcher (Finder/Dock, or a Linux `.desktop` entry) there is usually
  **no `TERM` at all**, and zsh responds by disabling/crippling ZLE, its
  line editor — which surfaces as ordinary keys like Backspace doing
  nothing. Not macOS-specific and not really zsh-specific either: it
  would hit a Linux applications-menu launch identically; zsh is just
  less forgiving of an unidentified terminal than bash.

  Fix: `Pty::set_terminal_env` sets `TERM=xterm-256color` and
  `COLORTERM=truecolor` at spawn. Chose `xterm-256color` over a bespoke
  `pain` terminfo entry deliberately — a custom `TERM` only works where
  its terminfo is installed, so it breaks the moment a user SSHes
  somewhere that's never heard of this app.

  Test-design note worth remembering: the first version spawned a real
  `sh` and read `$TERM` back, and **failed for a reason that was the
  test's fault** — the loop's exit condition matched the pty's *echo of
  the typed command*, which contained the marker text. Fixed that, then
  replaced the whole approach: proving the value wasn't merely inherited
  required clearing `TERM` from the test process, and mutating
  process-global env while the suite runs in parallel threads is a worse
  hazard than the coverage was worth. Now asserts against
  `CommandBuilder::get_env` directly — no spawn, no global state.

  Workspace build/clippy/test clean (36 pane tests), Windows cross-target
  clippy clean. Uncommitted alongside the `.app` ad-hoc signing and the
  README launch-instructions fix.

  **Update — 2026-07-26:** Shipped **v1.4.1** (patch — all fixes). Two
  commits at the developer's preference (grouped fixes, then the bump)
  rather than the four I proposed.

  All 7 jobs green, including `macos-bundle` running `codesign` for the
  first time. Verified the published artifact rather than the
  checkmarks: `Contents/_CodeSignature/CodeResources` **is now present**
  (absent in v1.4.0 — that was the defect), and both slices survive
  signing intact (x86_64 15.7 MB, arm64 14.4 MB, each carrying
  `LC_CODE_SIGNATURE`). Release has the right four assets, notes at 980
  bytes, and the APT pool is up to 5 versions with 1.4.1-1 newest.

  Verification note: the first architecture check printed nonsense
  (`0x3`, `0x0`) and briefly looked like signing had corrupted the
  binary. **It was my parsing script, not the artifact** — I
  mis-destructured `fat_arch` (cputype, cpusubtype, offset, size, align)
  by one field, having written it correctly in an earlier session and
  wrong this time. Re-ran it properly before reporting anything. Worth
  remembering: when a verification script disagrees with a build that
  passed its own in-CI assertions, suspect the script first.

  **Update — 2026-07-26:** Developer reported the GPU spinning up "like a
  game" on Windows, and pushed back on my initial staging ("this is a
  terminal and should be super lightweight") — fairly, since I'd framed
  the CPU-sleep fix as optional polish. Did all three properly.

  **Three compounding causes, all confirmed by reading source:**
  1. `Surface::get_default_config` picks `present_modes.first()`, and
     wgpu-hal's DX12 backend advertises `[Mailbox, Fifo]` — so **Windows
     silently ran Mailbox**: present-newest-frame, no vsync throttle, GPU
     rendering flat out forever. Metal lists `[Fifo, Immediate]` and was
     never affected, which is exactly why only Windows was reported.
  2. `about_to_wait` called `request_redraw()` unconditionally every
     iteration, so the full instance buffer was rebuilt, uploaded and
     submitted every frame regardless of whether anything changed.
  3. `ControlFlow::Poll` meant the loop never slept even between frames.

  **Fixes:** pinned `PresentMode::Fifo`; split `Graphics::poll` (config
  reload, foreground scan, PTY drain, pane reaping — returns
  `PollOutcome { needs_redraw, panes_remain }`) out of `redraw` (pure GPU
  work), so "did anything change?" is answerable without rendering to
  find out; switched to `ControlFlow::WaitUntil` with the deadline driven
  by the only remaining periodic work (the 500ms foreground-process
  scan); and added `crate::waker::Waker` so PTY reader threads wake the
  sleeping loop.

  Two details worth keeping: the waker **coalesces** — an unguarded
  `send_event` per read would have replaced a busy render loop with a
  busy *event* loop under `yes`/large `cat`, since winit delivers every
  proxy event. And titles are diffed rather than assumed dirty: the scan
  runs on a timer, but a scan is not a change, so `refresh_titles`
  compares against a cache — otherwise a twice-a-second poll would have
  become a twice-a-second repaint. Used `()` as the user-event type so
  no `EventLoop<T>` generic churn was needed.

  **Measured, not assumed.** This sandbox renders via llvmpipe
  (software), so drawing shows up as CPU — a sensitive detector. Idle
  consumed **0.000s of CPU over 10s wall** (0.00% of a core). Separately
  confirmed the timer still fires — `--verbose=foreground` showed 36
  scan lines over ~7s across 3 panes (~1.7 scans/sec, matching the 500ms
  interval), so pane titles stay current despite the loop sleeping.
  59 app-crate tests (2 new for the coalescing guard), clippy clean,
  Windows cross-target clean.

  **Risk to flag on testing:** if the wake path were broken this would
  present as a *frozen* terminal (output never appearing), not a subtle
  regression. Startup rendering the shell prompt exercises that path, and
  the app runs clean — but typing/output responsiveness needs real
  interactive testing before release. Uncommitted.

  **Update — 2026-07-27:** Developer confirmed the idle-cost work
  ("that all appears to work") and reported menus being clipped by the
  window edge in a small window, asking whether we could render outside
  the window extents.

  **Answered honestly: no.** egui draws into the same wgpu surface as the
  grid — that surface *is* the window, so pixels past its edge don't
  exist. Rendering beyond the window would need a separate OS-level
  window (which is how native menus work). Noted but not pursued: winit
  supports multiple windows and egui has multi-viewport, but it would
  mean a second surface + egui context + focus/dismissal handling, and
  **Wayland is a genuine blocker** — it has no global coordinates, so an
  arbitrarily-positioned popup needs `xdg_popup`, which winit doesn't
  expose usefully.

  Checked the obvious in-window lever first and it was already correct:
  `Area::constrain` defaults to `true` in egui 0.35, so position *was*
  being clamped. That only slides a menu around though — useless when
  the menu is simply taller than the window, which is what the
  screenshot actually showed. The fix is to make it fit: new
  `popup_bounds`/`fit_popup` shrinks to the window minus a margin (with
  140x80 floors so it never collapses to a sliver) and wraps both context
  menus in a height-bounded `ScrollArea`.

  Applied the same treatment to the paste dialog (fixed 420px width) and
  the settings panel (width was already proportional from an earlier
  round, but a short window could push Save/Cancel off the bottom with no
  way to reach them). Split `fit_popup` out from the egui lookup so the
  sizing rule is testable without a context — 4 tests: roomy window keeps
  the preferred width, narrow window shrinks, tiny window stops at the
  floor, height always leaves room for the edge.

  63 app-crate tests, clippy clean both targets, smoke clean.
  Uncommitted alongside the idle-cost work.

  **Update — 2026-07-27:** While writing up how to test the sleep/wake
  work, spotted a real robustness hole in it. The waker re-armed only in
  `user_event`, so a single dropped proxy event would leave `pending`
  stuck `true`, the reader threads permanently silent, and the terminal
  **frozen with no recovery**. Moved the re-arm into `about_to_wait`,
  which runs on every wake *including the 500ms timer* — worst case is
  now one timer interval of latency instead of a permanent hang.
  `user_event` is now an empty handler that exists only to wake the loop.

  Also worth recording for whoever tests this: **typing echoing back does
  not prove the wake path works.** A keypress is a window event, which
  drives a redraw on its own — so the shell's echo would appear even with
  a completely broken waker. The decisive test is output arriving with no
  input at all (`sleep 3; echo hi`): if that appears unprompted, the
  reader thread genuinely woke the sleeping loop.
