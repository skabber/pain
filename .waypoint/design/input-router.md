# Design: Input Router & Broadcast

**Status:** proposed — pending review
**Feeds from:** `.waypoint/conops.md` §5b–5d (grouping/broadcast, keybindings, mouse)

## Intent

Route keyboard and mouse events to the right pane(s), group-aware from the
start, with a clear split between chrome-owned input, app-level chords, and
passthrough to the running program.

## Data model

```rust
enum BroadcastMode { Off, Group, All }

struct GroupId(u64);

struct Router {
    focused: PaneId,
    broadcast_mode: BroadcastMode,
    groups: HashMap<GroupId, HashSet<PaneId>>,
    keymap: HashMap<Chord, Action>,
}

enum Action {
    SplitHorizontal,
    SplitVertical,
    ClosePane,
    Quit,
    FocusDir(Direction),
    ResizeDir(Direction),
    ToggleZoom,
    SetBroadcastMode(BroadcastMode),
}
```

## Keyboard flow

`event → resolve Chord → keymap lookup`:

- **Hit** (chord is bound to an `Action`): execute the app-level action.
  Split/close/focus/resize/zoom act on the `Layout`; `SetBroadcastMode`
  updates `Router.broadcast_mode`.
- **Miss**: the key is data for the running program. Resolve the target set
  (below) and write the raw bytes to every target pane's PTY.

Chords are never partially consumed — a key is either an app-level action or
passthrough, never both. This matches Terminator's model and keeps the router
free of modal state.

## Broadcast target resolution

`focused pane → broadcast_mode → target set`:

- **Off** → `{focused}`
- **Group** → the member set of `focused`'s group, or `{focused}` if
  ungrouped
- **All** → every pane currently in the layout

Resolution happens at event time (not cached), so adding/removing panes or
changing group membership takes effect on the next keystroke with no
invalidation logic needed.

## Mouse flow

Hit-test order, first match wins:

1. **Chrome regions** (divider, tab bar, config panel) — consumed entirely by
   our UI; never reaches pane logic. Divider drag adjusts the corresponding
   `Split.ratio` directly.
2. **In-grid, pane has mouse reporting enabled** (SGR mode, read from
   `alacritty_terminal`'s term state) **and Shift is not held** — encode and
   forward the event to that pane's PTY.
3. **Everything else in-grid** (mouse reporting off, or Shift held as
   override) — handled as terminal-level text selection in our own selection
   logic, regardless of what the running program requested.

Click always focuses the clicked pane before any of the above is evaluated.

## Visual indicator

The renderer reads `Router.broadcast_mode` and the resolved target set each
frame to draw a highlighted border on panes currently receiving broadcast
input. This is a read-only render-time query — it does not affect resolution,
which is recomputed independently at each keystroke.

## Rationale

- Modeling broadcast resolution as a pure function of current router/group
  state (rather than a maintained "active targets" list) avoids a whole class
  of bugs where the target set goes stale after a pane closes or a group
  changes mid-session.
- The all-or-nothing chord/passthrough split avoids partial-chord ambiguity
  (e.g. an unbound chord that shares a prefix with a bound one) — every chord
  is looked up whole, never incrementally.
