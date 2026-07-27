# Changelog

## Unreleased

## v1.6.0

- On Linux and macOS, nothing is injected into your shell at all any more.
  Working directories for session restore are read straight from the
  operating system's process table instead of relying on the shell to
  report them, so bash starts exactly as it would in any other terminal —
  no generated startup file, no `--rcfile`, nothing added to
  `PROMPT_COMMAND`. It also means **zsh and fish panes now restore their
  working directory**, which they never did, along with any other shell.
  Windows still uses the old mechanism, having no way to read another
  process's working directory.
- Panes now start the shell the way the platform's own terminals do:
  interactive non-login on Linux, where a desktop session has already read
  the profile files, and login on macOS, where it hasn't — matching GNOME
  Terminal and Konsole on one side and Terminal.app and iTerm2 on the
  other. On macOS that means `~/.bash_profile` and `~/.zprofile` are read
  again, which is where a Mac user's `PATH` usually lives.
- Fixed: whether a pane's shell was a login shell depended on whether you
  had set `default_shell` in your config — a setting that has nothing to
  do with it. Leaving it unset gave a login shell and setting it gave a
  non-login one, so two machines with the same dotfiles behaved
  differently for no visible reason.
- Fixed: bash panes started as if they were login shells, so `~/.bashrc`
  was often run **twice**. The stock `~/.bash_profile` on Fedora and RHEL
  — and commonly on Debian and Ubuntu — ends by sourcing `~/.bashrc`, and
  we then sourced it again ourselves. Anything written to append rather
  than assign did it twice too: duplicated `PATH` entries, duplicated
  `PROMPT_COMMAND`, and prompt frameworks installing their hooks on top of
  themselves, which is why it showed up as colors and prompts coming out
  wrong. It also ran login-only setup (`ssh-agent`, tmux auto-attach) once
  per pane instead of once per login, and printed the login message every
  time a pane opened. Panes now start an ordinary interactive non-login
  shell — the system bashrc and `~/.bashrc`, exactly like every other
  terminal.
- Fixed: on Fedora, RHEL and macOS the system bashrc (`/etc/bashrc`) was
  never read at all, losing the system default prompt and the interactive
  half of `/etc/profile.d`. Only Debian's `/etc/bash.bashrc` happened to
  get picked up, and only indirectly.

- Fedora, RHEL, Rocky and Alma get a GPG-signed DNF repository, so
  `dnf install pain` and `dnf upgrade` work the same way `apt` already
  did on Debian and Ubuntu.
- An AppImage, for every other distribution — one file, no install, no
  root, and it works on immutable systems like Silverblue, Kinoite and
  Bazzite where layering a package means a reboot. See the README if your
  distribution doesn't ship libfuse2 (Fedora and Ubuntu 24.04 among them).
- Fixed: the Linux build picked up its glibc requirement from whatever
  the CI runner happened to be running, which silently rose each time
  GitHub updated that image and would eventually have stopped the package
  installing on older distributions for no visible reason. The build now
  happens in a pinned container, fixing the floor at glibc 2.35 — Debian
  12+, Ubuntu 22.04+, and every current Fedora — until it's moved
  deliberately.
- The Linux packages now recommend a font. The application aborts if there
  isn't one on disk, and while any machine with a graphical session has
  fonts already, apt and dnf both install recommendations by default — so
  an ordinary install can't land in that state.
- Fixed: the Vulkan driver was a recommended dependency rather than a
  required one, so installing with `--no-install-recommends` produced an
  install that could never start. It's now required, as an alternation
  that an NVIDIA driver satisfies too, so nobody is forced to install
  Mesa's driver to satisfy it.

## v1.5.0

- Clipboard shortcuts now match what people actually expect. On Windows
  and Linux, `Ctrl+C` copies when text is selected and still sends an
  interrupt when nothing is — so it gains the familiar meaning without
  ever costing you the ability to stop a running command — and `Ctrl+V`
  pastes. On macOS, `Cmd+C`/`Cmd+V` copy and paste (they previously did
  nothing at all), `Cmd+Q` quits and `Cmd+W` closes a pane; `Ctrl` is
  left entirely alone there, since Command is what the clipboard belongs
  on.

  `Ctrl+Shift+C`/`Ctrl+Shift+V` are no longer bound. Those chords only
  ever existed because the unshifted pair wasn't available, which is no
  longer true on either platform — keeping them would give one action two
  shortcuts, the second of them the awkward one. Add
  `"ctrl+shift+c" = "copy"` and `"ctrl+shift+v" = "paste"` to
  `[keybindings]` if you have the muscle memory.

- Menus, panels, and dialogs are drawn whenever they need to be. Hover
  highlights update, closing one no longer leaves it on screen until some
  unrelated click forces a repaint, and a click on a menu item can no
  longer also start a text selection in the pane behind it. All three came
  from the same place: the overlay's own repaint requests were being
  ignored, and egui's idea of where the pointer is only advances when a
  frame runs — so a skipped frame left it answering questions about a
  stale pointer position.
- Menus and dialogs now render at their content's full size and scroll
  only when the app window is genuinely too small for them. Several
  scrolled regardless: the paste preview was pinned to 160 pixels tall,
  and the context menus sized themselves against their own height from the
  previous frame — a loop that settles at whatever height a menu first
  happened to take and leaves a scrollbar up for good, however much room
  the window has. Panels squeezed narrow by a small window also now grow
  back when it's widened again, instead of staying squeezed.
- The settings panel's keybinding list is now a collapsible section,
  folded away by default. It's reference material, not something you need
  open while changing a font size.
- Documentation. There's now a man page (`man pain`, shipped in the `.deb`),
  and `pain --help`/`--version` do something — `--help` prints the config
  file path resolved for the machine you're on, since "where does this keep
  its settings" was previously answerable only by reading the source. The
  README documents every keyboard shortcut and mouse action, and the config
  file: where it lives per platform, every key with its default, and what
  happens when the file is malformed.
- The settings panel's Keybindings section now lists the bindings actually
  in effect, defaults included, marking the ones your config changed. It
  previously showed only overrides, so anyone who had never edited their
  config — the people most likely to look — saw an empty box telling them
  defaults existed without saying what they were.

- Fixed: right-click menus, the settings panel, and the paste dialog were
  cut off by the window edge when the window was small. They now shrink to
  fit and scroll for whatever still doesn't, so every action stays
  reachable at any window size.

- Massively reduced resource use when idle. Three things were wrong:
  on Windows the swapchain defaulted to a present mode with **no vsync
  cap**, so the GPU rendered as fast as it physically could; the event
  loop asked for a fresh frame on every iteration whether or not anything
  had changed; and it never slept, so it spun the CPU continuously. The
  loop now sleeps until something actually happens — PTY output wakes it
  directly — and only repaints when the screen genuinely changed. An idle
  terminal now measures at essentially zero CPU and does no GPU work at
  all.

## v1.4.1

- Fixed: shells were never told what terminal they were running in — `TERM`
  was left to whatever the app process happened to inherit. Launched from a
  desktop launcher (Finder, the Dock, a Linux applications menu) there is
  usually no `TERM` at all, which degrades the shell: in zsh it disables the
  line editor, so Backspace and other ordinary keys stop working. Shells now
  get `TERM=xterm-256color` and `COLORTERM=truecolor`.
- The macOS `.app` is now ad-hoc code-signed, so the bundle carries a
  proper seal covering its `Info.plist` and resources. This isn't a real
  developer-signed build — Gatekeeper still needs the quarantine flag
  cleared — but the bundle is no longer unsealed.
- README: documented how to actually launch `pain.app` from a terminal.
  A `.app` is a directory, so `./pain.app` fails with "permission denied"
  (zsh) or "Is a directory" (bash); use `open pain.app`, or run
  `./pain.app/Contents/MacOS/pain` directly to see log output.

## v1.4.0

- Holding `Ctrl` now underlines the URL under the pointer and switches to
  a hand cursor, so it's clear what a `Ctrl+click` will open before you
  click it.
- Fixed: the paste confirmation dialog (and the settings panel) left a
  large empty gap above their buttons, making both windows much taller
  than their content.

## v1.3.0

- Paste is now safe by default. Text is wrapped in bracketed-paste markers
  when the running program supports them, so shells hold it on the prompt
  for review instead of executing every line as it arrives. When the
  program *doesn't* support them, a multi-line paste asks for confirmation
  first and shows exactly what will be sent (`confirm_multiline_paste` in
  config turns this off).
- Copy and paste keyboard shortcuts: `Ctrl+Shift+C` and `Ctrl+Shift+V`
  (both remappable as `copy`/`paste`). Previously paste was reachable only
  through the right-click menu.
- Double-click selects a word, triple-click selects a line.
- `Ctrl+click` opens a URL in your browser.
- Fixed: window transparency did nothing on macOS. The Metal backend only
  ever offers a `PostMultiplied` composite mode, which the surface setup
  didn't accept, so every Mac ran fully opaque regardless of the
  configured transparency level.

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
