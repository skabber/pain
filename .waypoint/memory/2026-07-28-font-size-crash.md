# Session: glyph atlas exhaustion crash (GitHub issue #1)

- **2026-07-28:** First externally-reported bug on `github.com/w-p/pain`
  ([issue #1](https://github.com/w-p/pain/issues/1), from `satelliteoflove`):
  dragging the settings panel's font-size slider crashes the app with
  `wgpu error: Validation Error / In Queue::write_texture / Copy of Y
  1002..1025 would end up overrunning the bounds of the Destination texture
  of Y size 1024`, followed by a segfault. The second panic in the report
  ("Trying to destroy a SwapchainAcquireSemaphore that is still in use") is
  fallout from unwinding through the first, not a separate bug.

  **Root cause — `crates/render/src/atlas.rs`, two compounding defects:**

  1. The glyph cache was keyed by `(char, font_size_bits, family)` and
     nothing was ever evicted. Every distinct font size the slider passes
     through therefore claimed its own permanent copy of the whole character
     set. The 1024x1024 atlas holds roughly 680 glyphs at 48px — a handful of
     size changes exhausts it.
  2. `alloc` (the shelf packer) had no bounds check at all, by explicit
     design — its doc comment said "v1 does not handle atlas exhaustion — the
     character sets rendered by a terminal fit comfortably within one
     1024x1024 atlas". That assumption was true when written and stopped being
     true the moment a font-size *selector* existed. Past the bottom edge it
     kept handing out positions, and `write_texture` at one of those is a
     wgpu validation error, which wgpu treats as fatal by default.

  **Fix:**
  - Entries are now keyed by `char` alone, with the packed `(size, family)`
    stored alongside; a change to either repacks the atlas from scratch. This
    is what actually bounds it — one size's glyph set at a time.
  - The packer is extracted into a `ShelfPacker` struct (pure logic, no GPU,
    so it is unit-testable) and `alloc` returns `Option`, refusing anything
    that won't fit. On refusal `entry` repacks once and retries; a glyph
    larger than the atlas returns `None` and is skipped. No path panics.
  - `ATLAS_SIZE` 1024 → 2048 (1 MiB → 4 MiB, `R8Unorm`). Headroom for the
    residual case the repack can't help with: a single frame containing more
    distinct glyphs than the atlas holds (large font + CJK) would otherwise
    repack mid-frame every frame. That case now garbles a frame instead of
    crashing, and 2048 puts it out of practical reach.

  **Verified by reproducing it, not by reasoning.** Added
  `sweeping_font_sizes_never_overruns_the_texture` — a real headless wgpu
  device (`pollster` added as a dev-dep to `render`; already in the lockfile
  via `app`) sweeping sizes 6..=48 over printable ASCII. Ran it against
  `HEAD`'s pre-fix `atlas.rs`: it panics in `GlyphAtlas::entry` with
  `Copy of Y 1004..1033 would end up overrunning ... texture of Y size 1024`
  — the reporter's failure verbatim. Passes on the fixed code. wgpu's
  validation only runs on a real device, so this could not have been caught
  by a pure-logic test.

  Two environment quirks worth remembering, both WSL-specific and neither a
  product bug:
  - Tearing a wgpu device down segfaults inside WSLg's GL driver. The test
    `std::mem::forget`s the device/queue so that doesn't fail it; there is a
    comment saying so.
  - `import -window root` cannot read the WSLg root window
    ("Resource temporarily unavailable"), so no screenshot of the running
    app was possible. Smoke launch was clean (12s, no panics), but visual
    confirmation of glyph rendering at the new atlas size needs real
    hardware.

  **`cargo fmt` used to be a trap here; now there's a `rustfmt.toml`.** The
  OPORD said "rustfmt defaults, no custom config, run before every commit",
  but the codebase was written at roughly 110-120 columns and was *not*
  rustfmt-default clean: a `cargo fmt --all` early in this session rewrote 24
  files / ~2200 lines across every crate, which had to be reverted by hand.
  Nothing in CI has ever checked it (`.github/workflows/` runs no
  `cargo fmt --check` and no `cargo test` at all).

  Developer asked for a config. Picked `max_width = 120` +
  `use_small_heuristics = "Max"` **by measurement, not preference** — swept
  candidate widths at both heuristic settings and counted the hunks
  `cargo fmt --all -- --check` produced against the existing tree:
  115/Default 185, 110/Max 116, 120/Max 100 (the minimum), 125/Max 110.
  `Max` matters more than the width: it keeps a call or struct literal on one
  line whenever it fits, which is what the hand-written code already does.
  The OPORD's Formatting row now points at `rustfmt.toml`.

  Developer's call: **format everything** ("I don't care if formatting
  creates a big diff"). Ran `cargo fmt --all` — 20 files, ~840 lines each
  way; `cargo fmt --all -- --check` is now clean, clippy clean, suite passes.
  The tree is rustfmt-clean for the first time, so future `cargo fmt` runs
  produce no unrelated churn.

- **2026-07-28 — bug hunt (developer asked for one after issue #1 crashed a
  user's terminal).** Read the whole workspace looking for crash-class and
  logic bugs; every finding below was **reproduced**, not inferred from
  reading. Nothing is fixed yet — reported and awaiting the developer's call
  on scope. Probe tests were written under `crates/*/tests/probe.rs`, used,
  and deleted; they are not in the tree.

  **1. `font_size = 0` in `config.toml` panics the running app.**
  `render::measure_cell` → `GlyphRasterizer::advance_width` →
  `cosmic_text::Buffer::new_empty`, which asserts `line_height != 0.0`.
  `glyph.rs` builds `Metrics::new(size_px, size_px * 1.2)`, so a zero size
  gives a zero line height. Reached from `Graphics::apply_settings`
  (`graphics.rs:499`) on hot reload — i.e. saving the file kills a running
  terminal — and from `Graphics::new` at startup. Directly violates the
  "never crash on a bad edit" rule `poll_config_reload`'s own doc comment
  states. Note `-0.0` fails the same assert.

  **2. `font_size` negative hangs the app at 100% CPU, permanently.**
  `measure_cell(-13.0, "")` never returns — measured 214s at 100% CPU with
  RSS flat at 5.6 MB, so it is a spin inside cosmic-text's layout, not an
  allocation blowup. A very large size (tested 4000) is effectively the same
  outcome. Same entry points as (1).

  Both are one fix: clamp/validate `appearance.font_size` at the `config`
  boundary (the settings slider's own 6.0–48.0 range is the obvious bound),
  so no hand-edited value can reach `measure_cell`.

  **3. `general.scrollback_lines` does nothing.** It is fully wired as a
  *setting* — settings-panel `DragValue`, saved, round-tripped, documented,
  defaulted to 5000 — but `pane::Screen::new` (`term.rs:92`) hardcodes
  `alacritty_terminal::term::Config::default()`, whose `scrolling_history` is
  10000. Nothing ever passes the configured value through. `project.md` records
  it as inert "since scrollback itself isn't implemented yet"; scrollback
  *was* implemented later (`Screen::scroll` + the `MouseWheel` handler) and
  this was never connected. So the real scrollback is 10000 regardless, and
  changing the setting has no effect.

  **4. Paste confirmation is bypassed by carriage returns.**
  `paste::needs_confirmation` tests `text.trim_end_matches('\n').contains('\n')`
  — only `\n`. But `\r` is what actually submits a command: `main.rs:696`
  sends `\r` for Enter, and the pty line discipline maps CR to NL. Verified
  end to end by writing `b"echo MARKER_ONE\recho MARKER_TWO\r"` to a real
  `/bin/sh` pty — both commands ran. So `echo a\recho b` pastes and executes
  with no prompt, while the module's own doc comment frames this as the
  protection against "attacker-influenced text (a web page, a log line, a
  file someone else wrote)". Fix: count `\r` as a line break too (and
  probably `\r\n` as one).

  **5. A left-button release the egui overlay consumes leaves a drag
  latched.** `main.rs`'s `MouseInput { .. Left .. } if !ui_consumed` arm is
  where `end_drag`/`end_selection`/`mouse_release` live. `egui-winit` reports
  `consumed: egui_ctx.egui_wants_pointer_input()` for `MouseInput` — press
  *and* release alike (checked in its source) — so press on a divider outside
  an open settings panel/context menu, drag over the panel, release there,
  and none of the three run: `dragging` stays `Some`, and every later mouse
  move keeps resizing the split with no button held. Same latch for
  `selecting` and for `mouse_gesture` (which keeps forwarding drag reports to
  the program). There is also **no `WindowEvent::Focused` or `CursorLeft`
  handler at all**, so alt-tabbing mid-drag latches identically. Fix:
  end drags/selections on release regardless of `ui_consumed`, and on focus
  loss.

  **6. A failed split leaves the surviving pane's PTY at the wrong size.**
  `Graphics::split_pane` calls `resize_panes_to_geometry()` *before*
  spawning; on spawn failure it does `self.layout.close(new_pane)` but never
  re-resizes, so the pane that was split keeps a grid sized for half the
  space while drawing at full width.

  **7. A pane that fails to spawn during session restore becomes a ghost.**
  `Graphics::new` logs the error and skips inserting the `PaneSession`, but
  the pane stays in the layout tree — a blank region with no title bar that
  `poll` never reaps (it only iterates `self.panes`) and that focus can still
  move onto, where typing goes nowhere. If it is the *only* pane, startup
  bails outright. Not reachable via a deleted saved cwd (checked: portable-pty
  filters a non-existent cwd on Windows and tolerates it on Unix), but
  reachable via a bad `default_shell`.

  **Checked and found sound**, so nobody re-treads them: `layout` (NaN/
  out-of-range ratios are absorbed by `geometry.rs`'s `.max(0.0)` guards;
  no panics), `pane::cwd`'s hand-rolled OSC 7/9;9 scanner (bounded buffer,
  no panics, byte-wise percent decoding), `mouse::encode` (no overflow,
  coordinates capped), `router::keymap`'s override parsing (total), `waker`
  (clear-before-drain ordering is correct — no lost wakeups), `session`
  (graceful on corrupt/missing files), `url` (`match_at_column` can't produce
  `end < start`). Alacritty tolerates the one-past-the-end grid coordinates
  `Graphics::cell_at` can produce when a pane rect isn't an exact multiple of
  the cell size — verified with a real `Screen` under debug assertions, so
  that is a latent oddity, not a bug.

  Workspace clippy clean, all tests pass (9 in `render`, including the new
  GPU one), Windows cross-target clippy clean apart from one pre-existing
  `unused import: super::*` warning in the `app` crate. `CHANGELOG.md`'s
  `## Unreleased` has a user-facing entry. **Uncommitted** — awaiting the
  developer's go-ahead, and no comment has been posted on the issue.

- **2026-07-28 — all seven fixed.** Developer: "Implement fixes for all of
  these findings. Document them as needed. Then commit it all, push it, and
  tag to create a release."

  **(1)(2) Bad `font_size` values.** New `Config::sanitize`, run on every
  file that parses, clamps `font_size` to 6-48, `transparency` to 0.0-1.0
  and `scrollback_lines` to a 1000000 ceiling. Out-of-range values clamp to
  the nearest end rather than resetting to the default (a `font_size = 100`
  has a legible intent worth honouring); `NaN` takes the default, and needs
  its own branch because `f32::clamp` passes `NaN` straight through. Chose
  the `config` crate boundary deliberately — it is the single place every
  hand-edited value enters the app, so nothing downstream needs its own
  guard.

  Verified against the real binary, not just unit tests: with
  `XDG_CONFIG_HOME` pointed at a scratch dir (so the developer's own config
  was never touched), the app now starts fine with `font_size = 0`, and
  **survives both bad values hot-reloaded into a running instance** — the
  case that actually mattered, since that path used to kill a live terminal.
  Stayed at ~4% CPU where a negative value previously pinned a core.

  Self-inflicted bug caught in that same verification: the first version
  printed the warning from inside `try_load`, and one config save produced
  **11 identical warnings** — the watcher re-reads the file several times
  per save, and the reload is only deduplicated later, at the apply
  decision. Fixed by having `sanitize` *return* the messages and
  `poll_config_reload` print them only when it actually applies. One line
  per edit now, measured. `try_load`'s signature changed to
  `Result<(Config, Vec<String>), _>` as a result.

  **(3) `scrollback_lines`.** `Screen::new` takes the value now instead of
  building `Config::default()`, threaded through `PaneSession::spawn` and
  all three `Graphics` call sites. Also made it hot-reloadable via a new
  `Screen::set_scrollback` (`Term::set_options` → `Grid::update_history`
  exists for exactly this), applied to already-open panes in
  `apply_settings` — consistent with every other setting here, and it
  avoids documenting an awkward "only affects new panes" caveat. Tests
  assert the retained history by over-scrolling and reading the resting
  display offset, which is the honest measurement of what the grid actually
  kept. **Note the user-visible consequence:** anyone who never set this now
  gets 5000 lines where they silently had 10000. Called out explicitly in
  the changelog rather than buried.

  **(4) Paste `\r` bypass.** `needs_confirmation` and a new private `lines`
  helper both split on `['\n', '\r']`. Empty pieces are dropped, which makes
  a trailing `\r\n` one break rather than two and stops a blank line
  inflating the dialog's count — defensible since the count exists to say
  roughly how many commands will run.

  **(5) Latched drags.** New `Graphics::end_pointer_gestures` ends the
  divider drag, the selection and the mouse-report gesture together and
  reports whether anything was live. `main.rs` calls it from a **new,
  deliberately un-gated** `MouseInput { state: Released, .. }` arm — the
  whole bug was that the release shared the `if !ui_consumed` guard with the
  press — and from a new `WindowEvent::Focused(false)` arm for the alt-tab
  case. The release still gets forwarded to the program on focus loss, so a
  mouse-tracking program isn't left believing the button is down.

  **(6)(7)** A failed split now re-runs `resize_panes_to_geometry` after
  undoing itself; a pane that fails to spawn during restore is closed out of
  the layout instead of lingering as an unreapable blank rect.

  Docs: `CHANGELOG.md` has a user-facing entry per fix, README and
  `man/pain.1` document the clamping and the new ranges, `project.md` has a
  hardening-pass paragraph. 196 tests pass, clippy clean on both native and
  the Windows cross-target, smoke launch clean.
