# OSC 8 hyperlinks and the Windows launch fix

**Shipped:** 2026-07-28. Two small, unrelated pieces of work recorded
together because neither warrants a document of its own.

## OSC 8 hyperlinks

A link a program marks explicitly with the OSC 8 escape sequence — what
`cargo` and `gcc` emit — is `Ctrl+click`-able. Previously only text that
*looked* like a URL was matched, so a link whose visible text was an ordinary
word was invisible.

**Nearly free, because the parsing already existed.** `alacritty_terminal`
parses OSC 8 and stores it per-cell; the app simply never read it. The work
was `Screen::hyperlink_at`.

**A point query, deliberately not a `RenderCell` field.** A link is only ever
needed for the cell under the pointer. Carrying one on every cell would add a
refcount bump per cell per frame to the render path for something almost
always absent.

**Span expansion compares `Hyperlink` by value**, which covers both id and
URI. Two adjacent runs that merely share a URI stay separate links — which is
exactly what an explicit OSC 8 id is for.

**The scheme allowlist applies to OSC 8 too, and this is the decision worth
recording.** An OSC 8 target is explicit rather than guessed, but it is still
arbitrary program output — a log line, or a file someone `cat`ed — choosing
both the link text and a target that need not resemble it. `url.rs`
deliberately excludes `file:` because terminals print paths constantly, and
`ls --hyperlink` emits exactly that. Rather than silently widen what a
Ctrl+click hands to the operating system, `url::is_allowed_scheme` governs
both paths.

The visible cost is that `ls --hyperlink` links are not clickable. That is an
existing policy held consistently, not a new restriction — and it is the
developer's call to relax it for declared links if they want.

## Windows launch (console subsystem)

**Symptom:** launching pain on Windows opened a console window that stayed
for the life of the process, and a shell that started it blocked until it
exited. macOS was unaffected; Linux appeared fine.

**Cause:** no `windows_subsystem` attribute existed, so the binary was built
as a **console** application — Rust's default. Windows allocates a console
for console-subsystem processes, and `cmd`/PowerShell wait for them.

Per platform, since the asymmetry confused the diagnosis:

- **macOS** — no subsystem concept; the `.app` launches from Finder with no
  shell involved. Never affected.
- **Linux** — *half* affected. No stray window, and the `.desktop` path is
  clean, but a shell launch does block. That is ordinary Unix foreground
  behaviour, which is why it looked fine.

**Fix:** `#![windows_subsystem = "windows"]` **plus**
`crates/app/src/console.rs`. The attribute alone would have been a silent
regression: with no console, `--help`, `--version` and `--verbose` print into
nothing from a terminal, and `--help` is specifically what tells people where
their config file lives. `AttachConsole(ATTACH_PARENT_PROCESS)` re-acquires
the launching terminal's console when there is one (Explorer has none, so
nothing appears), and reopens standard handles **only where they are
currently invalid**, so `pain --help > out.txt` keeps redirecting to the file
rather than being hijacked onto the screen.

Applied to debug builds too, deliberately, rather than the common
`not(debug_assertions)` variant — so development exercises the same startup
path that ships.

**Residual wart, inherent to the subsystem choice:** since the shell no
longer waits for a GUI application, CLI output arrives after the next prompt.
Every windowed Windows program with a command line behaves this way.
Documented in README and CHANGELOG rather than hidden.

**Verified without a Windows machine** by cross-compiling and reading the
PE Optional Header's Subsystem field, which reads `2`
(`IMAGE_SUBSYSTEM_WINDOWS_GUI`) where it was `3` (`WINDOWS_CUI`) — direct
evidence rather than inference from a clean compile. Real-world behaviour
still needs a Windows machine.
