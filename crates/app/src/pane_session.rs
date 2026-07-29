//! Owns a single pane's PTY and screen: a background thread pumps PTY output
//! into a channel, which the render loop drains once per frame.

use std::io::Read;
use std::sync::mpsc::{self, Receiver};
use std::thread;

/// What one [`PaneSession::pump`] found.
///
/// `changed` drives repainting; the other two feed the pane's title-bar
/// activity dot (`crate::activity`). They're deliberately separate: a pane
/// whose shell merely exited has `changed` set but did nothing worth
/// flagging for attention.
#[derive(Debug, Clone, Copy, Default)]
pub struct PumpOutcome {
    /// Whether anything changed such that a redraw is warranted.
    pub changed: bool,
    /// Whether new output actually arrived from the shell.
    pub output: bool,
    /// Whether the program rang the terminal bell.
    pub bell: bool,
}

/// A running shell plus the screen its output is parsed into.
pub struct PaneSession {
    pty: pane::Pty,
    screen: pane::Screen,
    rx: Receiver<Vec<u8>>,
    exit_logged: bool,
    /// What this pane has done since it was last focused — see
    /// `crate::activity`.
    activity: crate::activity::Activity,
    /// Set by [`PaneSession::write_input`], cleared by the next
    /// [`PaneSession::take_received_input`]. Input is what acknowledges a
    /// bell, and it arrives on the event loop's keyboard path rather than
    /// during a poll, so it has to be recorded until the poll gets to it.
    received_input: bool,
    /// The shell this pane was spawned with — `None` means "whatever the
    /// configured default was at the time" (so a restored pane keeps
    /// following the *current* default even if that's changed since),
    /// `Some` an explicit override (typically "Swap shell"). Recorded so
    /// session save can tell the two apart — nothing else remembers what a
    /// running pane's shell actually is.
    shell: Option<String>,
}

impl PaneSession {
    /// Spawns `shell` (or the platform default when `None`) behind a PTY of
    /// `size`, retaining `scrollback` lines of history, starting in `cwd` if
    /// given (session restore), and starts a background thread reading its
    /// output.
    pub fn spawn(
        shell: Option<&str>,
        size: pane::Size,
        scrollback: usize,
        cwd: Option<&std::path::Path>,
        waker: crate::waker::Waker,
    ) -> anyhow::Result<Self> {
        let pty = pane::Pty::spawn(shell, size, cwd)?;
        let mut reader = pty.try_clone_reader()?;
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        if crate::verbose::is_verbose(crate::verbose::Category::General) {
                            eprintln!("pane: PTY reader hit EOF (shell exited?)");
                        }
                        break;
                    }
                    Err(err) => {
                        if crate::verbose::is_verbose(crate::verbose::Category::General) {
                            eprintln!("pane: PTY reader error: {err}");
                        }
                        break;
                    }
                    Ok(n) => {
                        if crate::verbose::is_verbose(crate::verbose::Category::Pty) {
                            eprintln!("pane: read {n} bytes from PTY: {:?}", String::from_utf8_lossy(&buf[..n]));
                        }
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                        // Nudge the event loop: it's asleep until
                        // something happens, and this is the "something".
                        waker.wake();
                    }
                }
            }
        });

        Ok(Self {
            pty,
            screen: pane::Screen::new(size, scrollback),
            rx,
            exit_logged: false,
            activity: crate::activity::Activity::default(),
            received_input: false,
            shell: shell.map(str::to_string),
        })
    }

    /// The shell this pane was spawned with, for session save — see the
    /// field's own doc comment for what `None` vs. `Some` means.
    pub fn shell(&self) -> Option<&str> {
        self.shell.as_deref()
    }

    /// Applies any PTY output received since the last call.
    ///
    /// The `changed` field of the result drives repainting: the render loop
    /// only wakes the GPU when this (or some other real change) says so, so
    /// an idle pane must cost nothing.
    pub fn pump(&mut self) -> PumpOutcome {
        let mut outcome = PumpOutcome::default();
        while let Ok(chunk) = self.rx.try_recv() {
            self.screen.advance(&chunk);
            outcome.changed = true;
            outcome.output = true;
        }

        let writes = self.screen.take_pty_writes();
        if !writes.is_empty()
            && let Err(err) = self.pty.write(&writes)
        {
            eprintln!("pane: failed to write terminal reply to PTY: {err:#}");
        }

        // Read after the parse above, since that's what raises it. A bell
        // repaints too — the title-bar dot is the only thing that shows it,
        // so nothing else would bring it on screen.
        outcome.bell = self.screen.take_bell();
        if outcome.bell {
            outcome.changed = true;
        }

        if !self.exit_logged
            && let Some(status) = self.pty.exit_status()
        {
            if crate::verbose::is_verbose(crate::verbose::Category::General) {
                eprintln!("pane: shell exited: {status}");
            }
            self.exit_logged = true;
            outcome.changed = true;
        }

        outcome
    }

    /// What this pane has done since it was last focused.
    pub fn activity(&self) -> crate::activity::Activity {
        self.activity
    }

    /// Advances the pane's activity state for one poll — see
    /// `crate::activity::next` for the rule.
    pub fn update_activity(&mut self, focused: bool, signals: crate::activity::Signals) {
        self.activity = crate::activity::next(self.activity, focused, signals);
    }

    pub fn screen(&self) -> &pane::Screen {
        &self.screen
    }

    /// Whether the pane's shell has exited on its own (e.g. the user typed
    /// `exit`), as opposed to being closed via an app-level close action.
    pub fn has_exited(&mut self) -> bool {
        self.pty.has_exited()
    }

    /// Resizes both the PTY (so the kernel/ConPTY and the running program
    /// agree on the new size) and the parsed grid.
    pub fn resize(&mut self, size: pane::Size) -> anyhow::Result<()> {
        self.pty.resize(size)?;
        self.screen.resize(size);
        Ok(())
    }

    /// Writes keyboard input through to the shell. Also snaps the viewport
    /// back to live output first — matching every other terminal's
    /// convention that typing always returns focus to the live prompt,
    /// even mid-scrollback.
    pub fn write_input(&mut self, data: &[u8]) -> anyhow::Result<()> {
        if crate::verbose::is_verbose(crate::verbose::Category::Pty) {
            eprintln!("pane: writing {data:?} to PTY");
        }
        self.screen.scroll_to_bottom();
        self.received_input = true;
        self.pty.write(data)
    }

    /// Whether the user sent input to this pane since the last call,
    /// clearing the flag — see the field's own doc comment.
    pub fn take_received_input(&mut self) -> bool {
        std::mem::replace(&mut self.received_input, false)
    }

    /// Scrolls the viewport `lines` rows back into history (positive) or
    /// forward toward live output (negative) — see `pane::Screen::scroll`.
    pub fn scroll(&mut self, lines: i32) {
        self.screen.scroll(lines);
    }

    /// Changes how many lines of history this pane retains, for a live
    /// config edit — see `pane::Screen::set_scrollback`.
    pub fn set_scrollback(&mut self, scrollback: usize) {
        self.screen.set_scrollback(scrollback);
    }

    /// Starts a fresh in-grid text selection at 0-indexed (row, col).
    /// Starts a selection of the given granularity — see
    /// `pane::Screen::start_selection_of`.
    pub fn start_selection_of(&mut self, row: usize, col: usize, kind: pane::SelectionKind) {
        self.screen.start_selection_of(row, col, kind);
    }

    /// Extends the in-progress selection (if any) to 0-indexed (row, col).
    pub fn update_selection(&mut self, row: usize, col: usize) {
        self.screen.update_selection(row, col);
    }

    /// Clears the active selection, if any.
    pub fn clear_selection(&mut self) {
        self.screen.clear_selection();
    }

    /// Whether the active selection (if any) never actually moved from
    /// where it started — a plain click, not a drag, so there's nothing
    /// meaningful to keep highlighted or copy.
    pub fn selection_is_empty(&self) -> bool {
        self.screen.selection_is_empty()
    }

    /// The pid of this pane's own shell process, for foreground-process
    /// lookups (`crate::foreground_process`).
    pub fn shell_pid(&self) -> Option<u32> {
        self.pty.shell_pid()
    }

    /// The process group currently in the foreground of this pane's PTY,
    /// if this platform can report one (Unix only — see `pane::Pty::
    /// foreground_pgid`; always `None` elsewhere, so callers don't need
    /// their own `cfg` branch just to ask).
    pub fn foreground_pgid(&self) -> Option<u32> {
        #[cfg(unix)]
        {
            self.pty.foreground_pgid()
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// This pane's working directory, for session save — falls back from
    /// the shell's own OSC 7 report through an OS-level lookup to the
    /// user's home directory, in that order (see `crate::session_cwd`).
    pub fn cwd(&self, processes: &mut crate::foreground_process::ForegroundProcesses) -> std::path::PathBuf {
        let os_level = self.shell_pid().and_then(|pid| processes.cwd_of(pid));
        crate::session_cwd::resolve(self.screen.cwd(), os_level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reproduces the developer's exact real-app report ("running this
    // within WSL, bash does not return to the correct path"): not the raw
    // `pane::Pty`/`pane::Screen` pairing `pane`'s own tests already cover,
    // but the actual `PaneSession` + `pump()` loop the real render loop
    // uses, with a real `cd` typed through `write_input` — closer to real
    // usage than anything tested so far.
    #[cfg(unix)]
    #[test]
    fn cwd_reflects_a_real_cd_typed_into_a_real_pane_session() {
        let dir = std::env::temp_dir();
        let expected = dir.canonicalize().unwrap_or_else(|_| dir.clone());

        let mut session =
            PaneSession::spawn(Some("bash"), pane::Size { rows: 24, cols: 80 }, 100, None, crate::waker::Waker::noop())
                .expect("spawn a real pane");
        session.write_input(format!("cd {}\n", expected.display()).as_bytes()).expect("write cd command");

        let mut processes = crate::foreground_process::ForegroundProcesses::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut cwd = session.cwd(&mut processes);
        while cwd != expected && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
            session.pump();
            cwd = session.cwd(&mut processes);
        }

        assert_eq!(cwd, expected, "pane's tracked cwd should follow a real `cd` typed into it");

        // On Unix this has to be the process-table lookup doing the work,
        // not a shell hook: nothing injects OSC 7 there any more, and the
        // point of reading the OS directly is that no cooperation from the
        // shell is required. If a report did arrive, the shell's own
        // configuration emitted it, and this assertion is the wrong shape
        // — but on a machine where that isn't happening, it's what stops
        // this test quietly going back to proving the old mechanism.
        #[cfg(unix)]
        if session.screen().cwd().is_none() {
            let os_level = session.shell_pid().and_then(|pid| processes.cwd_of(pid));
            assert_eq!(os_level.as_deref(), Some(expected.as_path()), "the OS-level lookup alone should have found it");
        }
    }

    /// The capability that reading the OS bought us: a shell nothing was
    /// ever injected into now tracks its working directory. `/bin/sh` is
    /// used because it's guaranteed to exist, but the same is true of zsh
    /// and fish, which had no cwd tracking at all before this.
    #[cfg(unix)]
    #[test]
    fn a_shell_with_no_integration_still_tracks_its_working_directory() {
        let dir = std::env::temp_dir();
        let expected = dir.canonicalize().unwrap_or_else(|_| dir.clone());

        let mut session = PaneSession::spawn(
            Some("/bin/sh"),
            pane::Size { rows: 24, cols: 80 },
            100,
            None,
            crate::waker::Waker::noop(),
        )
        .expect("spawn a real pane");
        session.write_input(format!("cd {}\n", expected.display()).as_bytes()).expect("write cd command");

        let mut processes = crate::foreground_process::ForegroundProcesses::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut cwd = session.cwd(&mut processes);
        while cwd != expected && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
            session.pump();
            cwd = session.cwd(&mut processes);
        }

        assert_eq!(cwd, expected, "an uninjected shell's cwd should be readable from the process table");
    }

    /// The whole chain, through a real shell rather than a stand-in: a
    /// program prints `BEL`, the VT parser raises the event, `pump` reports
    /// it, and an unfocused pane ends up flagged for attention. Each link
    /// is unit-tested on its own; this is what proves they're connected.
    #[test]
    fn a_real_shell_ringing_the_bell_flags_an_unfocused_pane() {
        let mut session =
            PaneSession::spawn(Some("sh"), pane::Size { rows: 24, cols: 80 }, 100, None, crate::waker::Waker::noop())
                .expect("spawn a real pane");

        session.write_input(b"printf '\\a'\n").expect("write the bell command");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut rang = false;
        while !rang && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let pumped = session.pump();
            rang |= pumped.bell;
            // Unfocused, which is the only state the dot exists for.
            session.update_activity(
                false,
                crate::activity::Signals { output: pumped.output, bell: pumped.bell, input: false },
            );
        }

        assert!(rang, "a real shell printing BEL should surface as a bell from pump()");
        assert_eq!(
            session.activity(),
            crate::activity::Activity::Bell,
            "the bell should leave the pane flagged for attention"
        );

        // Merely focusing the pane must NOT clear it. `printf '\a'` rings
        // while its own pane is still focused, so clearing on focus erased
        // the flag before it could ever be drawn — the reported bug.
        session.update_activity(true, crate::activity::Signals::default());
        assert_eq!(
            session.activity(),
            crate::activity::Activity::Bell,
            "focus alone must not erase a bell that was never seen"
        );

        // Typing into the pane is what acknowledges it, through the real
        // input path rather than a hand-set flag.
        session.write_input(b"\n").expect("write to the pane");
        let signals = crate::activity::Signals { input: session.take_received_input(), ..Default::default() };
        session.update_activity(true, signals);
        assert_eq!(session.activity(), crate::activity::Activity::Idle);
    }
}
