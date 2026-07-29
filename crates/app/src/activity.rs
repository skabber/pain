//! What a pane has done since you last looked at it.
//!
//! Split out from `Graphics` so the transition rule is a pure function over
//! (current state, was it focused, what happened) and can be tested without
//! standing up a window, a GPU surface, or a real shell.

/// A pane's activity state, shown as a dot in its title bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activity {
    /// Nothing worth flagging.
    #[default]
    Idle,
    /// The shell produced output since this pane was last focused — a build
    /// still logging, a test run finishing, a tail picking something up.
    ///
    /// Only meaningful for a pane you aren't looking at, so focus clears it.
    Output,
    /// A program rang the terminal bell, and that hasn't been acknowledged
    /// yet.
    ///
    /// Outranks [`Activity::Output`], and unlike it **survives focus**. A
    /// bell is a program explicitly asking for attention, and it usually
    /// rings the instant a command starts — which is while its own pane is
    /// still focused, because you just pressed Enter there. Clearing it on
    /// focus meant `printf '\a'` set and erased the flag within the same
    /// poll and could never be seen. It clears on the next input to the
    /// pane instead: typing there is what actually proves you noticed.
    Bell,
}

/// What a pane did during one poll, as far as this module cares.
#[derive(Debug, Clone, Copy, Default)]
pub struct Signals {
    /// New bytes arrived from the shell.
    pub output: bool,
    /// The program rang the bell.
    pub bell: bool,
    /// The user sent input to this pane — a keystroke or a paste. Read as
    /// "the bell has been acknowledged"; see [`Activity::Bell`].
    pub input: bool,
}

/// The pane's activity state after this poll.
///
/// Order matters and encodes the priorities: a bell always wins, because it
/// is the one signal a program raises deliberately. Input clears a standing
/// bell, since it means the person is at that pane. Focus alone clears only
/// [`Activity::Output`] — a pane you're looking at can't have unattended
/// output, but it can absolutely have just rung at you.
pub fn next(current: Activity, focused: bool, signals: Signals) -> Activity {
    if signals.bell {
        return Activity::Bell;
    }
    if signals.input {
        return Activity::Idle;
    }
    if current == Activity::Bell {
        return Activity::Bell;
    }
    if focused {
        return Activity::Idle;
    }
    if signals.output {
        return Activity::Output;
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The indicator is drawn as a glyph, not a shape the renderer builds
    /// out of rectangles — so if the font has no `●` it silently draws
    /// nothing at all, which looks exactly like the feature not working.
    /// Font fallback covers this on any real desktop; this catches the case
    /// where it doesn't.
    #[test]
    fn the_activity_glyph_actually_rasterizes() {
        let mut rasterizer = render::GlyphRasterizer::new();
        let glyph = rasterizer
            .rasterize(crate::graphics::ACTIVITY_GLYPH, 16.0, "")
            .expect("the activity indicator glyph must resolve, or no dot is ever drawn");
        assert!(glyph.width > 0 && glyph.height > 0);
    }

    const NOTHING: Signals = Signals { output: false, bell: false, input: false };
    const OUTPUT: Signals = Signals { output: true, bell: false, input: false };
    const BELL: Signals = Signals { output: false, bell: true, input: false };
    const INPUT: Signals = Signals { output: false, bell: false, input: true };

    /// The reported bug, as the exact sequence that produced it: typing
    /// `printf '\a'` into a pane rings the bell *while that pane is still
    /// focused*, because you just pressed Enter in it. Clearing on focus
    /// erased the flag in the same poll it arrived, so the bell could never
    /// be seen — while `sleep 3; echo hi` appeared to work only because the
    /// delay let you click away first.
    #[test]
    fn a_bell_rung_in_the_focused_pane_survives_to_be_seen() {
        // Enter is pressed: input and the bell land together, pane focused.
        let both = Signals { output: false, bell: true, input: true };
        let mut state = next(Activity::Idle, true, both);
        assert_eq!(state, Activity::Bell, "the bell must win over the keystroke that caused it");

        // Still focused, nothing further happening — it must not evaporate.
        for _ in 0..5 {
            state = next(state, true, NOTHING);
            assert_eq!(state, Activity::Bell);
        }

        // Click away: still flagged, which is the whole point.
        state = next(state, false, NOTHING);
        assert_eq!(state, Activity::Bell);

        // Come back and type: acknowledged.
        state = next(state, true, INPUT);
        assert_eq!(state, Activity::Idle);
    }

    #[test]
    fn input_clears_a_standing_bell_whether_or_not_the_pane_is_focused() {
        assert_eq!(next(Activity::Bell, true, INPUT), Activity::Idle);
        assert_eq!(next(Activity::Bell, false, INPUT), Activity::Idle);
    }

    /// Input is only an acknowledgement of a bell. It shouldn't be able to
    /// mask a bell that arrives in the same poll.
    #[test]
    fn a_bell_arriving_with_input_still_registers() {
        let both = Signals { output: false, bell: true, input: true };
        assert_eq!(next(Activity::Idle, false, both), Activity::Bell);
        assert_eq!(next(Activity::Bell, true, both), Activity::Bell);
    }

    #[test]
    fn an_idle_unfocused_pane_with_nothing_happening_stays_idle() {
        assert_eq!(next(Activity::Idle, false, NOTHING), Activity::Idle);
    }

    #[test]
    fn output_on_an_unfocused_pane_flags_it() {
        assert_eq!(next(Activity::Idle, false, OUTPUT), Activity::Output);
    }

    /// Focus clears unattended *output* — you're looking at it, so it isn't
    /// unattended. It deliberately does not clear a bell; see
    /// `a_bell_rung_in_the_focused_pane_survives_to_be_seen`.
    #[test]
    fn focusing_a_pane_clears_output_but_not_a_bell() {
        assert_eq!(next(Activity::Output, true, NOTHING), Activity::Idle);
        assert_eq!(next(Activity::Bell, true, NOTHING), Activity::Bell);
    }

    #[test]
    fn a_focused_pane_never_accumulates_output_activity() {
        assert_eq!(next(Activity::Idle, true, OUTPUT), Activity::Idle);
    }

    #[test]
    fn a_bell_outranks_output_arriving_in_the_same_poll() {
        let both = Signals { output: true, bell: true, input: false };
        assert_eq!(next(Activity::Idle, false, both), Activity::Bell);
    }

    /// The common shape of a bell: a program rings once and then keeps
    /// printing. If later output demoted the state, the bell would be
    /// erased before anyone saw it.
    #[test]
    fn later_output_does_not_downgrade_a_pane_that_already_rang() {
        assert_eq!(next(Activity::Bell, false, OUTPUT), Activity::Bell);
        assert_eq!(next(Activity::Bell, false, NOTHING), Activity::Bell);
    }

    #[test]
    fn a_bell_upgrades_a_pane_already_flagged_for_output() {
        assert_eq!(next(Activity::Output, false, BELL), Activity::Bell);
    }

    /// Unfocusing alone is not activity — a pane you split away from
    /// shouldn't immediately show a dot for having done nothing.
    #[test]
    fn output_flagged_state_persists_until_focus_clears_it() {
        let mut state = Activity::Idle;
        state = next(state, false, OUTPUT);
        for _ in 0..5 {
            state = next(state, false, NOTHING);
        }
        assert_eq!(state, Activity::Output);

        state = next(state, true, NOTHING);
        assert_eq!(state, Activity::Idle);
    }
}
