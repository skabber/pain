//! Tracks a pane's current working directory from two escape-sequence
//! conventions shells/programs use to report it at each new prompt:
//!
//! - **OSC 7** (`ESC ] 7 ; file://host/path (BEL | ST)`) — the xterm/Unix
//!   convention, used by bash/zsh/etc.
//! - **OSC 9;9** (`ESC ] 9 ; 9 ; path (BEL | ST)`) — Windows Terminal/
//!   ConEmu's own convention, a plain path with no URI wrapping. Needed
//!   specifically for `crate::integration`'s PowerShell injection: a raw
//!   OSC 7 written via PowerShell's `Write-Host` (the legacy Windows
//!   Console API) gets silently dropped somewhere in ConPTY/conhost's own
//!   VT-translation layer rather than passed through — confirmed by a
//!   developer's real-hardware test (the prompt function installed and
//!   ran correctly; nothing ever arrived here). Windows' own tooling
//!   doesn't have that problem with OSC 9;9, since Microsoft added
//!   explicit support for it.
//!
//! Neither is handled by `alacritty_terminal`/`vte` at all — checked
//! directly in both crates' vendored source: `vte`'s OSC dispatch has no
//! case for `7` or `9`, it just falls through to an unhandled-sequence
//! debug log. Both formats are narrow and stable enough that hand-rolling
//! this scanner is far less ongoing maintenance than patching two
//! vendored dependencies to add a new feature (as opposed to the targeted
//! single-file bugfix `vendor/wgpu-hal` already is) — see project memory
//! for the fuller discussion.

use std::path::{Path, PathBuf};

/// Above this, a started-but-never-terminated sequence is abandoned rather
/// than buffered indefinitely — a real reported path is at most a few
/// hundred bytes; anything wildly longer than that is corrupted input or a
/// program writing bytes that happen to start with a marker but aren't
/// really one of these sequences at all, not a real path still arriving.
const MAX_PENDING: usize = 4096;

const OSC7_MARKER: &[u8] = b"\x1b]7;";
const OSC9X9_MARKER: &[u8] = b"\x1b]9;9;";

/// Which convention a matched marker was, so its payload gets parsed the
/// right way (a `file://` URI for OSC 7, a plain path for OSC 9;9).
#[derive(Clone, Copy)]
enum Marker {
    Osc7,
    Osc9x9,
}

/// Watches raw PTY output for OSC 7/OSC 9;9 sequences, remembering the
/// latest path seen. PTY output arrives in arbitrary byte chunks that can
/// split a sequence mid-way, so a partial match is buffered across
/// `advance` calls rather than discarded.
pub struct CwdWatcher {
    cwd: Option<PathBuf>,
    pending: Vec<u8>,
}

impl CwdWatcher {
    pub fn new() -> Self {
        Self { cwd: None, pending: Vec::new() }
    }

    /// Scans `bytes` for OSC 7/OSC 9;9 sequences, updating the tracked cwd
    /// on a match. Call with every chunk of raw PTY output, independently
    /// of (not instead of) feeding the same bytes to the VT parser — this
    /// never consumes or alters what the parser sees.
    pub fn advance(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);

        loop {
            let Some((start, marker, marker_len)) = find_marker(&self.pending) else {
                // No marker anywhere in the buffer: nothing to do next
                // call except watch for one split across the boundary, so
                // keep only enough of a tail to still recognize that —
                // the longer of the two markers, minus one byte.
                let keep = self.pending.len().min(OSC9X9_MARKER.len() - 1);
                let drop = self.pending.len() - keep;
                self.pending.drain(..drop);
                break;
            };

            let payload_start = start + marker_len;
            match find_terminator(&self.pending[payload_start..]) {
                Some((end, terminator_len)) => {
                    let payload = &self.pending[payload_start..payload_start + end];
                    let parsed = match marker {
                        Marker::Osc7 => parse_file_uri(payload),
                        Marker::Osc9x9 => parse_raw_path(payload),
                    };
                    if let Some(path) = parsed {
                        self.cwd = Some(path);
                    }
                    self.pending.drain(..payload_start + end + terminator_len);
                }
                None if self.pending.len() - start > MAX_PENDING => {
                    // Never terminated and grown implausibly large —
                    // abandon it rather than buffer forever.
                    self.pending.drain(..payload_start);
                }
                None => {
                    // A real sequence in progress: keep everything from
                    // its start for the next call, and stop scanning —
                    // there's nothing complete left to find.
                    self.pending.drain(..start);
                    break;
                }
            }
        }
    }

    /// The most recent directory reported, if any OSC 7/OSC 9;9 sequence
    /// has arrived yet.
    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }
}

impl Default for CwdWatcher {
    fn default() -> Self {
        Self::new()
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Finds whichever marker (OSC 7 or OSC 9;9) starts earliest in `haystack`,
/// returning its start offset, which one it was, and its byte length.
fn find_marker(haystack: &[u8]) -> Option<(usize, Marker, usize)> {
    let osc7 = find_subslice(haystack, OSC7_MARKER).map(|i| (i, Marker::Osc7, OSC7_MARKER.len()));
    let osc9x9 = find_subslice(haystack, OSC9X9_MARKER).map(|i| (i, Marker::Osc9x9, OSC9X9_MARKER.len()));
    match (osc7, osc9x9) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// Finds OSC's terminator (`BEL` or the two-byte `ESC \`, both standard
/// depending on which the emitting program prefers), returning its offset
/// and byte length so the caller can skip exactly the sequence, not just
/// the payload.
fn find_terminator(bytes: &[u8]) -> Option<(usize, usize)> {
    for (i, &b) in bytes.iter().enumerate() {
        if b == 0x07 {
            return Some((i, 1));
        }
        if b == 0x1b && bytes.get(i + 1) == Some(&b'\\') {
            return Some((i, 2));
        }
    }
    None
}

/// Extracts a filesystem path from OSC 7's `file://host/path` payload,
/// percent-decoding it. `host` is always this machine (OSC 7 has no
/// cross-host meaning) so it's skipped rather than checked. Handles the
/// Windows form (`file:///C:/Users/...`, an empty host and a drive-letter
/// path) by stripping the extra leading slash a plain `PathBuf::from`
/// would otherwise bake into the path.
fn parse_file_uri(uri: &[u8]) -> Option<PathBuf> {
    let uri = std::str::from_utf8(uri).ok()?;
    let rest = uri.strip_prefix("file://")?;
    let path_start = rest.find('/')?;
    let path = percent_decode(&rest[path_start..]);

    let is_windows_drive_path = path.as_bytes().get(2) == Some(&b':')
        && path.as_bytes().first() == Some(&b'/')
        && path.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic);
    Some(PathBuf::from(if is_windows_drive_path { &path[1..] } else { &path[..] }))
}

/// Extracts a path from OSC 9;9's payload — unlike OSC 7, this is already
/// a plain path with no `file://` wrapping and no percent-encoding
/// (Windows Terminal/ConEmu's own convention, simpler by design since it
/// only ever needs to carry a native path, backslashes included).
fn parse_raw_path(payload: &[u8]) -> Option<PathBuf> {
    let s = std::str::from_utf8(payload).ok()?;
    (!s.is_empty()).then(|| PathBuf::from(s))
}

fn percent_decode(s: &str) -> String {
    // Works on raw bytes throughout, not `&s[..]` sub-slicing — a `%XX`
    // escape's two hex digits aren't guaranteed to land on a UTF-8 char
    // boundary if the input is malformed, which would panic if sliced
    // directly out of the `&str`. `str::from_utf8` on the byte pair
    // instead just fails gracefully in that case.
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex_digits = (bytes[i] == b'%' && i + 3 <= bytes.len())
            .then(|| std::str::from_utf8(&bytes[i + 1..i + 3]).ok())
            .flatten();
        if let Some(digits) = hex_digits
            && let Ok(byte) = u8::from_str_radix(digits, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_cwd_reported_before_any_osc_7_sequence() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"just some normal shell output\r\n");
        assert_eq!(watcher.cwd(), None);
    }

    #[test]
    fn bel_terminated_sequence_updates_cwd() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"\x1b]7;file://myhost/home/will/project\x07");
        assert_eq!(watcher.cwd(), Some(Path::new("/home/will/project")));
    }

    #[test]
    fn st_terminated_sequence_updates_cwd() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"\x1b]7;file://myhost/home/will/project\x1b\\");
        assert_eq!(watcher.cwd(), Some(Path::new("/home/will/project")));
    }

    #[test]
    fn sequence_split_across_multiple_advance_calls_still_matches() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"some output\r\n\x1b]7;file://myhost/home");
        assert_eq!(watcher.cwd(), None, "shouldn't resolve until the terminator arrives");
        watcher.advance(b"/will/project\x07more output");
        assert_eq!(watcher.cwd(), Some(Path::new("/home/will/project")));
    }

    #[test]
    fn marker_itself_split_across_advance_calls_still_matches() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"prompt text \x1b]7");
        watcher.advance(b";file://myhost/etc\x07");
        assert_eq!(watcher.cwd(), Some(Path::new("/etc")));
    }

    #[test]
    fn later_sequence_replaces_the_earlier_tracked_cwd() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"\x1b]7;file://myhost/home/will\x07");
        assert_eq!(watcher.cwd(), Some(Path::new("/home/will")));
        watcher.advance(b"\x1b]7;file://myhost/home/will/project\x07");
        assert_eq!(watcher.cwd(), Some(Path::new("/home/will/project")));
    }

    #[test]
    fn percent_encoded_characters_are_decoded() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"\x1b]7;file://myhost/home/will/my%20project\x07");
        assert_eq!(watcher.cwd(), Some(Path::new("/home/will/my project")));
    }

    #[test]
    fn windows_drive_letter_path_drops_the_extra_leading_slash() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"\x1b]7;file://myhost/C:/Users/will\x07");
        assert_eq!(watcher.cwd(), Some(Path::new("C:/Users/will")));
    }

    #[test]
    fn osc_9x9_bel_terminated_sequence_updates_cwd() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"\x1b]9;9;C:\\Users\\will\x07");
        assert_eq!(watcher.cwd(), Some(Path::new("C:\\Users\\will")));
    }

    #[test]
    fn osc_9x9_st_terminated_sequence_updates_cwd() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"\x1b]9;9;C:\\Users\\will\x1b\\");
        assert_eq!(watcher.cwd(), Some(Path::new("C:\\Users\\will")));
    }

    #[test]
    fn osc_9x9_sequence_split_across_multiple_advance_calls_still_matches() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"prompt \x1b]9;9;C:\\Us");
        assert_eq!(watcher.cwd(), None);
        watcher.advance(b"ers\\will\x07");
        assert_eq!(watcher.cwd(), Some(Path::new("C:\\Users\\will")));
    }

    #[test]
    fn osc_9x9_marker_itself_split_across_advance_calls_still_matches() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"prompt \x1b]9;9");
        watcher.advance(b";C:\\Users\\will\x07");
        assert_eq!(watcher.cwd(), Some(Path::new("C:\\Users\\will")));
    }

    #[test]
    fn osc_7_and_osc_9x9_in_the_same_buffer_resolve_in_order() {
        // Whichever marker starts earliest wins first, and a later one
        // still updates the cwd afterward — not something either
        // convention alone would exercise.
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"\x1b]7;file://myhost/home/will\x07\x1b]9;9;C:\\Users\\will\x07");
        assert_eq!(watcher.cwd(), Some(Path::new("C:\\Users\\will")));
    }

    #[test]
    fn an_unterminated_sequence_that_never_completes_does_not_grow_forever() {
        let mut watcher = CwdWatcher::new();
        for _ in 0..10 {
            watcher.advance(&[b'x'; 1000]);
        }
        watcher.advance(b"\x1b]7;");
        for _ in 0..10 {
            watcher.advance(&[b'y'; 1000]);
        }
        // No terminator ever arrived, and the buffer grew well past
        // MAX_PENDING along the way — this must not panic or leak memory
        // unboundedly, and must still recover once a real sequence starts.
        assert_eq!(watcher.cwd(), None);
        watcher.advance(b"\x1b]7;file://myhost/tmp\x07");
        assert_eq!(watcher.cwd(), Some(Path::new("/tmp")));
    }

    #[test]
    fn non_file_uri_payload_is_ignored() {
        let mut watcher = CwdWatcher::new();
        watcher.advance(b"\x1b]7;not-a-uri\x07");
        assert_eq!(watcher.cwd(), None);
    }
}
