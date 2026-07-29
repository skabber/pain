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

/// One of the two `alacritty_terminal` events a pane's screen acts on,
/// carried over a single channel so their relative order can't be lost.
enum ScreenEvent {
    /// `alacritty_terminal` asking the frontend to write a reply back to the
    /// PTY's input — device status reports, cursor-position queries. Some
    /// shells' startup handshakes (notably Windows ConPTY/conhost) block
    /// waiting for these and never produce any further output without a
    /// reply.
    PtyWrite(Vec<u8>),
    /// The program rang the terminal bell (`BEL`, `0x07`). Surfaced so an
    /// unfocused pane can flag that it wants attention — see
    /// [`Screen::take_bell`].
    Bell,
}

/// Forwards the events a pane's screen actually needs to act on. See
/// [`ScreenEvent`] for what those are and why.
///
/// `Event::Title`/`Event::ResetTitle` (OSC 0/1/2) used to be forwarded here
/// too, for the pane title bar's "current application" label — reverted:
/// most shells' default prompt only sets this to `user@host: cwd`, updated
/// at the prompt, never to the actually-running foreground command, so it
/// couldn't answer the question it was being used for. Replaced with real
/// foreground-process detection (`pane::Pty::foreground_pgid` on Unix via
/// `tcgetpgrp`, `crates/app/src/foreground_process.rs`'s process-tree walk
/// on Windows) — see project memory for the full reasoning. Every other
/// event (clipboard, color queries) is discarded; broadcast/grouping and
/// chrome react to those independently already.
#[derive(Clone)]
struct EventProxy(Sender<ScreenEvent>);

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        let forwarded = match event {
            Event::PtyWrite(text) => ScreenEvent::PtyWrite(text.into_bytes()),
            Event::Bell => ScreenEvent::Bell,
            _ => return,
        };
        let _ = self.0.send(forwarded);
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
    events: Receiver<ScreenEvent>,
    /// Drained out of `events` by [`Screen::drain_events`] and held until
    /// [`Screen::take_pty_writes`] collects it.
    pending_writes: Vec<u8>,
    /// Set by [`Screen::drain_events`], cleared by [`Screen::take_bell`].
    bell_rang: bool,
    cwd: crate::cwd::CwdWatcher,
    /// Kept so [`Screen::set_scrollback`] can change one field without
    /// resetting the rest of what `Term` was configured with.
    config: Config,
}

impl Screen {
    /// Creates an empty screen of the given size, retaining `scrollback`
    /// lines of history per pane.
    ///
    /// `scrollback` is passed in rather than defaulted: this used to build
    /// `Config::default()`, whose own `scrolling_history` is 10000, so the
    /// `general.scrollback_lines` setting — wired all the way through the
    /// settings panel, saved, and documented as defaulting to 5000 — never
    /// reached the terminal grid at all and had no effect on anything.
    pub fn new(size: Size, scrollback: usize) -> Self {
        let dimensions = TermSize { columns: size.cols as usize, lines: size.rows as usize };
        let (tx, rx) = mpsc::channel();
        let config = Config { scrolling_history: scrollback, ..Config::default() };
        let term = Term::new(config.clone(), &dimensions, EventProxy(tx));
        Self {
            term,
            parser: Processor::new(),
            events: rx,
            pending_writes: Vec::new(),
            bell_rang: false,
            cwd: crate::cwd::CwdWatcher::new(),
            config,
        }
    }

    /// Changes how many lines of history this screen retains, for a live
    /// config edit. Shrinking discards the oldest history beyond the new
    /// limit; growing simply raises the ceiling, leaving what's already
    /// retained alone.
    pub fn set_scrollback(&mut self, scrollback: usize) {
        if self.config.scrolling_history == scrollback {
            return;
        }
        self.config.scrolling_history = scrollback;
        self.term.set_options(self.config.clone());
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
        self.term.resize(TermSize { columns: size.cols as usize, lines: size.rows as usize });
    }

    /// Moves everything the event channel is holding into the per-kind
    /// fields the `take_*` accessors read.
    ///
    /// Both accessors call this first, so neither can consume an event
    /// meant for the other — draining the channel directly inside each one
    /// would mean whichever ran first silently discarded the other's
    /// events.
    fn drain_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                ScreenEvent::PtyWrite(bytes) => self.pending_writes.extend(bytes),
                ScreenEvent::Bell => self.bell_rang = true,
            }
        }
    }

    /// Drains any bytes the terminal needs written back to the PTY's input
    /// since the last call (e.g. a cursor-position report reply). Callers
    /// must forward these to the pane's `Pty::write` — some shells block
    /// waiting for them.
    pub fn take_pty_writes(&mut self) -> Vec<u8> {
        self.drain_events();
        std::mem::take(&mut self.pending_writes)
    }

    /// Whether the program rang the terminal bell since the last call,
    /// clearing the flag. Multiple bells between calls collapse into one —
    /// this answers "does this pane want attention?", and ringing twice
    /// doesn't want it twice as much.
    pub fn take_bell(&mut self) -> bool {
        self.drain_events();
        std::mem::replace(&mut self.bell_rang, false)
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

    /// Whether the running program has enabled bracketed paste (DECSET
    /// 2004). When it has, pasted text should be wrapped in the
    /// bracketed-paste markers so the program can tell a paste from
    /// typing — see the `paste` module in the app crate. Exposed as a
    /// plain bool so callers don't need to depend on
    /// `alacritty_terminal`'s own `TermMode` type just to ask.
    pub fn wants_bracketed_paste(&self) -> bool {
        self.term.mode().contains(alacritty_terminal::term::TermMode::BRACKETED_PASTE)
    }

    /// Starts a fresh in-grid text selection at 0-indexed (row, col),
    /// replacing whatever selection (if any) was already active. Used for
    /// mouse-drag selection when the pane's program hasn't turned on mouse
    /// reporting — always `Side::Left`/`SelectionType::Simple` since a
    /// per-half-cell click side isn't tracked at this granularity yet.
    /// Starts a selection of a given granularity — a plain character drag,
    /// or the whole word/line under the point (what double- and
    /// triple-click produce in every other terminal). Word and line
    /// selections still track further mouse movement, extending by that
    /// same unit, which is what `alacritty_terminal`'s own `Semantic` and
    /// `Lines` types give for free.
    pub fn start_selection_of(&mut self, row: usize, col: usize, kind: SelectionKind) {
        let point = Point::new(Line(row as i32), Column(col));
        let ty = match kind {
            SelectionKind::Character => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
        };
        self.term.selection = Some(Selection::new(ty, point, Side::Left));
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

    /// The OSC 8 hyperlink on the cell at 0-indexed (`row`, `col`) within
    /// the visible screen, plus the span of columns sharing it.
    ///
    /// This is an explicit escape sequence — `ls --hyperlink`, `cargo`, and
    /// similar tools marking text as a link — as opposed to the app's own
    /// pattern-matching over what a line happens to look like. It is
    /// authoritative where it exists: the program said what the target is,
    /// so there's nothing to guess and the link text needn't resemble a URL
    /// at all.
    ///
    /// Deliberately a point query rather than a field on
    /// [`RenderCell`]: this is only ever needed for the cell under the
    /// pointer, and carrying a link on every cell would add a refcount
    /// bump per cell per frame to the render path for something almost
    /// always absent.
    ///
    /// The span stops at the row's edges. A link wrapped across rows
    /// reports only the part on this one, which is what the underline
    /// should cover anyway.
    pub fn hyperlink_at(&self, row: usize, col: usize) -> Option<HyperlinkMatch> {
        let grid = self.term.grid();
        if row >= grid.screen_lines() || col >= grid.columns() {
            return None;
        }
        let line = &grid[Line(row as i32 - grid.display_offset() as i32)];
        let target = line[Column(col)].hyperlink()?;

        // Equality covers both id and URI, so two runs that merely share a
        // URI stay separate links — which is exactly what an explicit OSC 8
        // id is for.
        let same = |c: usize| line[Column(c)].hyperlink().is_some_and(|link| link == target);
        let mut start = col;
        while start > 0 && same(start - 1) {
            start -= 1;
        }
        let mut end = col + 1;
        while end < grid.columns() && same(end) {
            end += 1;
        }

        Some(HyperlinkMatch { uri: target.uri().to_string(), start, end })
    }
}

/// An OSC 8 hyperlink found under a grid cell: its target, and the half-open
/// range of columns on that row which are part of the same link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperlinkMatch {
    pub uri: String,
    pub start: usize,
    pub end: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How far back into history `screen` can actually scroll, in rows —
    /// scrolling is clamped to what's retained, so the resting display
    /// offset after over-scrolling *is* the retained history size.
    fn retained_history(screen: &mut Screen) -> usize {
        screen.scroll_to_bottom();
        screen.scroll(1_000_000);
        screen.term.grid().display_offset()
    }

    #[test]
    fn resize_changes_visible_row_count() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 }, 100);
        screen.resize(Size { rows: 2, cols: 10 });

        assert_eq!(screen.visible_rows().len(), 2);
    }

    /// The configured scrollback has to actually reach the grid. It didn't:
    /// this built `Config::default()` and ignored the setting entirely, so
    /// every pane silently retained alacritty's own 10000-line default no
    /// matter what `general.scrollback_lines` said.
    #[test]
    fn the_configured_scrollback_is_what_the_grid_retains() {
        let mut small = Screen::new(Size { rows: 3, cols: 20 }, 10);
        let mut large = Screen::new(Size { rows: 3, cols: 20 }, 200);
        for i in 0..500 {
            small.advance(format!("line{i}\r\n").as_bytes());
            large.advance(format!("line{i}\r\n").as_bytes());
        }

        assert_eq!(retained_history(&mut small), 10);
        assert_eq!(retained_history(&mut large), 200);
    }

    #[test]
    fn set_scrollback_applies_to_an_already_running_screen() {
        let mut screen = Screen::new(Size { rows: 3, cols: 20 }, 200);
        for i in 0..500 {
            screen.advance(format!("line{i}\r\n").as_bytes());
        }
        assert_eq!(retained_history(&mut screen), 200);

        // Shrinking drops the oldest history beyond the new limit...
        screen.set_scrollback(20);
        assert_eq!(retained_history(&mut screen), 20);

        // ...and growing raises the ceiling for what arrives next, rather
        // than resurrecting what shrinking already discarded.
        screen.set_scrollback(100);
        assert_eq!(retained_history(&mut screen), 20);
        for i in 0..500 {
            screen.advance(format!("more{i}\r\n").as_bytes());
        }
        assert_eq!(retained_history(&mut screen), 100);
    }

    #[test]
    fn advance_updates_cwd_from_an_osc_7_sequence_alongside_the_grid() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 }, 100);
        assert_eq!(screen.cwd(), None);

        screen.advance(b"\x1b]7;file://host/home/will/project\x07prompt$ ");

        assert_eq!(screen.cwd(), Some(std::path::Path::new("/home/will/project")));
        // The same bytes still reached the VT parser as normal — this
        // isn't instead of updating the grid, just alongside it.
        assert_eq!(screen.visible_rows()[0], "prompt$");
    }

    #[test]
    fn scrolling_back_reveals_output_pushed_into_history() {
        let mut screen = Screen::new(Size { rows: 3, cols: 20 }, 100);
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
        let mut screen = Screen::new(Size { rows: 3, cols: 20 }, 100);
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
        let mut screen = Screen::new(Size { rows: 5, cols: 20 }, 100);
        screen.advance(b"\x1b[3;5Hhi");
        screen.advance(b"\x1b[6n");

        let reply = screen.take_pty_writes();
        assert_eq!(reply, b"\x1b[3;7R");
    }

    #[test]
    fn a_bel_byte_raises_the_bell_flag_once_and_then_clears_it() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 }, 100);
        assert!(!screen.take_bell());

        screen.advance(b"done\x07");

        assert!(screen.take_bell(), "BEL should raise the bell flag");
        assert!(!screen.take_bell(), "taking the bell should clear it");
        // The bell is a side channel, not a printed character.
        assert_eq!(screen.visible_rows()[0], "done");
    }

    #[test]
    fn repeated_bells_between_takes_collapse_into_one() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 }, 100);
        screen.advance(b"\x07\x07\x07");

        assert!(screen.take_bell());
        assert!(!screen.take_bell());
    }

    /// Both accessors drain the same channel, so one must not be able to
    /// swallow the other's events. Draining inside each accessor
    /// independently would make this fail: whichever ran first would
    /// consume — and discard — the other kind.
    #[test]
    fn taking_the_bell_does_not_consume_a_pending_pty_reply() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 }, 100);
        // A bell and a cursor-position query, in that order.
        screen.advance(b"\x07\x1b[6n");

        assert!(screen.take_bell());
        assert_eq!(screen.take_pty_writes(), b"\x1b[1;1R");
    }

    #[test]
    fn taking_pty_writes_does_not_consume_a_pending_bell() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 }, 100);
        screen.advance(b"\x1b[6n\x07");

        assert_eq!(screen.take_pty_writes(), b"\x1b[1;1R");
        assert!(screen.take_bell());
    }

    /// Writes `text` as an OSC 8 hyperlink to `uri`. `id` distinguishes two
    /// runs that share a URI but are meant as separate links.
    fn osc8(id: &str, uri: &str, text: &str) -> String {
        format!("\x1b]8;id={id};{uri}\x1b\\{text}\x1b]8;;\x1b\\")
    }

    #[test]
    fn an_osc_8_hyperlink_is_found_under_its_own_cells_and_spans_them() {
        let mut screen = Screen::new(Size { rows: 5, cols: 40 }, 100);
        screen.advance(format!("see {} ok", osc8("a", "https://example.com", "this link")).as_bytes());

        // "see " is columns 0-3, so the link text occupies 4..13.
        let found = screen.hyperlink_at(0, 6).expect("the cell should carry a hyperlink");
        assert_eq!(found.uri, "https://example.com");
        assert_eq!((found.start, found.end), (4, 13));

        // The link text needn't look like a URL at all — that's the whole
        // point of the program declaring it.
        assert_eq!(screen.visible_rows()[0], "see this link ok");
    }

    #[test]
    fn cells_outside_a_hyperlink_carry_none() {
        let mut screen = Screen::new(Size { rows: 5, cols: 40 }, 100);
        screen.advance(format!("see {} ok", osc8("a", "https://example.com", "link")).as_bytes());

        assert!(screen.hyperlink_at(0, 0).is_none(), "plain text before the link");
        assert!(screen.hyperlink_at(0, 9).is_none(), "plain text after the link");
        assert!(screen.hyperlink_at(1, 0).is_none(), "an empty row");
    }

    /// Two runs with the same URI but different ids are two links, and the
    /// span of one must not swallow the other. Comparing by URI alone would
    /// merge them.
    #[test]
    fn adjacent_links_sharing_a_uri_stay_separate() {
        let mut screen = Screen::new(Size { rows: 5, cols: 40 }, 100);
        let uri = "https://example.com";
        screen.advance(format!("{}{}", osc8("one", uri, "aaa"), osc8("two", uri, "bbb")).as_bytes());

        let first = screen.hyperlink_at(0, 1).expect("first link");
        let second = screen.hyperlink_at(0, 4).expect("second link");
        assert_eq!((first.start, first.end), (0, 3));
        assert_eq!((second.start, second.end), (3, 6));
    }

    #[test]
    fn a_hyperlink_query_outside_the_grid_returns_none_rather_than_panicking() {
        let mut screen = Screen::new(Size { rows: 3, cols: 10 }, 100);
        screen.advance(osc8("a", "https://example.com", "hi").as_bytes());

        assert!(screen.hyperlink_at(99, 0).is_none());
        assert!(screen.hyperlink_at(0, 99).is_none());
    }

    #[test]
    fn renders_known_vt_sequence_into_grid() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 }, 100);
        screen.advance(b"hello, pane\r\n");

        let rows = screen.visible_rows();
        assert_eq!(rows[0], "hello, pane");
        assert_eq!(rows[1], "");
    }

    #[test]
    fn cursor_movement_escape_positions_text() {
        let mut screen = Screen::new(Size { rows: 5, cols: 20 }, 100);
        // Move cursor to row 3, column 5 (1-indexed, per CUP), then write.
        screen.advance(b"\x1b[3;5Hhi");

        let rows = screen.visible_rows();
        assert_eq!(rows[2], "    hi");
    }
}

/// How much text one selection gesture covers. Maps onto
/// `alacritty_terminal`'s selection types, but kept as its own enum so
/// callers (the app crate) don't need to depend on that crate directly
/// just to ask for a word selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    Character,
    Word,
    Line,
}
