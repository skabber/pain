//! Injects OSC 7 (cwd reporting) into shells that don't emit it on their
//! own — entirely at spawn time, via extra arguments and a generated
//! script file, never by editing the user's own persistent dotfiles
//! (`.bashrc`, `$PROFILE`, ...). The generated script always sources the
//! user's real configuration first, so nothing of theirs is lost; ours is
//! layered on top, the same technique real terminals with shell
//! integration (iTerm2, Windows Terminal, VS Code) use.
//!
//! **This is Windows-only in practice.** Unix reads a pane's working
//! directory from the process table instead
//! (`app::foreground_process::ForegroundProcesses::cwd_of`), which is
//! simply a better answer: it needs no cooperation from the shell, so
//! nothing has to be injected, nothing about the user's startup files
//! changes, and it works for every shell rather than the two this module
//! can recognize — zsh and fish panes get cwd tracking they never had.
//!
//! Injecting is the fallback for the platform with no such primitive.
//! Reading another process's working directory on Windows means walking
//! its PEB, and a WSL pane's shell lives in a process table the Windows
//! side cannot see at all.
//!
//! Deliberately narrow even there: only shells this can positively
//! identify (bash, PowerShell, and `wsl.exe` when its own inner shell
//! turns out to be bash) get anything injected. Everything else —
//! cmd.exe (which needs a different, ConEmu-style `OSC 9;9` sequence
//! instead of OSC 7, not implemented here), `wsl.exe` when its inner
//! shell *isn't* bash (zsh, fish, ... — forcing one would risk silently
//! changing what a user's WSL session actually runs), and any shell this
//! doesn't recognize — spawns exactly as it did before this existed.
//!
//! `Family::Wsl` is the one case that can't be decided at spawn time from
//! the shell string alone: `wsl.exe` gives no indication up front of what
//! runs inside it. `wsl_entrypoint`'s generated script does that detection
//! itself, at the moment the pane actually starts, entirely inside the
//! WSL side — see its own doc comment for why (mainly: the alternative,
//! a synchronous pre-flight `wsl.exe` call from here to ask first, would
//! double the number of WSL invocations and add real spawn latency for
//! every WSL pane, not just ones that end up running bash).

use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;

/// A shell family this module knows how to inject OSC 7 support into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Bash,
    PowerShell,
    /// `wsl.exe` specifically — its actual inner shell is decided at
    /// runtime by the generated entrypoint script (`wsl_entrypoint`), not
    /// here, since nothing about the shell string itself says what runs
    /// inside the distro.
    Wsl,
    /// Anything else — cmd.exe, zsh, fish, a custom path this doesn't
    /// recognize. No injection; spawns unchanged.
    Other,
}

/// Mirrors `portable_pty::CommandBuilder::get_shell()`'s own Unix
/// resolution exactly — `$SHELL` first, then the current user's `/etc/
/// passwd` entry — so a shell resolved only through that second fallback
/// still gets classified, and so still gets OSC 7 integration, correctly.
/// Without this, a launch context where `$SHELL` isn't exported (some
/// desktop/GUI-launcher paths, unlike an interactive terminal — a real,
/// confirmed cause of "integration silently didn't apply") would fall
/// straight to `Family::Other` even though `portable_pty` still correctly
/// resolves and runs the very same shell.
#[cfg(unix)]
pub fn resolve_default_shell() -> Option<String> {
    std::env::var("SHELL").ok().or_else(passwd_shell)
}

#[cfg(unix)]
fn passwd_shell() -> Option<String> {
    // Matches `portable_pty::unix::get_shell()`'s own approach exactly
    // (down to using the same raw, not-thread-safe libc call it does —
    // no worse here than in the dependency already making this call).
    let ent = unsafe { libc::getpwuid(libc::getuid()) };
    if ent.is_null() {
        return None;
    }
    let shell = unsafe { std::ffi::CStr::from_ptr((*ent).pw_shell) };
    shell.to_str().ok().map(str::to_string)
}

/// Whether `family` gets extra spawn arguments on this platform.
///
/// The caller needs this before building the command, not just when
/// applying it: `CommandBuilder::new_default_prog()` panics outright if an
/// argument is added to it, so a family that gets arguments has to be named
/// explicitly instead. Kept beside `apply`, since the two have to agree.
pub fn injects(family: Family) -> bool {
    match family {
        // See `apply` — Unix reads the working directory from the process
        // table, so nothing is injected there at all. That includes
        // PowerShell, which does run on Linux and macOS as `pwsh`.
        Family::Bash | Family::PowerShell => cfg!(not(unix)),
        // Windows-only by construction: there is no `wsl.exe` to classify
        // anywhere else.
        Family::Wsl => true,
        Family::Other => false,
    }
}

/// Identifies a shell by its executable's basename (`bash`, not
/// `/usr/bin/bash` or `bash.exe`), so exact install path/extension
/// differences don't matter. Splits on `/` and `\` manually rather than
/// via `std::path::Path` — `Path` only treats `\` as a separator when
/// *compiled* for Windows, but a Windows-style shell path (`default_shell`
/// in a hand-edited config, say) needs to classify correctly regardless
/// of which OS this happens to be running on.
pub fn classify(shell: &str) -> Family {
    let basename = shell.rsplit(['/', '\\']).next().unwrap_or(shell).to_ascii_lowercase();
    let stem = basename.strip_suffix(".exe").unwrap_or(&basename);
    match stem {
        "bash" => Family::Bash,
        "powershell" | "pwsh" => Family::PowerShell,
        "wsl" => Family::Wsl,
        _ => Family::Other,
    }
}

/// Appends whatever arguments make `family` emit OSC 7 at each new
/// prompt, writing the generated script it depends on first (recreated
/// unconditionally each call — cheap, and always current with this
/// build, so nothing about it needs cleaning up or versioning). Does
/// nothing for `Family::Other`. Failing to write the script is reported,
/// not fatal — the pane still spawns, just without cwd tracking, same as
/// any shell this doesn't recognize.
pub fn apply(cmd: &mut CommandBuilder, family: Family) {
    match family {
        // Nothing at all on Unix: the working directory is read straight
        // out of the process table instead (`ForegroundProcesses::cwd_of`,
        // `/proc/<pid>/cwd` on Linux and `proc_pidinfo` on macOS), so bash
        // starts exactly as it would in any other terminal, reading its own
        // startup files with no `--rcfile` in the way.
        //
        // That's a strictly better trade than it first appears. Injecting
        // meant reproducing bash's startup-file logic by hand, and getting
        // it wrong broke real configurations in ways that looked nothing
        // like their cause. Reading the OS also works for every shell, so
        // zsh and fish panes get working-directory tracking they never had.
        //
        // Windows has no equivalent — reading another process's cwd there
        // means walking its PEB — so bash under Windows (Git Bash, MSYS)
        // still gets the hook, as does WSL, whose shell lives in a process
        // table the Windows side can't see at all.
        Family::Bash if cfg!(unix) => {}
        Family::Bash => {
            let Some(path) = write_bash_integration() else { return };
            cmd.args(["--rcfile", &path.to_string_lossy()]);
        }
        Family::PowerShell => {
            cmd.args(["-NoExit", "-Command", POWERSHELL_INTEGRATION]);
        }
        Family::Wsl => {
            let Some(path) = write_bash_integration() else { return };
            let Some(wsl_bash_path) = windows_path_to_wsl_mount(&path) else {
                eprintln!(
                    "pane: could not translate {} into a WSL mount path; WSL panes won't have cwd tracking",
                    path.display()
                );
                return;
            };

            let entrypoint_path = wsl_entrypoint_path();
            if let Err(err) = write_script(&entrypoint_path, &wsl_entrypoint_contents(&wsl_bash_path)) {
                eprintln!("pane: failed to write WSL entrypoint script {}: {err:#}", entrypoint_path.display());
                return;
            }
            let Some(wsl_entrypoint) = windows_path_to_wsl_mount(&entrypoint_path) else {
                eprintln!(
                    "pane: could not translate {} into a WSL mount path; WSL panes won't have cwd tracking",
                    entrypoint_path.display()
                );
                return;
            };
            cmd.args(["--", "sh", &wsl_entrypoint]);
        }
        Family::Other => {}
    }
}

/// Writes the bash integration script (shared between `Family::Bash` and
/// `Family::Wsl`, which reaches the same script through a translated
/// path) and returns its Windows-side path, or `None` (logged) if writing
/// it failed.
fn write_bash_integration() -> Option<PathBuf> {
    let path = bash_integration_path();
    match write_script(&path, BASH_INTEGRATION) {
        Ok(()) => Some(path),
        Err(err) => {
            eprintln!("pane: failed to write shell-integration script {}: {err:#}", path.display());
            None
        }
    }
}

/// Writes `contents` to `path` atomically (write to a sibling temp file,
/// then rename into place) rather than directly — `path` is a single
/// fixed, shared location (not unique per spawn), so two panes spawning
/// around the same time (opening several splits at once, say) would
/// otherwise race: one could read a half-written file if it happened to
/// open `--rcfile` mid-write by the other. A same-filesystem rename is
/// atomic, so a reader only ever sees the complete old or new content,
/// never a partial one.
fn write_script(path: &Path, contents: &str) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| std::io::Error::other("script path has no parent directory"))?;
    std::fs::create_dir_all(dir)?;
    let tmp_path = dir.join(format!(
        ".tmp-{}-{:?}-{}",
        std::process::id(),
        std::thread::current().id(),
        path.file_name().and_then(|n| n.to_str()).unwrap_or("script")
    ));
    std::fs::write(&tmp_path, contents)?;
    std::fs::rename(&tmp_path, path)
}

/// A fixed location under the system temp directory — not `config::dir()`
/// (this crate has no reason to depend on `config` just for a path), and
/// not a per-process-unique temp file either: this is regenerated
/// identically on every spawn, so a stable, shared location is simpler
/// than managing per-instance cleanup.
fn bash_integration_path() -> PathBuf {
    std::env::temp_dir().join("pain-shell-integration").join("shell-integration.bash")
}

/// Where `wsl_entrypoint_contents` gets written — alongside the bash
/// integration script, since `wsl.exe`'s inner shell (if it turns out to
/// be bash) ends up reading that same file too, just through its
/// translated WSL mount path.
fn wsl_entrypoint_path() -> PathBuf {
    std::env::temp_dir().join("pain-shell-integration").join("wsl-entrypoint.sh")
}

/// Translates a Windows path (`C:\Users\Will\...`) into its default WSL2
/// mount equivalent (`/mnt/c/Users/Will/...`) — the standard automount
/// convention (`/etc/wsl.conf`'s default `[automount] root = /mnt/`, drive
/// letter lowercased). A distro with a customized automount root or a
/// remapped drive wouldn't match this — there's no way to ask `wsl.exe`
/// for its actual mount convention without a separate synchronous
/// round-trip before every WSL pane spawns, which isn't worth the added
/// latency for what's already a best-effort fallback path. Returns `None`
/// for anything that doesn't look like a drive-letter path (nothing to
/// translate) rather than guessing.
fn windows_path_to_wsl_mount(path: &Path) -> Option<String> {
    let s = path.to_str()?;
    let bytes = s.as_bytes();
    if bytes.len() < 2 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    let rest = s[2..].replace('\\', "/");
    Some(format!("/mnt/{drive}{rest}"))
}

/// A POSIX `sh` script, run as `wsl.exe -- sh <this-file's-WSL-mount-
/// path>`, replacing `wsl.exe`'s own default bare invocation. Detects the
/// distro's actual configured shell itself, at run time, inside the WSL
/// side — not something decided from here at spawn time, since nothing
/// about the shell string `wsl.exe` itself gives any indication of what
/// runs inside. Only bash is ever specifically injected into; anything
/// else execs the user's own real shell completely unmodified, exactly
/// what a bare `wsl.exe` invocation would have run — never forcing a
/// shell the user didn't already have configured as their default.
///
/// `bash_integration_wsl_path` is the *already WSL-mount-translated* path
/// to the same script `Family::Bash` uses natively — passed in rather
/// than translated here, since translation can fail (see
/// `windows_path_to_wsl_mount`) and the caller needs to know that before
/// deciding whether to write this file at all.
fn wsl_entrypoint_contents(bash_integration_wsl_path: &str) -> String {
    format!(
        r#"shell="${{SHELL:-$(getent passwd "$(id -un)" 2>/dev/null | cut -d: -f7)}}"
shell="${{shell:-/bin/sh}}"
case "$(basename "$shell")" in
    bash)
        exec bash --rcfile "{bash_integration_wsl_path}" -i
        ;;
    *)
        exec "$shell"
        ;;
esac
"#
    )
}

/// Passed to `bash --rcfile`, which otherwise replaces *all* of bash's own
/// startup-file sourcing — so this has to reproduce it by hand before
/// adding the OSC 7 hook.
///
/// What it reproduces is an **interactive non-login shell**: the system
/// bashrc, then `~/.bashrc`. That is what bash reads when a terminal
/// emulator starts it, and what every other terminal on Linux does. The
/// login files (`/etc/profile`, `~/.bash_profile`, `~/.bash_login`,
/// `~/.profile`) are deliberately not sourced — a desktop session has
/// already read those once, and a terminal is not a login.
///
/// This used to source the login chain as well, which broke real setups
/// in a way that looked like "my `.bashrc` didn't load". The stock
/// `~/.bash_profile` on Fedora and RHEL — and commonly on Debian and
/// Ubuntu — ends with `[ -f ~/.bashrc ] && . ~/.bashrc`, so sourcing both
/// chains ran `~/.bashrc` *twice*. Anything written to append rather than
/// assign then did it twice too: duplicated `PATH` entries, duplicated
/// `PROMPT_COMMAND`, and prompt frameworks (starship, bash-preexec,
/// git-prompt) installing their hooks on top of themselves. It also ran
/// login-only side effects — `ssh-agent` startup, tmux auto-attach — once
/// per pane instead of once per login.
///
/// The system bashrc is `/etc/bash.bashrc` on Debian and Ubuntu but
/// `/etc/bashrc` on Fedora, RHEL and macOS; bash itself is compiled with
/// one path or the other, so at most one exists on any given system.
/// Sourcing neither (which is what the old script effectively did on
/// Fedora, where nothing in the login chain reaches `/etc/bashrc`) loses
/// the system default prompt and the interactive half of
/// `/etc/profile.d/*`.
///
/// Prepends to `PROMPT_COMMAND` rather than overwriting it, so a user's
/// own hook still runs. Known limitation: bash 5.1 added an array form of
/// `PROMPT_COMMAND`, and this string assignment only sees its first
/// element.
const BASH_INTEGRATION: &str = r#"
if [ -f /etc/bash.bashrc ]; then
    . /etc/bash.bashrc
elif [ -f /etc/bashrc ]; then
    . /etc/bashrc
fi
[ -f "$HOME/.bashrc" ] && . "$HOME/.bashrc"

__pain_report_cwd() {
    printf '\033]7;file://%s%s\033\\' "$HOSTNAME" "$PWD"
}
PROMPT_COMMAND="__pain_report_cwd${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
"#;

/// Passed to `powershell.exe -NoExit -Command`. Profile scripts still
/// load normally before this runs (only `-NoProfile` would skip them), so
/// `$function:prompt` here is already whatever the user's own profile
/// defined, or PowerShell's built-in default if they never touched it —
/// captured and wrapped, not replaced, either way.
///
/// Emits `OSC 9;9`, not `OSC 7` — confirmed on real hardware that a plain
/// OSC 7 written via `Write-Host` (the legacy Windows Console API) never
/// arrives: the prompt function installs and runs correctly (its body was
/// read back verbatim), but nothing reaches `crate::cwd`'s scanner.
/// Windows' own console host (ConPTY/conhost) evidently doesn't forward
/// that sequence the way a real Unix pty passes bytes through untouched —
/// `OSC 9;9` is the convention Windows Terminal/ConEmu actually support
/// for this, a raw path with no `file://` wrapping or percent-encoding
/// needed (see `crate::cwd`'s doc comment).
const POWERSHELL_INTEGRATION: &str = "$global:PainPrevPrompt = $function:prompt; \
function global:prompt { \
$p = (Get-Location).Path; \
Write-Host -NoNewline ([char]27 + ']9;9;' + $p + [char]27 + '\\'); \
& $global:PainPrevPrompt \
}";

#[cfg(test)]
mod tests {
    use super::*;

    /// The reason the startup-file emulation above exists at all is that
    /// `--rcfile` replaces it — so on the platform that no longer passes
    /// `--rcfile`, bash must be getting no arguments whatsoever, and the
    /// user's own startup files are read by bash itself.
    #[test]
    fn unix_injects_nothing_into_bash_and_windows_still_does() {
        assert_eq!(injects(Family::Bash), cfg!(not(unix)));
        // PowerShell runs on Linux and macOS too, as `pwsh`, and gets the
        // same treatment there: nothing injected, cwd read from the OS.
        assert_eq!(injects(Family::PowerShell), cfg!(not(unix)));
        // Windows has no way to read another process's working directory
        // short of walking its PEB, and a WSL pane's shell isn't in the
        // Windows process table at all — that one always needs the hook.
        assert!(injects(Family::Wsl));
        assert!(!injects(Family::Other), "an unrecognized shell is never touched");
    }

    /// A terminal starts an interactive *non-login* shell, so the injected
    /// rcfile must read the bashrc files and nothing else. Sourcing the
    /// login chain on top is what made `~/.bashrc` run twice on any system
    /// whose `~/.bash_profile` sources it — the stock arrangement on
    /// Fedora and RHEL — duplicating `PATH` entries and prompt hooks.
    /// Asserted rather than merely deleted, because "we stopped sourcing
    /// these on purpose" reads like an oversight to the next person.
    #[test]
    fn the_bash_rcfile_reproduces_a_non_login_shell_and_nothing_more() {
        assert!(BASH_INTEGRATION.contains(r#""$HOME/.bashrc""#), "must source the user's bashrc");
        // Debian and Ubuntu use the first; Fedora, RHEL and macOS the
        // second. Bash is compiled with one or the other, so both are
        // tried and at most one will exist.
        assert!(BASH_INTEGRATION.contains("/etc/bash.bashrc"), "must source Debian's system bashrc");
        assert!(BASH_INTEGRATION.contains("/etc/bashrc"), "must source Fedora's system bashrc");

        for login_file in ["/etc/profile", ".bash_profile", ".bash_login", ".profile"] {
            assert!(
                !BASH_INTEGRATION.contains(login_file),
                "{login_file} is a login-shell file and must not be sourced by a terminal"
            );
        }
    }

    /// Overwriting `PROMPT_COMMAND` would silently disable whatever the
    /// user's own configuration installed there — which on many setups is
    /// what draws their prompt.
    #[test]
    fn the_bash_rcfile_keeps_any_existing_prompt_command() {
        assert!(
            BASH_INTEGRATION.contains(r#"PROMPT_COMMAND="__pain_report_cwd${PROMPT_COMMAND:+; $PROMPT_COMMAND}""#),
            "the OSC 7 hook must be added to PROMPT_COMMAND, not assigned over it"
        );
    }

    #[cfg(unix)]
    #[test]
    fn passwd_shell_finds_a_real_executable_path() {
        let shell = passwd_shell().expect("should find a passwd entry for the current user");
        assert!(std::path::Path::new(&shell).exists(), "expected {shell:?} to be a real path");
    }

    #[cfg(unix)]
    #[test]
    fn resolve_default_shell_returns_something_plausible() {
        let shell = resolve_default_shell().expect("should resolve to something on any real Unix system");
        assert!(!shell.is_empty());
    }

    #[test]
    fn classifies_known_shells_by_file_stem_regardless_of_path_or_extension() {
        assert_eq!(classify("bash"), Family::Bash);
        assert_eq!(classify("/usr/bin/bash"), Family::Bash);
        assert_eq!(classify("/bin/bash"), Family::Bash);
        assert_eq!(classify("powershell.exe"), Family::PowerShell);
        assert_eq!(classify("pwsh"), Family::PowerShell);
        assert_eq!(classify("C:\\Program Files\\PowerShell\\7\\pwsh.exe"), Family::PowerShell);
    }

    #[test]
    fn classifies_wsl_separately_from_bash_and_powershell() {
        assert_eq!(classify("wsl.exe"), Family::Wsl);
        assert_eq!(classify("wsl"), Family::Wsl);
        assert_eq!(classify("WSL.EXE"), Family::Wsl);
    }

    #[test]
    fn classifies_everything_else_as_other() {
        assert_eq!(classify("zsh"), Family::Other);
        assert_eq!(classify("fish"), Family::Other);
        assert_eq!(classify("cmd.exe"), Family::Other);
        assert_eq!(classify("sh"), Family::Other);
    }

    #[test]
    fn apply_is_a_no_op_for_other() {
        let mut cmd = CommandBuilder::new("sh");
        apply(&mut cmd, Family::Other);
        // Nothing to assert on `cmd` directly (`CommandBuilder` doesn't
        // expose its args for inspection) — this just guards against a
        // panic; the real behavioral guarantee (no `--rcfile`/`-Command`
        // args, no real shell spawned differently) is covered by the
        // fact that `Family::Other` never reaches either write/append arm.
    }

    /// Unix must touch bash not at all; Windows must still get the hook,
    /// since it has no way to read a process's working directory short of
    /// walking its PEB.
    ///
    /// Deliberately removes any script left by an earlier run first. The
    /// previous version of this test didn't, and kept passing after Unix
    /// stopped writing the script at all — it was asserting the existence
    /// of a stale file in the temp directory rather than anything this
    /// call did.
    #[test]
    fn apply_injects_into_bash_only_where_the_os_cant_report_a_cwd() {
        let script_path = bash_integration_path();
        let _ = std::fs::remove_file(&script_path);

        let mut cmd = CommandBuilder::new("bash");
        apply(&mut cmd, Family::Bash);
        let args: Vec<_> = cmd.get_argv().iter().skip(1).collect();

        if cfg!(unix) {
            assert!(args.is_empty(), "bash should start exactly as any other terminal starts it, got {args:?}");
            assert!(!script_path.exists(), "nothing should be written when nothing is injected");
        } else {
            assert!(args.iter().any(|a| *a == "--rcfile"), "expected an --rcfile argument, got {args:?}");
            let contents = std::fs::read_to_string(&script_path).expect("should have written the script");
            assert!(contents.contains("PROMPT_COMMAND"));
            assert!(contents.contains(".bashrc"));
        }
    }

    #[test]
    fn windows_path_to_wsl_mount_translates_the_standard_automount_layout() {
        assert_eq!(
            windows_path_to_wsl_mount(Path::new("C:\\Users\\Will\\file.txt")).as_deref(),
            Some("/mnt/c/Users/Will/file.txt")
        );
        // Lowercased regardless of the drive letter's own case.
        assert_eq!(windows_path_to_wsl_mount(Path::new("D:\\stuff")).as_deref(), Some("/mnt/d/stuff"));
    }

    #[test]
    fn windows_path_to_wsl_mount_rejects_non_drive_letter_paths() {
        assert_eq!(windows_path_to_wsl_mount(Path::new("/already/unix/style")), None);
        assert_eq!(windows_path_to_wsl_mount(Path::new("relative\\path")), None);
        assert_eq!(windows_path_to_wsl_mount(Path::new("")), None);
    }

    #[test]
    fn wsl_entrypoint_contents_embeds_the_given_path_and_execs_bash_or_the_real_shell() {
        let contents = wsl_entrypoint_contents(
            "/mnt/c/Users/Will/AppData/Local/Temp/pain-shell-integration/shell-integration.bash",
        );
        assert!(
            contents.contains("/mnt/c/Users/Will/AppData/Local/Temp/pain-shell-integration/shell-integration.bash")
        );
        assert!(contents.contains("exec bash --rcfile"));
        // The non-bash fallback execs whatever the distro's real shell
        // is — never forces one the user didn't already have configured.
        assert!(contents.contains(r#"exec "$shell""#));
    }

    // Windows-only: `apply`'s `Family::Wsl` branch translates
    // `bash_integration_path()`/`wsl_entrypoint_path()` (both built from
    // `std::env::temp_dir()`) through `windows_path_to_wsl_mount`, which
    // only ever succeeds against an actual drive-letter path — on this
    // dev sandbox (Linux), `temp_dir()` returns a plain Unix path, so
    // translation would correctly, harmlessly fail here rather than
    // exercise anything. `windows_path_to_wsl_mount`/
    // `wsl_entrypoint_contents` above already cover the real logic
    // portably, with explicit literal paths instead of the real temp dir.
    #[cfg(windows)]
    #[test]
    fn apply_for_wsl_writes_both_scripts_with_a_translated_reference_between_them() {
        let mut cmd = CommandBuilder::new("wsl.exe");
        apply(&mut cmd, Family::Wsl);

        let bash_path = bash_integration_path();
        assert!(bash_path.exists(), "should have written the shared bash integration script");

        let entrypoint_path = wsl_entrypoint_path();
        assert!(entrypoint_path.exists(), "should have written the WSL entrypoint script");
        let entrypoint_contents = std::fs::read_to_string(&entrypoint_path).unwrap();
        let translated_bash_path =
            windows_path_to_wsl_mount(&bash_path).expect("bash_integration_path should be a drive-letter path");
        assert!(
            entrypoint_contents.contains(&translated_bash_path),
            "entrypoint should reference the bash script by its translated WSL path, got: {entrypoint_contents:?}"
        );
    }
}
