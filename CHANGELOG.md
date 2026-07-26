# Changelog

## Unreleased

## v1.2.0

- An application icon, and a desktop entry on Linux — the app now appears
  in the applications menu after `apt install` (previously it could only
  be launched by typing `pain` into some other terminal) and shows its own
  icon in the taskbar and alt-tab switcher.
- macOS releases now ship a proper universal `pain.app` bundle — one
  download that runs natively on both Intel and Apple Silicon, launchable
  from Finder and Spotlight, instead of a bare per-architecture binary.
- The window title is now "pain" rather than "Terminal Emulator (dev)".

## v1.1.0

- A GPG-signed APT repository (hosted on GitHub Pages) for Debian/Ubuntu,
  published automatically on every release — `apt install`/`apt upgrade`
  support instead of manually downloading the `.deb` each time. See the
  README for the `sources.list` setup.

## v1.0.0

- A close button on every pane's title bar, and a "Close" action on both
  right-click menus (the pane-management one and the terminal content
  one) — closing a pane no longer requires the `Ctrl+Shift+W` chord. The
  close button is a proper square, evenly padded from the title bar's
  top, right, and bottom edges alike, rather than a tall sliver shaped by
  raw monospace-cell metrics.
- Fixed: closing a pane in the middle of an arranged row/column only grew
  its immediate structural neighbor, leaving everything else at its old
  size (e.g. closing the middle of three equal horizontal panes left one
  at its original third and ballooned the other to two-thirds). Closing a
  pane now rebalances every pane in the same visual row/column to an
  equal share of the freed space.
- Settings now live-preview as you edit — background/accent color,
  transparency, and font family/size update the terminal immediately
  while the panel is open, not just after Save; closing the panel via
  Cancel (or its own close button) without saving reverts to the last
  saved values.
- Fixed: the terminal grid's font size ignored the OS's display-scaling
  setting entirely — on a 125%-scaled display, text rendered noticeably
  smaller than every other (DPI-aware) app on screen, even though the
  configured size was unchanged. Font size is now scaled by the window's
  DPI factor, recomputed live if the window moves to a monitor with a
  different scaling setting.
- The project has a name: **pain**. The `app` crate/binary is now `pain`
  (`cargo run -p pain`); a `.deb` package can be built with `cargo deb -p
  pain` (requires `cargo install cargo-deb` once) for Debian/Ubuntu
  distribution.
- A new right-click terminal context menu (Copy/Paste) when right-clicking
  a pane's terminal content; the existing pane-management menu
  (Broadcast/Split/Arrange/Group/Swap shell/Settings) now only opens from
  a right-click on the pane's title bar specifically.
- Fixed: Tab-key completion silently did nothing in every shell — egui's
  own focus-cycling convention was unconditionally swallowing every Tab
  keypress before it could reach the pty.
- Refined the context menu and settings panel layout: a uniform 2px corner
  radius throughout, bordered sections with small-caps monospace headers
  in the context menu (Broadcast/Split/Arrange/Group/Swap shell), a
  plain-link "Settings..." entry, and a grid-aligned four-section settings
  panel (Appearance/Terminal/Shell/Keybindings) with evenly distributed
  shell quick-pick buttons.
- A new default look ("Graphite"): a cool near-black palette, a
  user-configurable accent color (Settings) driving the cursor and
  selection highlight, and native system-font chrome for the context menu
  and settings panel instead of a generic toolkit look.
- A right-click "Arrange all panes" action (Horizontal/Vertical/Grid) to
  instantly retile every open pane into a preset layout.
- Session persistence: layout, window size, and each pane's working
  directory, chosen shell, and group membership are saved on quit and
  restored on next launch (never restarts whatever was running).
- Automatic OSC 7 (working-directory reporting) shell integration for bash
  and PowerShell panes, so session restore's directory tracking actually
  works without any manual shell configuration.
- Colored terminal output: full ANSI/256-color/true-color rendering.
- Scrollback: mouse-wheel scrolling through a pane's history.
- A font-family selector in Settings, listing installed monospaced fonts.
- A "Swap shell" pane context-menu action, for switching a pane's shell
  in place (e.g. into WSL) without closing it.
- `--verbose` now accepts categories (`mouse`/`pty`/`foreground`/`all`) so
  high-frequency diagnostic streams don't drown out everything else.
- Fixed: a WSL-rooted pane's title could get stuck on `conhost.exe`
  forever, regardless of what was actually running in the shell.
- Fixed: brighter pane-group title-bar colors weren't switching to dark
  text for readability.
- Project scaffolding: Cargo workspace with `pane`, `layout`, `router`,
  `config`, `render`, and `app` crates. MIT license.
