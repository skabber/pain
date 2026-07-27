//! Resolves a pane's working directory for session save, falling back
//! through progressively weaker signals (CONOPS §5g) when a stronger one
//! isn't available: the shell's own OSC 7 report (`pane::Screen::cwd`),
//! then `crate::foreground_process::ForegroundProcesses::cwd_of`'s
//! OS-level lookup, and finally the user's home directory if even that
//! fails (the process already exited, or the OS doesn't expose this).
//!
//! Which of the first two actually answers is a platform split. On Unix
//! the OS-level lookup does all the real work — nothing injects OSC 7
//! there any more (see `pane::integration`), so a report only arrives if
//! the user's own shell configuration emits one, and it's honoured when it
//! does because it's the shell's own logical path, symlinks and all. On
//! Windows the OS-level lookup is the weak one, and injected OSC 7 (or
//! `OSC 9;9`) is what usually answers.
//!
//! Not to be confused with `pane::cwd`, which is the OSC 7 *scanner*
//! itself (parsing the escape sequence out of raw PTY bytes) — this is
//! just the fallback chain built on top of it and the OS-level lookup.

use std::path::{Path, PathBuf};

/// Picks the best available cwd from what's known: `osc7` (the shell's own
/// report, if any) wins outright; otherwise `os_level` (an OS process-table
/// lookup); otherwise the user's home directory, which is assumed to
/// always exist as a last resort.
pub fn resolve(osc7: Option<&Path>, os_level: Option<PathBuf>) -> PathBuf {
    osc7.map(Path::to_path_buf).or(os_level).unwrap_or_else(home_dir)
}

/// The user's home directory, or `.` (the process's own current
/// directory) in the unlikely case the OS can't report one at all —
/// always returns something usable rather than `None`, since this is
/// already the last fallback in the chain above.
fn home_dir() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc7_wins_when_present_regardless_of_os_level() {
        let osc7 = Path::new("/from/osc7");
        let os_level = Some(PathBuf::from("/from/os"));
        assert_eq!(resolve(Some(osc7), os_level), PathBuf::from("/from/osc7"));
    }

    #[test]
    fn os_level_is_used_when_osc7_is_absent() {
        let os_level = Some(PathBuf::from("/from/os"));
        assert_eq!(resolve(None, os_level), PathBuf::from("/from/os"));
    }

    #[test]
    fn home_directory_is_the_final_fallback() {
        let resolved = resolve(None, None);
        assert!(!resolved.as_os_str().is_empty(), "should always return something usable, never an empty path");
    }
}
