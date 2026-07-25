//! Session file: layout tree, window size, and per-pane cwd/group
//! membership — saved on quit, restored on next launch. Never restarts
//! whatever was running (CONOPS §5g): restore always spawns a fresh shell
//! into the saved cwd, nothing more.
//!
//! Deliberately a separate file from `config::Config`, not a section
//! inside it, even though it's stored right alongside it and follows the
//! exact same hand-editable-TOML conventions: this is written
//! automatically on every quit, and folding it into `config.toml` would
//! mean every quit also rewrites (and risks clobbering a concurrent
//! hand-edit of) the user's actual settings file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "session.toml";

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Session {
    pub window: WindowSize,
    pub layout: layout::SavedNode,
    /// One entry per pane, in the same left-to-right, depth-first order as
    /// the layout's own leaves (see `layout::Layout::panes`/
    /// `from_snapshot`) — position, not id, is what correlates a pane's
    /// saved state back to its place in the tree, since `PaneId`s don't
    /// survive a restart.
    pub panes: Vec<PaneState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct PaneState {
    pub cwd: PathBuf,
    pub group: Option<String>,
    /// `None` means "whatever the configured default shell was" (so
    /// restore keeps following the *current* default, even if it's
    /// changed since); `Some` an explicit override the pane was actually
    /// running (e.g. via "Swap shell") — see `PaneSession::shell`'s doc
    /// comment, which this mirrors exactly.
    pub shell: Option<String>,
}

impl Session {
    /// The session file's path: alongside `config::Config::default_path`,
    /// same directory, different filename.
    pub fn default_path() -> PathBuf {
        config::dir().join(FILE_NAME)
    }

    /// Loads a previously saved session. `None` if there isn't one yet (a
    /// fresh install, or the user simply hasn't quit with any panes open
    /// before) or if it can't be parsed (a corrupted write, or a format
    /// from an incompatible previous version) — either way, the caller
    /// falls back to a normal fresh start rather than erroring, the same
    /// "never crash on a bad file" convention `config::Config::load` uses.
    pub fn load(path: &Path) -> Option<Session> {
        let contents = std::fs::read_to_string(path).ok()?;
        match toml::from_str(&contents) {
            Ok(session) => Some(session),
            Err(err) => {
                eprintln!("session: failed to parse {}: {err}", path.display());
                None
            }
        }
    }

    /// Serializes and writes `self` to `path`, creating its parent
    /// directory first if needed.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let contents = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_loads_none() {
        let path = PathBuf::from("/nonexistent/definitely/not/a/real/path/session.toml");
        assert_eq!(Session::load(&path), None);
    }

    #[test]
    fn malformed_file_loads_none_not_a_crash() {
        let dir = std::env::temp_dir().join(format!("pain-session-test-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.toml");
        std::fs::write(&path, "this is not [valid toml").unwrap();

        assert_eq!(Session::load(&path), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("pain-session-test-save-{}", std::process::id()));
        let path = dir.join("nested").join("session.toml");

        let session = Session {
            window: WindowSize { width: 1024, height: 768 },
            layout: layout::SavedNode::Split {
                orientation: layout::Orientation::Horizontal,
                ratio: 0.6,
                first: Box::new(layout::SavedNode::Leaf),
                second: Box::new(layout::SavedNode::Leaf),
            },
            panes: vec![
                PaneState {
                    cwd: PathBuf::from("/home/will/project"),
                    group: Some("backend".to_string()),
                    shell: Some("wsl.exe".to_string()),
                },
                PaneState { cwd: PathBuf::from("/home/will"), group: None, shell: None },
            ],
        };

        session.save(&path).expect("save should create parent dirs and write the file");
        assert_eq!(Session::load(&path), Some(session));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_pane_state_missing_the_shell_field_still_parses() {
        // Forward compatibility: a session file written before `shell`
        // existed shouldn't fail to load just because of a field it
        // predates — `#[serde(default)]` should fill it in as `None`. Real
        // serialized output with the `shell` line stripped out, rather
        // than hand-typed TOML, so this doesn't depend on guessing
        // `SavedNode`'s exact enum serialization by hand.
        let session = Session {
            window: WindowSize { width: 800, height: 600 },
            layout: layout::SavedNode::Leaf,
            panes: vec![PaneState { cwd: PathBuf::from("/home/will"), group: None, shell: Some("bash".to_string()) }],
        };
        let toml_text = toml::to_string_pretty(&session).unwrap();
        let without_shell: String =
            toml_text.lines().filter(|line| !line.trim_start().starts_with("shell")).collect::<Vec<_>>().join("\n");
        assert!(!without_shell.contains("shell"), "test setup should actually remove the shell line");

        let dir = std::env::temp_dir().join(format!("pain-session-test-compat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.toml");
        std::fs::write(&path, without_shell).unwrap();

        let loaded = Session::load(&path).expect("should still parse despite the missing field");
        assert_eq!(loaded.panes[0].shell, None);
        assert_eq!(loaded.panes[0].cwd, PathBuf::from("/home/will"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
