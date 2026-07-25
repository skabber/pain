//! PTY wrapper: spawns a shell and exposes it as a plain byte stream.

mod cwd;
mod integration;
mod term;

use std::io::{Read, Write};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub use alacritty_terminal::selection::SelectionRange;
pub use alacritty_terminal::term::TermMode;
pub use alacritty_terminal::term::cell::Flags;
pub use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
pub use term::{RenderCell, Screen};

/// Terminal dimensions, in character cells.
#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub rows: u16,
    pub cols: u16,
}

#[cfg(unix)]
fn default_shell_for_classification() -> Option<String> {
    integration::resolve_default_shell()
}

#[cfg(not(unix))]
fn default_shell_for_classification() -> Option<String> {
    None
}

/// A spawned shell process behind a pseudo-terminal.
pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Pty {
    /// Spawns `shell` (or the platform default shell when `None`) behind a
    /// new PTY sized to `size`, starting in `cwd` if given (session
    /// restore's job — `None` elsewhere, which leaves the shell to inherit
    /// this process's own cwd, same as running it by hand). Shells this
    /// recognizes (`crate::integration::classify`) get OSC 7 (cwd
    /// reporting) injected automatically, since not every shell emits it
    /// on its own — see `crate::integration`'s doc comment for how.
    pub fn spawn(shell: Option<&str>, size: Size, cwd: Option<&std::path::Path>) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // An explicit `shell` always wins; `None` is resolved the same
        // way `portable_pty` itself would on Unix (`$SHELL`, then the
        // current user's `/etc/passwd` entry — see
        // `integration::resolve_default_shell`), purely to decide whether
        // integration applies — Windows' own default (`%ComSpec%`, i.e.
        // cmd.exe) isn't a family this injects into, so there's nothing
        // to gain resolving it further there.
        let resolved = shell.map(str::to_string).or_else(default_shell_for_classification);
        let family = resolved.as_deref().map(integration::classify).unwrap_or(integration::Family::Other);

        let mut cmd = match (shell, family) {
            // A recognized family needs its own extra arguments, which
            // `new_default_prog()` refuses outright (it panics if `arg`
            // is called on it) — so even a shell resolved only from
            // `$SHELL`, not named explicitly, has to be spawned
            // explicitly here. The integration script manually replicates
            // what a plain login shell would have sourced, so this
            // doesn't lose the login-shell startup behavior
            // `new_default_prog()` would otherwise have provided.
            (_, integration::Family::Bash) | (_, integration::Family::PowerShell) => {
                CommandBuilder::new(resolved.as_deref().expect("classified implies resolved"))
            }
            (Some(shell), _) => CommandBuilder::new(shell),
            (None, _) => CommandBuilder::new_default_prog(),
        };
        integration::apply(&mut cmd, family);
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }
        let child = pair.slave.spawn_command(cmd)?;
        let writer = pair.master.take_writer()?;

        Ok(Self {
            master: pair.master,
            writer,
            child,
        })
    }

    /// Returns a fresh handle for reading the shell's output.
    ///
    /// Only one reader should be kept alive per `Pty` — each byte of PTY
    /// output is delivered to whichever reader happens to read it first.
    pub fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        self.master.try_clone_reader()
    }

    /// Writes raw bytes to the shell's input.
    pub fn write(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.writer.write_all(data)?;
        Ok(())
    }

    /// Resizes the underlying PTY, informing the shell of the new dimensions.
    pub fn resize(&self, size: Size) -> anyhow::Result<()> {
        self.master.resize(PtySize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// Reports whether the child shell has already exited.
    pub fn has_exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Returns the child's exit status once it has exited, for diagnostics.
    pub fn exit_status(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.to_string()),
            _ => None,
        }
    }

    /// Terminates the child shell process. Best-effort: called on drop, so
    /// closing a pane always frees its process regardless of whether the
    /// caller does this explicitly.
    pub fn kill(&mut self) -> anyhow::Result<()> {
        self.child.kill()?;
        Ok(())
    }

    /// The pid of the spawned shell itself (the direct child of this PTY),
    /// if the platform reports one.
    pub fn shell_pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// The process group currently in the foreground of this PTY, if any —
    /// `tcgetpgrp` on the PTY master, via `portable_pty`'s own
    /// `process_group_leader`. Unix only: a shell puts each foreground job
    /// in its own process group, led by the job itself, so for a simple
    /// foreground command this is almost always that command's own pid —
    /// the correct, direct signal for "what's running in this pane right
    /// now," which Windows/ConPTY has no equivalent of.
    #[cfg(unix)]
    pub fn foreground_pgid(&self) -> Option<u32> {
        self.master.process_group_leader().and_then(|pid| u32::try_from(pid).ok())
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn spawns_shell_and_echoes_input() {
        let mut pty = Pty::spawn(None, Size { rows: 24, cols: 80 }, None).expect("spawn shell");
        let mut reader = pty.try_clone_reader().expect("clone reader");
        pty.write(b"echo hello_pty_test\n")
            .expect("write input to shell");

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut seen = String::new();
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                seen.push_str(&String::from_utf8_lossy(&buf[..n]));
                if seen.contains("hello_pty_test") {
                    break;
                }
            }
            let _ = tx.send(seen);
        });

        let output = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("shell produced no output within timeout");
        assert!(
            output.contains("hello_pty_test"),
            "expected echoed output, got: {output:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn spawns_the_shell_in_the_given_starting_directory() {
        let dir = std::env::temp_dir();
        let expected = dir.canonicalize().unwrap_or_else(|_| dir.clone());
        let needle = expected.to_str().expect("temp dir should be valid UTF-8").to_string();

        // `/bin/sh`, not the platform default shell (`None`): a real
        // interactive login shell can run dotfiles (`.bashrc`/`.profile`)
        // that `cd` somewhere themselves, which would silently override
        // whatever `cwd` was passed here and has nothing to do with
        // whether that argument was actually honored at spawn time — this
        // dev environment's own `.bashrc` does exactly that.
        let mut pty =
            Pty::spawn(Some("/bin/sh"), Size { rows: 24, cols: 80 }, Some(&dir)).expect("spawn shell with a cwd");
        let mut reader = pty.try_clone_reader().expect("clone reader");
        pty.write(b"echo $PWD\n").expect("write input to shell");

        // Accumulates every chunk read for a fixed window, rather than
        // guessing "done" from the buffer's content — the shell's own
        // startup banner and its verbatim echo of our typed command both
        // legitimately show up before the command actually runs, so there's
        // no reliable partial-output marker to stop on early. `$PWD`'s
        // *expanded* value never appears in the command source itself
        // (only the literal text `$PWD` does), so plainly waiting long
        // enough and then checking for it is simpler and more robust than
        // trying to detect completion.
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });

        let mut seen = String::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
            match rx.recv_timeout(remaining) {
                Ok(chunk) => seen.push_str(&String::from_utf8_lossy(&chunk)),
                Err(_) => break,
            }
        }

        assert!(seen.contains(&needle), "expected $PWD output to contain {needle:?}, got: {seen:?}");
    }

    #[cfg(unix)]
    #[test]
    fn bash_shell_integration_reports_cwd_via_osc7_with_no_manual_write() {
        // The real end-to-end path: a genuine `bash` process, spawned
        // through the actual `crate::integration` injection (not a
        // hand-crafted OSC 7 sequence written by the test itself) — this
        // is what actually has to work for session save's cwd tracking to
        // mean anything on a real bash pane.
        //
        // Asserts only that *some* real absolute path gets reported, not
        // that it's exactly the spawned-into directory: the integration
        // script deliberately sources the real `~/.bashrc`/`.profile` (so
        // a user's own customizations still apply), and that file is free
        // to `cd` elsewhere itself — this repo's own dev machine's
        // `~/.bashrc` does exactly that. `spawns_the_shell_in_the_given_
        // starting_directory` (using `/bin/sh`, no dotfiles at all)
        // already covers "the `cwd` argument is honored"; this test's job
        // is only to prove the injected hook actually fires.
        let dir = std::env::temp_dir();
        let start_dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());

        let pty =
            Pty::spawn(Some("bash"), Size { rows: 24, cols: 80 }, Some(&start_dir)).expect("spawn bash with a cwd");
        let mut reader = pty.try_clone_reader().expect("clone reader");
        let mut screen = Screen::new(Size { rows: 24, cols: 80 });

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while screen.cwd().is_none() {
            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else { break };
            match rx.recv_timeout(remaining) {
                Ok(chunk) => screen.advance(&chunk),
                Err(_) => break,
            }
        }

        let reported = screen.cwd().expect("the injected OSC 7 hook should have reported some cwd by now");
        assert!(reported.is_absolute(), "expected an absolute path, got {reported:?}");
    }
}
