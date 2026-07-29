//! Windows console attachment.
//!
//! This binary is built for the *windows* subsystem (see the
//! `windows_subsystem` attribute in `main.rs`), which is what a GUI
//! application should be: no console is created for it, so launching it from
//! Explorer, a shortcut, or the Start menu doesn't flash up a black window
//! that then sits there for the life of the process, and a shell that starts
//! it returns to its prompt instead of blocking until the terminal is closed.
//!
//! The cost of that, left alone, is that `--help`, `--version` and
//! `--verbose` would print into nothing when run from an existing terminal,
//! since the process has no console to write to. `--help` in particular
//! exists to tell someone where their config file lives, so silently
//! printing nothing is not an acceptable trade.
//!
//! [`attach_to_parent`] buys back both: attach to the console of whatever
//! started us, if it had one. Explorer has none, so nothing appears; `cmd`
//! and PowerShell do, so output lands where the person typing expects it.
//!
//! One visible wart remains and is inherent to the subsystem choice rather
//! than to this code: because the shell no longer waits for a GUI
//! application, output arrives *after* it has already printed its next
//! prompt. Every GUI application on Windows that also has a command line
//! behaves this way.

/// Attaches to the parent process's console, if it has one.
///
/// A no-op on every platform except Windows, and on Windows when there is no
/// parent console to attach to (launched from Explorer, a shortcut, or the
/// Start menu). Call once, before any output.
#[cfg(windows)]
pub fn attach_to_parent() {
    use std::ptr;

    use windows_sys::Win32::Foundation::{FALSE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
    };

    // SAFETY: both calls are plain Win32 FFI with no pointer arguments.
    // Failure is expected and fine — it means the parent had no console,
    // which is exactly the Explorer-launch case this whole module exists to
    // keep quiet.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == FALSE {
        return;
    }

    // Opening `CONOUT$` gives a handle to the console we just attached to.
    // Needed because a process started without a console may have no valid
    // standard handles at all, and attaching doesn't retroactively create
    // them.
    //
    // SAFETY: `name` is a valid NUL-terminated wide string that outlives the
    // call; the null pointers are documented as optional for
    // `CreateFileW`'s security-attributes and template-file parameters.
    let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
    let console = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    if console == INVALID_HANDLE_VALUE {
        return;
    }

    // Only fill in handles the process doesn't already have. A redirected
    // run — `pain --help > out.txt`, or piping into a pager — arrives with
    // perfectly good handles already, and pointing those at the console
    // instead would send the output to the screen and leave the file empty,
    // which is worse than the problem being fixed.
    for handle in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        if !has_valid_std_handle(handle) {
            // SAFETY: plain FFI with a handle we just successfully opened.
            unsafe { SetStdHandle(handle, console) };
        }
    }
}

/// Whether the process already has a usable handle for `which` — i.e. it was
/// started with its output redirected, and that redirection must be left
/// alone.
#[cfg(windows)]
fn has_valid_std_handle(which: windows_sys::Win32::System::Console::STD_HANDLE) -> bool {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::GetStdHandle;

    // SAFETY: plain FFI taking a documented constant.
    let handle = unsafe { GetStdHandle(which) };
    handle != INVALID_HANDLE_VALUE && !handle.is_null()
}

#[cfg(not(windows))]
pub fn attach_to_parent() {}
