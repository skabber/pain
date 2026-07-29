//! Breaking a row of grid cells into runs that are safe to shape together.
//!
//! Only used when ligatures are enabled. Shaping a run lets the font
//! substitute one glyph for several characters (`!=` becoming `≠`), which is
//! the whole point — but it also means the shaper, which knows nothing about
//! the terminal's cell grid, decides where those glyphs sit. So a run may
//! only cover cells where ligating is actually correct.
//!
//! Three rules, each for a concrete reason:
//!
//! - **Break on a color change.** A ligature is a single glyph and can carry
//!   only one color. If `!` is red and `=` is green, they cannot be ligated
//!   without losing one of the two.
//! - **Break on whitespace.** Ligatures never span a space, and excluding
//!   spaces keeps runs short and avoids shaping the vast empty right-hand
//!   side of most terminal rows.
//! - **Break around the cursor.** While editing `!=` with the cursor between
//!   the two characters, a ligature would hide which one you are on. Every
//!   terminal that supports ligatures does this.

/// One cell's contribution to a run: what to draw and what color it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunCell {
    pub c: char,
    pub color: [f32; 4],
}

/// A maximal stretch of cells that can be shaped as a unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// Column of the run's first cell, for positioning it on screen.
    pub start_col: usize,
    pub text: String,
    pub color: [f32; 4],
}

/// Splits one row into runs, isolating `cursor_col` into a run of its own so
/// no ligature spans the cursor.
///
/// `cursor_col` is `None` for rows the cursor isn't on, and for a pane whose
/// viewport is scrolled back — the cursor tracks the live screen, so it
/// doesn't correspond to anything visible there.
pub fn split(cells: &[RunCell], cursor_col: Option<usize>) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut current: Option<Run> = None;

    for (col, cell) in cells.iter().enumerate() {
        let breaks_here = cell.c.is_whitespace() || Some(col) == cursor_col;
        let continues = match (&current, breaks_here) {
            (Some(run), false) => run.color == cell.color && run.start_col + run.text.chars().count() == col,
            _ => false,
        };

        if !continues && let Some(run) = current.take() {
            runs.push(run);
        }
        if cell.c.is_whitespace() {
            continue;
        }

        match &mut current {
            Some(run) => run.text.push(cell.c),
            None => current = Some(Run { start_col: col, text: cell.c.to_string(), color: cell.color }),
        }

        // The cursor's own cell is a complete run: the break above kept the
        // preceding text out of it, and closing it here keeps what follows
        // out too.
        if Some(col) == cursor_col
            && let Some(run) = current.take()
        {
            runs.push(run);
        }
    }

    if let Some(run) = current {
        runs.push(run);
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

    fn row(text: &str) -> Vec<RunCell> {
        text.chars().map(|c| RunCell { c, color: RED }).collect()
    }

    /// The shape of run this exists to produce: `!=` reaching the shaper as
    /// one unit so the font can ligate it.
    #[test]
    fn contiguous_same_colored_text_is_one_run() {
        let runs = split(&row("a!=b"), None);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "a!=b");
        assert_eq!(runs[0].start_col, 0);
    }

    #[test]
    fn whitespace_splits_runs_and_is_not_included_in_them() {
        let runs = split(&row("ab cd"), None);
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].start_col, runs[0].text.as_str()), (0, "ab"));
        assert_eq!((runs[1].start_col, runs[1].text.as_str()), (3, "cd"));
    }

    #[test]
    fn leading_and_trailing_whitespace_produce_no_runs() {
        let runs = split(&row("   ab   "), None);
        assert_eq!(runs.len(), 1);
        assert_eq!((runs[0].start_col, runs[0].text.as_str()), (3, "ab"));

        assert!(split(&row("     "), None).is_empty());
        assert!(split(&[], None).is_empty());
    }

    /// A ligature is one glyph and carries one color, so differently-colored
    /// characters can't be ligated together however adjacent they are.
    #[test]
    fn a_color_change_splits_a_run_even_mid_word() {
        let cells = vec![RunCell { c: '!', color: RED }, RunCell { c: '=', color: GREEN }];
        let runs = split(&cells, None);

        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].start_col, runs[0].text.as_str(), runs[0].color), (0, "!", RED));
        assert_eq!((runs[1].start_col, runs[1].text.as_str(), runs[1].color), (1, "=", GREEN));
    }

    /// Without this you can't tell which half of `!=` you're editing.
    #[test]
    fn the_cursor_cell_is_isolated_into_its_own_run() {
        let runs = split(&row("a!=b"), Some(2));

        assert_eq!(runs.len(), 3);
        assert_eq!((runs[0].start_col, runs[0].text.as_str()), (0, "a!"));
        assert_eq!((runs[1].start_col, runs[1].text.as_str()), (2, "="));
        assert_eq!((runs[2].start_col, runs[2].text.as_str()), (3, "b"));
    }

    #[test]
    fn a_cursor_at_the_start_or_end_of_a_row_still_splits_cleanly() {
        let runs = split(&row("ab"), Some(0));
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "a");
        assert_eq!(runs[1].text, "b");

        let runs = split(&row("ab"), Some(1));
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "a");
        assert_eq!(runs[1].text, "b");
    }

    #[test]
    fn a_cursor_over_whitespace_or_off_the_row_changes_nothing() {
        assert_eq!(split(&row("ab cd"), Some(2)), split(&row("ab cd"), None));
        assert_eq!(split(&row("abcd"), Some(99)), split(&row("abcd"), None));
    }

    /// Every non-space character must appear in exactly one run, at the
    /// column it actually occupies — runs drive positioning, so a dropped or
    /// misplaced cell is text rendered in the wrong place.
    #[test]
    fn runs_cover_every_non_space_cell_exactly_once() {
        let cells: Vec<RunCell> = "ab cd!=ef  g"
            .chars()
            .enumerate()
            .map(|(i, c)| RunCell { c, color: if i % 5 == 0 { GREEN } else { RED } })
            .collect();

        for cursor in [None, Some(0), Some(3), Some(6), Some(11)] {
            let runs = split(&cells, cursor);
            let mut seen: Vec<(usize, char)> = Vec::new();
            for run in &runs {
                for (offset, c) in run.text.chars().enumerate() {
                    seen.push((run.start_col + offset, c));
                }
            }

            let expected: Vec<(usize, char)> = cells
                .iter()
                .enumerate()
                .filter(|(_, cell)| !cell.c.is_whitespace())
                .map(|(i, cell)| (i, cell.c))
                .collect();
            assert_eq!(seen, expected, "cursor={cursor:?}");
        }
    }
}
