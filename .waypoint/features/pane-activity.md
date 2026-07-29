# Pane activity indicator

**Shipped:** 2026-07-28. Revised the same day after the developer's testing —
see "The bell bug" below, which is the part worth reading.

## What exists

A pane shows a dot in its title bar when something has happened in it:

| Dot | Meaning | Cleared by |
|---|---|---|
| Blue | The shell produced output since the pane was last focused | Focusing the pane |
| Red | A program rang the terminal bell | Typing into the pane |

- `crates/app/src/activity.rs` — the state machine, a pure function.
- `crates/pane/src/term.rs` — `Event::Bell` plumbing and `take_bell`.
- `crates/app/src/pane_session.rs` — `PumpOutcome`, `received_input`.

## Why it is built this way

**The bell was previously discarded.** `alacritty_terminal` emits
`Event::Bell` and the event proxy dropped everything except `PtyWrite`. A
program ringing the bell did nothing at all. That plumbing is the feature's
foundation, not an add-on to it.

**Both events share one channel, drained through one function.**
`take_pty_writes` and `take_bell` both call `drain_events` first. Draining
the channel independently inside each accessor would mean whichever ran first
silently consumed and discarded the other's events. Two tests cover exactly
that ordering hazard, in both directions.

**The indicator is a glyph, not a shape.** The renderer only draws
rectangles, so the first version emitted a `SolidRect` and was — accurately —
a small square, which the developer flagged as not what was asked for. It is
now `●` drawn through the ordinary glyph path, the same approach the close
button's `×` already used. A circle would otherwise need a shader change or
an alpha mask; the font already has the glyph. A test asserts it rasterizes,
since a missing glyph would draw nothing and look exactly like the feature
not working.

**The indicator slot is reserved unconditionally.** Whether or not a dot is
drawn, the space is held and the group name starts after it. Letting the slot
collapse when idle would make every label in the bar jump sideways the moment
a background pane produced output — motion precisely where the eye is being
drawn.

**Activity is diffed, not assumed dirty.** `poll` runs on every wake, and the
overwhelmingly common case is that nothing changed. Repainting on every poll
would undo the idle-cost work from v1.5.

## The bell bug

The first version's rule was "focus clears everything". The developer
reported that `sleep 3; echo hi` worked but `printf '\a'` did nothing.

Both observations were correct, and the cause was not detection — the bell
*was* being detected, and then immediately thrown away. `printf '\a'` emits
BEL the instant the command runs, which is while its own pane is still
focused, because you just pressed Enter in it. The rule reset focused panes
to `Idle` on the same poll the bell arrived, so it could never be drawn.
`sleep 3` appeared to work only because the three-second delay gave time to
click away first.

The fix separates the two signals rather than special-casing the symptom:

- **Output** is about attention you haven't given a pane, so focus genuinely
  clears it.
- **A bell** is a program explicitly asking for attention. It survives focus
  and is cleared by *input* to that pane — typing there is what actually
  proves someone noticed, and unlike focus it cannot happen incidentally.

`Signals` gained an `input` field, set by `PaneSession::write_input` and
consumed at the next poll, because input arrives on the event loop's keyboard
path rather than during a poll.

The general lesson, recorded because it generalises: a state machine that
clears on a *passive* condition (focus) will erase events that arrive
simultaneously with that condition. Clearing on an *active* one (input) does
not have that failure mode.

## Consequences worth knowing

- A bell shows a dot even on the focused pane, until you next type there.
  That is deliberate — it is the only way `printf '\a'` is ever visible.
- Multiple bells between polls collapse to one. Ringing twice does not want
  attention twice as much.
- A pane whose shell merely exited sets `changed` (so it repaints) but
  neither `output` nor `bell`, so it raises no dot.
