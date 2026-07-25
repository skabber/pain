//! Screen grid: feeds PTY output through `alacritty_terminal`'s VT parser.

use std::sync::mpsc::{self, Receiver, Sender};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;

use crate::Size;

/// `Dimensions` impl for the fixed size a `Term` is constructed with.
struct TermSize {
    columns: usize,
    lines: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// Forwards the one event a pane's screen actually needs to act on:
/// `Event::PtyWrite`, `alacritty_terminal`'s way of asking the frontend to
/// write a reply back to the PTY's input — used for things like device
/// status reports and cursor-position queries. Some shells' startup
/// handshakes (notably Windows ConPTY/conhost) block waiting for these and
/// never produce any further output without a reply.
///
/// `Event::Title`/`Event::ResetTitle` (OSC 0/1/2) used to be forwarded here
/// too, for the pane title bar's "current application" label — reverted:
/// most shells' default prompt only sets this to `user@host: cwd`, updated
/// at the prompt, never to the actually-running foreground command, so it
/// couldn't answer the question it was being used for. Replaced with real
/// foreground-process detection (`pane::Pty::foreground_pgid` on Unix via
/// `tcgetpgrp`, `crates/app/src/foreground_process.rs`'s process-tree walk
/// on Windows) — see project memory for the full reasoning. Every other
/// event (title changes, bell, clipboard) is discarded; broadcast/grouping
/// and chrome react to those independently already.
#[derive(Clone)]
struct EventProxy(Sender<Vec<u8>>);

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            let _ = self.0.send(text.into_bytes());
        }
    }
}

/// One visible grid cell's character plus everything needed to color it —
/// unlike `visible_rows`, which discards attributes entirely (it only ever
/// fed plain text into a single fixed render color). `fg`/`bg` are still
/// the raw `Color` the program asked for (a named ANSI slot, a 256-index,
/// or a direct RGB spec) — resolving that into an actual displayed color
/// needs a palette and the app's own configured default foreground/
/// background, neither of which this crate has any business owning (no
/// theme system exists yet — CONOPS §8), so that resolution is the
/// frontend's job.
#[derive(Clone, Copy)]
pub struct RenderCell {
    pub c: char,
    pub fg: alacritty_terminal::vte::ansi::Color,
    pub bg: alacritty_terminal::vte::ansi::Color,
    pub flags: alacritty_terminal::term::cell::Flags,
}

/// A pane's screen: VT parser state plus the resulting character grid.
pub struct Screen {
    term: Term<EventProxy>,
    parser: Processor,
    pty_writes: Receiver<Vec<u8>>,
    cwd: crate::cwd::CwdWatcher,
}

impl Screen {
    /// Creates an empty screen of the given size.
    pub fn new(size: Size) -> Self {
        let dimensions = TermSize {
            columns: size.cols as usize,
            lines: size.rows as usize,
        };
        let (tx, rx) = mpsc::channel();
        let term = Term::new(Config::default(), &dimensions, EventProxy(tx));
        Self {
            term,
            parser: Processor::new(),
            pty_writes: rx,
            cwd: crate::cwd::CwdWatcher::new(),
        }
    }

    /// Feeds raw PTY output bytes into the terminal parser, updating the
    /// grid, and into the OSC 7 cwd watcher — a separate, independent scan
    /// of the same bytes (see `crate::cwd`'s doc comment for why this
    /// isn't handled by the VT parser itself).
    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
        self.cwd.advance(bytes);
    }

    /// The pane's most recently reported working directory, if any OSC 7
    /// sequence has arrived yet. `None` until then — not every shell
    /// configuration emits one, so callers need their own fallback (OS-level
    /// process cwd lookup, then home directory) rather than treating this
    /// as authoritative on its own.
    pub fn cwd(&self) -> Option<&std::path::Path> {
        self.cwd.cwd()
    }

    /// Resizes the grid to `size`. Does not touch the PTY — pair with
    /// `Pty::resize` so the kernel/ConPTY and the parsed grid agree.
    pub fn resize(&mut self, size: Size) {
        self.term.resize(TermSize {
            columns: size.cols as usize,
            lines: size.rows as usize,
        });
    }

    /// Drains any bytes the terminal needs written back to the PTY's input
    /// since the last call (e.g. a cursor-position report reply). Callers
    /// must forward these to the pane's `Pty::write` — some shells block
    /// waiting for them.
    pub fn take_pty_writes(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        while let Ok(bytes) = self.pty_writes.try_recv() {
            out.extend(bytes);
        }
        out
    }

    /// Returns the visible screen contents, one string per row, with
    /// trailing padding spaces trimmed. While scrolled back (`scroll`),
    /// this is history, not the live screen — same rows `visible_cells`
    /// would report.
    pub fn visible_rows(&self) -> Vec<String> {
        let grid = self.term.grid();
        let offset = grid.display_offset() as i32;
        (0..grid.screen_lines())
            .map(|i| {
                let row = &grid[Line(i as i32 - offset)];
                let text: String = row.into_iter().map(|cell| cell.c).collect();
                text.trim_end().to_string()
            })
            .collect()
    }

    /// Returns the visible screen's cells, one row of `RenderCell`s per
    /// row — nothing trimmed or discarded, unlike `visible_rows`, so a
    /// blank cell with an explicit background color (e.g. a program
    /// painting a status line) still comes through. While scrolled back
    /// (`scroll`), these are history rows, not the live screen.
    pub fn visible_cells(&self) -> Vec<Vec<RenderCell>> {
        let grid = self.term.grid();
        let offset = grid.display_offset() as i32;
        (0..grid.screen_lines())
            .map(|i| {
                let row = &grid[Line(i as i32 - offset)];
                row.into_iter()
                    .map(|cell| RenderCell { c: cell.c, fg: cell.fg, bg: cell.bg, flags: cell.flags })
                    .collect()
            })
            .collect()
    }

    /// Scrolls the viewport `lines` rows back into history (positive) or
    /// forward toward live output (negative) — for a mouse wheel over the
    /// pane. Safely clamped by `alacritty_terminal` itself at both ends:
    /// scrolling back further than the available history, or forward past
    /// live output, just stops there. Also a no-op while a full-screen
    /// program (vim, htop, less, ...) is in control of the pane — the
    /// "alternate screen" those switch to intentionally carries no
    /// scrollback of its own, the same convention every other terminal
    /// follows, so there's nothing for this to scroll into regardless.
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    /// Snaps the viewport back to live output. Called whenever the user
    /// types (`crate::Pty::write` callers) — matching every other
    /// terminal's convention that starting to type always returns focus
    /// to the live prompt, even mid-scrollback.
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// Whether the viewport is currently scrolled back into history rather
    /// than showing live output.
    pub fn is_scrolled_back(&self) -> bool {
        self.term.grid().display_offset() != 0
    }

    /// Returns the cursor's `(row, column)` within the visible screen.
    ///
    /// Only meaningful while the viewport isn't scrolled back
    /// (`is_scrolled_back`) — the cursor's tracked position is always
    /// against the live screen, so it doesn't correspond to anything
    /// currently visible once the viewport has scrolled away from it.
    pub fn cursor(&self) -> (usize, usize) {
        let point = self.term.grid().cursor.point;
        (point.line.0.max(0) as usize, point.column.0)
    }

    /// The terminal's current mode flags — mouse reporting
    /// (`MOUSE_REPORT_CLICK`/`MOUSE_DRAG`/`MOUSE_MOTION`/`SGR_MOUSE`) among
    /// them, which the frontend needs to decide whether a click/drag should
    /// be forwarded to the shell as an escape sequence or handled locally as
    /// a text selection.
    pub fn mode(&self) -> alacritty_terminal::term::TermMode {
        *self.term.mode()
    }

    /// Starts a fresh in-grid text selection at 0-indexed (row, col),
    /// replacing whatever selection (if any) was already active. Used for
    /// mouse-drag selection when the pane's program hasn't turned on mouse
    /// reporting — always `Side::Left`/`SelectionType::Simple` since a
    /// per-half-cell click side isn't tracked at this granularity yet.
    pub fn start_selection(&mut self, row: usize, col: usize) {
        let point = Point::new(Line(row as i32), Column(col));
        self.term.selection = Some(Selection::new(SelectionType::Simple, point, Side::Left));
    }

    /// Extends the in-progress selection (if any) to 0-indexed (row, col).
    pub fn update_selection(&mut self, row: usize, col: usize) {
        if let Some(selection) = &mut self.term.selection {
            selection.update(Point::new(Line(row as i32), Column(col)), Side::Left);
        }
    }

    /// Clears the active selection, if any.
    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    /// Whether the active selection (if any) never actually moved from
    /// where it started — no selection at all counts as empty too, so
    /// callers don't need to check `Option`-ness separately.
    pub fn selection_is_empty(&self) -> bool {
        self.term.selection.as_ref().is_none_or(Selection::is_empty)
    }

    /// The currently selected text, ready to copy to the clipboard — `None`
    /// if there's no selection, or it's empty.
    pub fn selection_to_string(&self) -> Option<String> {
        self.term.selection_to_string()
    }

    /// The range of grid cells currently selected, for drawing a highlight —
    /// the same `Selection::to_range` call `alacritty_terminal`'s own
    /// `RenderableContent` uses for the same purpose.
    pub fn selection_range(&self) -> Option<SelectionRange> {
        self.term.selection.as_ref().and_then(|s| s.to_range(&self.term))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_changes_visible_row_count() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 });
        screen.resize(Size { rows: 2, cols: 10 });

        assert_eq!(screen.visible_rows().len(), 2);
    }

    #[test]
    fn advance_updates_cwd_from_an_osc_7_sequence_alongside_the_grid() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 });
        assert_eq!(screen.cwd(), None);

        screen.advance(b"\x1b]7;file://host/home/will/project\x07prompt$ ");

        assert_eq!(screen.cwd(), Some(std::path::Path::new("/home/will/project")));
        // The same bytes still reached the VT parser as normal — this
        // isn't instead of updating the grid, just alongside it.
        assert_eq!(screen.visible_rows()[0], "prompt$");
    }

    #[test]
    fn scrolling_back_reveals_output_pushed_into_history() {
        let mut screen = Screen::new(Size { rows: 3, cols: 20 });
        assert!(!screen.is_scrolled_back());

        for i in 0..9 {
            screen.advance(format!("line{i}\r\n").as_bytes());
        }
        let live_top = screen.visible_rows()[0].clone();

        screen.scroll(3);
        assert!(screen.is_scrolled_back());
        let scrolled_top = screen.visible_rows()[0].clone();
        assert_ne!(scrolled_top, live_top, "scrolling back should reveal different, earlier content");

        screen.scroll_to_bottom();
        assert!(!screen.is_scrolled_back());
        assert_eq!(screen.visible_rows()[0], live_top, "scrolling back to bottom should restore the live view");
    }

    #[test]
    fn scrolling_back_past_available_history_clamps_instead_of_panicking() {
        let mut screen = Screen::new(Size { rows: 3, cols: 20 });
        // Overflow the 3 visible rows by 2 lines, so there's a little real
        // history to land in — otherwise (no history at all) clamping to 0
        // is the *correct* behavior, not evidence either way about the
        // over-scroll case this test means to exercise.
        for i in 0..5 {
            screen.advance(format!("line{i}\r\n").as_bytes());
        }

        // Wildly over-scrolling must clamp, not panic (`Storage`'s indexer
        // only debug-asserts range correctness — a bug here would only ever
        // surface as a debug-build panic under real usage).
        screen.scroll(1_000_000);
        assert!(screen.is_scrolled_back());
    }

    #[test]
    fn cursor_position_query_produces_a_pty_reply() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 });
        screen.advance(b"\x1b[3;5Hhi");
        screen.advance(b"\x1b[6n");

        let reply = screen.take_pty_writes();
        assert_eq!(reply, b"\x1b[3;7R");
    }

    #[test]
    fn renders_known_vt_sequence_into_grid() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 });
        screen.advance(b"hello, pane\r\n");

        let rows = screen.visible_rows();
        assert_eq!(rows[0], "hello, pane");
        assert_eq!(rows[1], "");
    }

    #[test]
    fn cursor_movement_escape_positions_text() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 });
        // Move cursor to row 3, column 5 (1-indexed, per CUP), then write.
        screen.advance(b"\x1b[3;5Hhi");

        let rows = screen.visible_rows();
        assert_eq!(rows[2], "    hi");
    }
}
