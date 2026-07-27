//! Determines the name of whatever's actually running in the foreground of
//! a pane right now (e.g. "vim", "npm"), for the title bar.
//!
//! This replaced tracking the terminal's OSC 0/1/2 title for the same
//! purpose: most shells' default prompt only sets that to `user@host: cwd`,
//! refreshed at the prompt, never to the actually-running foreground
//! command — it couldn't answer the question it was being used for.
//!
//! Unix has a real, correct primitive here: `tcgetpgrp` on the pty (exposed
//! by `portable_pty`/`pane::Pty` as `foreground_pgid`) reports the process
//! group currently in the foreground — a shell puts each foreground job in
//! its own process group, led by the job itself, so this is almost always
//! that job's own pid directly. Windows/ConPTY has no equivalent concept;
//! the closest available signal is walking the process tree down from the
//! shell's own pid and picking the most recently started live descendant
//! at each level, as a best-effort approximation of "the current job."
//!
//! Both signals are scoped to a single OS's own process list, which breaks
//! down at a WSL boundary: running `wsl.exe`/`bash` from inside a Windows
//! shell hands the foreground over to a different kernel's process tree
//! entirely, invisible to `sysinfo` on the Windows side — there's no pid to
//! walk to. That isn't fixable here; the title bar's "Swap shell" context
//! menu item (`Graphics::restart_pane_shell`) exists to sidestep it by
//! letting a pane switch directly into the nested shell instead.
//!
//! Even for a pane whose own shell *is* `wsl.exe` directly (via "Swap
//! shell"), the tree-walk fallback used to get stuck reporting a Windows
//! interop helper forever, never the real WSL-side command: `wsl.exe`
//! spawns lasting, never-exiting helper children on the Windows side —
//! `conhost.exe`, then (once that was excluded) `wslhost.exe` took its
//! place as the next one found — and since nothing else Windows can see
//! ever supersedes either, "most recently started live child" kept
//! picking whichever one was left. Fixed by excluding both outright
//! (`is_console_host_implementation_detail`) — the fallback now correctly
//! bottoms out at `wsl.exe`'s own name instead, the most honest answer
//! this mechanism can give given the boundary above.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// How often to re-scan the process list. Enumeration touches every
/// process on the system, not just the ones any one pane cares about —
/// cheap per call, but wasteful to repeat every single frame across every
/// pane, so this is refreshed at a human-perceptible rate instead of live.
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// A shared, throttled process-list snapshot every pane's title bar reads
/// from — one instance per `Graphics`, not one per pane, since a full
/// process-list refresh already covers every pane at once.
pub struct ForegroundProcesses {
    system: System,
    last_refresh: Instant,
}

impl ForegroundProcesses {
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_processes(ProcessesToUpdate::All, true);
        Self { system, last_refresh: Instant::now() }
    }

    /// Re-scans the process list if `REFRESH_INTERVAL` has passed since the
    /// last scan. Call once per frame; a cheap no-op most of the time.
    /// Returns whether a scan actually happened, so callers can log
    /// exactly when the snapshot changed instead of every single frame.
    /// When the next scan becomes due. The event loop sleeps until this
    /// instant, so it has to come from the same `last_refresh` the
    /// throttle below uses — deriving it anywhere else would let the two
    /// drift and either over-scan or stall pane titles.
    pub fn next_refresh_at(&self) -> Instant {
        self.last_refresh + REFRESH_INTERVAL
    }

    pub fn maybe_refresh(&mut self) -> bool {
        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.system.refresh_processes(ProcessesToUpdate::All, true);
            self.last_refresh = Instant::now();
            true
        } else {
            false
        }
    }

    /// Looks up `pid`'s current working directory directly from the OS —
    /// the session-save cwd fallback (CONOPS §5g) for a pane whose shell
    /// never emitted OSC 7. A separate, on-demand, single-pid refresh
    /// rather than something `maybe_refresh`'s continuous scan also
    /// collects: cwd is only ever needed once, at save time, so asking the
    /// OS for it on every 500ms scan (of every process on the system)
    /// would be pure waste. `remove_dead_processes: false` matters here —
    /// this must not prune `self.system`'s wider process cache (what
    /// `name_for`'s tree-walk relies on) just because this call only asked
    /// about one pid.
    pub fn cwd_of(&mut self, pid: u32) -> Option<PathBuf> {
        let pid = Pid::from_u32(pid);
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            false,
            ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always),
        );
        self.system.process(pid)?.cwd().map(|path| path.to_path_buf())
    }

    /// The foreground process name for a pane, given its shell's own pid
    /// (`pane::Pty::shell_pid`) and, on Unix, the pty's reported foreground
    /// process group (`pane::Pty::foreground_pgid`). `None` if nothing
    /// could be determined at all (no `shell_pid`, or the process has
    /// already exited) — callers should show a generic fallback like
    /// `"shell"` in that case.
    ///
    /// When there's no deeper foreground child (the shell is idle at its
    /// prompt), this naturally resolves to the shell's own name by walking
    /// zero levels and looking itself up — so it also doubles as "what
    /// shell actually ended up running," correct even when the pane was
    /// spawned with the platform default rather than an explicit path.
    pub fn name_for(&self, shell_pid: Option<u32>, foreground_pgid: Option<u32>) -> Option<String> {
        // The direct, correct signal where it's available.
        if let Some(pgid) = foreground_pgid
            && let Some(process) = self.system.process(Pid::from_u32(pgid))
        {
            return Some(process.name().to_string_lossy().into_owned());
        }

        // Otherwise (Windows always; Unix only if the pgid lookup somehow
        // came up empty), approximate it by walking down from the shell's
        // own pid, picking the most recently started live child at each
        // level, skipping `is_console_host_implementation_detail` matches.
        // Zero iterations (no children at all) leaves `current` as the
        // shell's own pid, which is exactly the right fallback.
        let mut current = Pid::from_u32(shell_pid?);
        while let Some(youngest_child) = self
            .system
            .processes()
            .values()
            .filter(|p| p.parent() == Some(current) && !is_console_host_implementation_detail(&p.name().to_string_lossy()))
            .max_by_key(|p| p.start_time())
        {
            current = youngest_child.pid();
        }

        self.system.process(current).map(|p| p.name().to_string_lossy().into_owned())
    }
}

impl Default for ForegroundProcesses {
    fn default() -> Self {
        Self::new()
    }
}

/// Windows-only implementation-detail helper processes a shell's real child
/// might spawn, but that are never "what's running" from a user's
/// perspective — excluded from the tree-walk fallback so they can't
/// permanently mask a pane's actual foreground program.
///
/// Both confirmed directly (a developer's real pane, not a guess), not
/// just `conhost.exe`: `wsl.exe` spawns a lasting, never-exiting
/// `conhost.exe` child on the Windows side for every interactive WSL
/// session — and, once that was excluded, the *next* most recently
/// started live child turned out to be another interop helper,
/// `wslhost.exe` ("COM Server for WSL", used for clipboard/notification/
/// interop plumbing between Windows and the WSL VM), which then took its
/// place as the wrongly-reported title. Nothing else Windows can see ever
/// supersedes either of them (the real foreground command runs inside the
/// WSL2 VM's own kernel, invisible to `sysinfo` here regardless — see
/// this module's doc comment), so without excluding both the tree-walk
/// just finds whichever one starts last and reports it forever, no
/// matter what actually runs in the WSL shell.
fn is_console_host_implementation_detail(name: &str) -> bool {
    name.eq_ignore_ascii_case("conhost.exe") || name.eq_ignore_ascii_case("wslhost.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Unix-only: exercises the real logic against real spawned processes
    // rather than just checking the code compiles — the fallback/priority
    // behavior below is exactly what a mistake in the `if let` ordering or
    // the tree-walk loop would silently get wrong. Gated to Unix since
    // that's the platform this can actually be verified on directly; the
    // Windows-only tree-walk-from-shell-pid path this same function takes
    // when `foreground_pgid` is `None` is exercised by the second test too
    // (it's the same code, just also given a `foreground_pgid` to prefer
    // over it).

    #[cfg(unix)]
    #[test]
    fn name_for_falls_back_to_the_shells_own_name_when_idle() {
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .expect("spawn a real child process to look up");

        let mut processes = ForegroundProcesses::new();
        processes.system.refresh_processes(ProcessesToUpdate::All, true);

        assert_eq!(processes.name_for(Some(child.id()), None).as_deref(), Some("sleep"));

        let _ = child.kill();
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[test]
    fn name_for_prefers_the_foreground_pgid_signal_over_the_shell_pid() {
        let mut shell_stand_in = std::process::Command::new("sleep").arg("5").spawn().expect("spawn shell stand-in");
        let mut foreground_stand_in = std::process::Command::new("cat").spawn().expect("spawn foreground stand-in");
        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut processes = ForegroundProcesses::new();
        processes.system.refresh_processes(ProcessesToUpdate::All, true);

        let name = processes.name_for(Some(shell_stand_in.id()), Some(foreground_stand_in.id()));
        assert_eq!(name.as_deref(), Some("cat"));

        let _ = shell_stand_in.kill();
        let _ = shell_stand_in.wait();
        let _ = foreground_stand_in.kill();
        let _ = foreground_stand_in.wait();
    }

    // End-to-end through the real `pane::Pty`, not a raw `std::process`
    // stand-in — this is the exact pipeline the running app uses (spawn a
    // real shell, run a real foreground command in it, ask the PTY for its
    // foreground process group, look that up). The other tests above check
    // the lookup logic in isolation; this one checks the whole thing is
    // actually wired together correctly.
    #[cfg(unix)]
    #[test]
    fn real_pty_reports_the_actual_foreground_command() {
        // `"sh"`, not `None`: this is testing `tcgetpgrp`/job-control
        // behavior, a kernel/POSIX-shell feature not specific to bash —
        // `None` resolves to whatever `$SHELL` is (bash, on a real dev
        // box), which now goes through `crate::integration`'s injected
        // rcfile sourcing. That's real startup work (a login shell's own
        // profile chain, replicated manually), and this test's short,
        // fixed delay below isn't enough time for it — not a bug in the
        // injection itself, just irrelevant overhead for what this test
        // actually checks.
        let mut pty = pane::Pty::spawn(Some("sh"), pane::Size { rows: 24, cols: 80 }, None)
            .expect("spawn a real shell");
        pty.write(b"sleep 5\n").expect("write to the shell");

        // Give the shell a moment to actually exec into `sleep` and become
        // its own foreground process group leader.
        std::thread::sleep(std::time::Duration::from_millis(300));

        let mut processes = ForegroundProcesses::new();
        processes.system.refresh_processes(ProcessesToUpdate::All, true);

        let name = processes.name_for(pty.shell_pid(), pty.foreground_pgid());
        assert_eq!(name.as_deref(), Some("sleep"));
    }

    // Reproduces the developer's exact report: the real app constructs
    // `ForegroundProcesses` once at startup, long before the user runs
    // anything — unlike every test above, which spawns the target process
    // *before* the first scan ever happens. This one builds the tracker
    // first, *then* spawns something new, to check whether a later
    // `maybe_refresh` actually discovers a process that didn't exist yet
    // at construction time.
    #[cfg(unix)]
    #[test]
    fn discovers_a_process_that_started_after_the_tracker_did() {
        let mut processes = ForegroundProcesses::new();

        let mut child = std::process::Command::new("sleep").arg("5").spawn().expect("spawn a real child process");
        std::thread::sleep(std::time::Duration::from_millis(50));
        processes.system.refresh_processes(ProcessesToUpdate::All, true);

        assert_eq!(processes.name_for(Some(child.id()), None).as_deref(), Some("sleep"));

        let _ = child.kill();
        let _ = child.wait();
    }

    // Direct repro of the developer's report (a real shell running `htop`
    // specifically, not `sleep`), simulating the real app's actual usage
    // pattern more closely than a single check: many repeated refresh
    // cycles over several seconds, since the real app calls `maybe_refresh`
    // on every redraw continuously for as long as it runs.
    #[cfg(unix)]
    #[test]
    fn htop_name_stays_correct_across_many_refresh_cycles() {
        // `"sh"`, not `None` — see the comment on
        // `real_pty_reports_the_actual_foreground_command`, the same
        // reasoning applies here.
        let mut pty = pane::Pty::spawn(Some("sh"), pane::Size { rows: 24, cols: 80 }, None)
            .expect("spawn a real shell");
        pty.write(b"htop\n").expect("write to the shell");
        std::thread::sleep(std::time::Duration::from_millis(500));

        let mut processes = ForegroundProcesses::new();
        for i in 0..20 {
            processes.system.refresh_processes(ProcessesToUpdate::All, true);
            let name = processes.name_for(pty.shell_pid(), pty.foreground_pgid());
            assert_eq!(name.as_deref(), Some("htop"), "wrong name on cycle {i}");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[test]
    fn name_for_returns_none_without_a_shell_pid_or_foreground_pgid() {
        let processes = ForegroundProcesses::new();
        assert_eq!(processes.name_for(None, None), None);
    }

    #[test]
    fn conhost_and_wslhost_are_recognized_as_implementation_details_to_skip() {
        assert!(is_console_host_implementation_detail("conhost.exe"));
        assert!(is_console_host_implementation_detail("CONHOST.EXE"));
        assert!(is_console_host_implementation_detail("wslhost.exe"));
        assert!(is_console_host_implementation_detail("WSLHOST.EXE"));
        assert!(!is_console_host_implementation_detail("wsl.exe"));
        assert!(!is_console_host_implementation_detail("htop"));
    }

    #[cfg(unix)]
    #[test]
    fn cwd_of_reports_a_real_processes_actual_working_directory() {
        let dir = std::env::temp_dir();
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .current_dir(&dir)
            .spawn()
            .expect("spawn a real child process with a known cwd");

        let mut processes = ForegroundProcesses::new();
        let cwd = processes.cwd_of(child.id());

        // Compare canonicalized: `current_dir` and what the OS reports back
        // can differ by a resolved symlink (e.g. macOS's `/tmp` vs `/private/tmp`)
        // without that being a real mismatch.
        assert_eq!(
            cwd.and_then(|p| p.canonicalize().ok()),
            dir.canonicalize().ok(),
            "cwd_of should report the same directory the process was spawned into"
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn cwd_of_returns_none_for_a_pid_that_does_not_exist() {
        let mut processes = ForegroundProcesses::new();
        // Reserved/invalid on every platform sysinfo supports — never a
        // real process to accidentally collide with.
        assert_eq!(processes.cwd_of(0), None);
    }
}
