//! Paste safety: bracketed-paste encoding and the "this looks risky"
//! check that drives the confirmation prompt.
//!
//! Two independent protections, because they cover different failure
//! modes:
//!
//! - **Bracketed paste** is the protocol-level fix. A program that has
//!   asked for it (`TermMode::BRACKETED_PASTE`) receives the text wrapped
//!   in start/end markers, so a shell like bash 4.4+/zsh/fish knows the
//!   content is *pasted* rather than typed and leaves it sitting on the
//!   prompt for review instead of executing each newline as it arrives.
//!   It also stops editors like vim from applying auto-indent to pasted
//!   code. Zero friction, so it's always on when the program supports it.
//!
//! - **Confirmation** covers the case bracketing can't: a program that
//!   never enabled the mode (a bare `cat`, an old shell, an SSH session
//!   to something ancient) gets pasted text as if typed, newlines and
//!   all. There the only protection left is asking first.

/// Bracketed-paste start marker (`ESC [ 2 0 0 ~`).
const START: &[u8] = b"\x1b[200~";
/// Bracketed-paste end marker (`ESC [ 2 0 1 ~`).
const END: &[u8] = b"\x1b[201~";

/// Encodes `text` for writing to a PTY. When `bracketed` is set (the
/// program requested `TermMode::BRACKETED_PASTE`) the content is wrapped
/// in the start/end markers; otherwise it's passed through untouched.
///
/// Any end marker *inside* the pasted text is stripped first. Without
/// that, content containing a literal `ESC [ 2 0 1 ~` would terminate the
/// bracket early and everything after it would be received as ordinary
/// typed input — turning a paste of attacker-influenced text (a web page,
/// a log line, a file someone else wrote) into arbitrary command
/// execution. Real terminals filter this for the same reason.
pub fn encode(text: &str, bracketed: bool) -> Vec<u8> {
    if !bracketed {
        return text.as_bytes().to_vec();
    }
    let mut out = Vec::with_capacity(text.len() + START.len() + END.len());
    out.extend_from_slice(START);
    out.extend_from_slice(&strip_end_markers(text.as_bytes()));
    out.extend_from_slice(END);
    out
}

fn strip_end_markers(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(END) {
            i += END.len();
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Characters that submit a line to the program behind the PTY.
///
/// **Both**, not just `\n`. Carriage return is what the Enter key actually
/// sends (see `key_bytes` in `main.rs`), and the PTY's line discipline maps
/// it to a newline — so `\r` executes a command exactly as `\n` does. This
/// check tested only `\n` at first, which meant text using bare carriage
/// returns ran however many commands it liked without ever prompting:
/// precisely the "attacker-influenced text" case this module exists to
/// cover, and trivially reachable by anyone who knew to use `\r`.
const SUBMIT: [char; 2] = ['\n', '\r'];

/// Whether a paste should be confirmed with the user before it's sent.
///
/// `bracketed` means the receiving program has bracketed paste enabled,
/// which already prevents the newlines from executing — asking as well
/// would be pure friction, so it never prompts in that case.
///
/// Otherwise the rule is "would this run more than one command": any line
/// break that isn't part of the single trailing one. A lone trailing
/// newline is the overwhelmingly common "copy a command, paste it, run it"
/// case, and prompting on it would train people to click through the dialog
/// without reading — which is worse than not having it. A trailing `\r\n`
/// counts as one such break, not two.
pub fn needs_confirmation(text: &str, bracketed: bool) -> bool {
    if bracketed {
        return false;
    }
    text.trim_end_matches(SUBMIT).contains(SUBMIT)
}

/// Splits `text` into non-empty lines on either line-break convention, the
/// same set [`needs_confirmation`] counts. `str::lines` only knows `\n`
/// (treating `\r` as ordinary text), which would report a
/// carriage-return-separated paste as a single line in the confirmation
/// dialog — understating exactly what the user is being asked to approve.
///
/// Empty pieces are dropped, which handles `\r\n` (one break, not two)
/// and also means a blank line isn't counted. That suits what the count is
/// for: the dialog is telling the user roughly how many commands are about
/// to run, and a blank line runs nothing.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split(SUBMIT).filter(|line| !line.is_empty())
}

/// A short, single-line summary of a paste for the confirmation prompt —
/// enough to recognize what's about to be sent without letting a large
/// paste blow out the dialog.
pub fn summarize(text: &str) -> String {
    let count = lines(text).count();
    let first = lines(text).next().unwrap_or("").trim();
    let truncated: String = first.chars().take(60).collect();
    let ellipsis = if first.chars().count() > 60 { "…" } else { "" };
    format!("{count} lines, starting: {truncated}{ellipsis}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unbracketed_paste_passes_through_untouched() {
        assert_eq!(encode("echo hi\n", false), b"echo hi\n".to_vec());
    }

    #[test]
    fn bracketed_paste_is_wrapped_in_markers() {
        let out = encode("echo hi", true);
        assert!(out.starts_with(b"\x1b[200~"));
        assert!(out.ends_with(b"\x1b[201~"));
        assert!(String::from_utf8_lossy(&out).contains("echo hi"));
    }

    #[test]
    fn an_embedded_end_marker_cannot_escape_the_bracket() {
        // Without stripping, everything after the embedded marker would
        // arrive as typed input and run immediately.
        let hostile = "safe\x1b[201~rm -rf /\n";
        let out = encode(hostile, true);
        let body = &out[START.len()..out.len() - END.len()];
        assert!(
            !body.windows(END.len()).any(|w| w == END),
            "the end marker must not survive inside the bracketed body"
        );
        // The visible text is still delivered — it's neutered, not dropped.
        assert!(String::from_utf8_lossy(body).contains("rm -rf /"));
    }

    #[test]
    fn bracketed_pastes_never_prompt() {
        assert!(!needs_confirmation("a\nb\nc\n", true));
    }

    #[test]
    fn multi_line_unbracketed_paste_prompts() {
        assert!(needs_confirmation("echo one\necho two", false));
        assert!(needs_confirmation("echo one\necho two\n", false));
    }

    /// Carriage return is what Enter actually sends, and the PTY maps it to
    /// a newline — so `\r` runs a command exactly as `\n` does. This check
    /// tested only `\n`, which let a caller run as many commands as they
    /// liked with no prompt just by separating them with `\r`. Verified
    /// against a real `/bin/sh` pty at the time: both commands ran.
    #[test]
    fn carriage_returns_prompt_the_same_as_newlines() {
        assert!(needs_confirmation("echo one\recho two", false));
        assert!(needs_confirmation("echo one\recho two\r", false));
        assert!(needs_confirmation("echo one\r\necho two\r\n", false));
        // Mixed conventions in one payload, which is what a naive
        // "normalize then check" pass tends to miss.
        assert!(needs_confirmation("echo one\recho two\necho three", false));
    }

    #[test]
    fn a_single_trailing_carriage_return_does_not_prompt() {
        // The `\n` case's exact counterpart: one command, submitted once.
        assert!(!needs_confirmation("echo hi\r", false));
        assert!(!needs_confirmation("echo hi\r\n", false));
    }

    #[test]
    fn a_carriage_return_separated_paste_is_not_summarized_as_one_line() {
        // `str::lines` treats `\r` as ordinary text, so this used to read
        // "1 lines" — understating exactly what the user is approving.
        assert_eq!(summarize("echo one\recho two\r"), "2 lines, starting: echo one");
        // A `\r\n` pair is one break, not two, so it must not inflate the
        // count either.
        assert_eq!(summarize("echo one\r\necho two\r\n"), "2 lines, starting: echo one");
    }

    #[test]
    fn a_single_trailing_newline_does_not_prompt() {
        // The everyday "copy one command and run it" case — prompting here
        // would just teach people to dismiss the dialog reflexively.
        assert!(!needs_confirmation("echo hi\n", false));
        assert!(!needs_confirmation("echo hi", false));
    }

    #[test]
    fn summary_reports_line_count_and_truncates_long_first_lines() {
        assert_eq!(summarize("one\ntwo\n"), "2 lines, starting: one");
        let long = "x".repeat(100);
        let s = summarize(&long);
        assert!(s.contains('…'), "{s}");
        assert!(s.chars().count() < 100, "{s}");
    }
}
