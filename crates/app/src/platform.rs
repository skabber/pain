//! Environment detection that affects behavior across more than one module
//! (window creation, rendering, and the settings UI all care whether
//! transparency is usable here).

/// Whether this process is running under WSL (Windows Subsystem for Linux).
///
/// WSLg is not a target platform for this app (Windows and native Linux
/// are) — it's a development environment that happens to run on the same
/// machine. Its compositor doesn't handle real per-pixel window
/// transparency correctly (observed: fully see-through regardless of the
/// configured level, and mouse clicks passing through the window), which
/// doesn't reproduce on either real target platform and isn't something
/// this project chases (same call as the WSLg cursor-theme quirks
/// documented in project memory) — used to disable transparency outright
/// on WSL rather than ship a feature that looks broken on a non-primary
/// environment.
#[cfg(target_os = "linux")]
pub fn is_wsl() -> bool {
    std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some()
}

#[cfg(not(target_os = "linux"))]
pub fn is_wsl() -> bool {
    false
}
