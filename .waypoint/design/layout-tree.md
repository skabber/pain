# Design: Layout Tree & Pane Model

**Status:** proposed — pending review
**Feeds from:** `.waypoint/conops.md` §3 (Panes and layout), §4.1

## Intent

A data model for arbitrarily nested pane splits that supports drag-resize,
directional focus movement, zoom, and close-with-rebalance, without
delegating geometry to egui.

## Data model

```rust
struct PaneId(u64);

enum Orientation { Horizontal, Vertical }

enum Node {
    Split {
        orientation: Orientation,
        ratio: f32,           // 0.0–1.0, position of the divider; clamped to [0.05, 0.95]
        first: Box<Node>,
        second: Box<Node>,
    },
    Leaf(PaneId),
}

struct Layout {
    root: Node,
    zoomed: Option<PaneId>,   // if set, render/route as if this pane were the only leaf
}

struct Pane {
    id: PaneId,
    pty: PortablePty,
    term: alacritty_terminal::Term,
    group: Option<GroupId>,  // orthogonal to tree position
    cwd: PathBuf,             // latest known, for session persistence
}
```

Pane state (`HashMap<PaneId, Pane>`) is separate from the tree. The tree only
encodes geometry and ordering; group membership and pane state live alongside
it, keyed by `PaneId`, so grouping and layout can change independently.

## Operations

- **Split(pane_id, orientation).** Replace the target `Leaf(pane_id)` with a
  `Split` node: `first` = the existing leaf, `second` = a new leaf for the
  freshly spawned pane, `ratio` = 0.5.
- **Close(pane_id).** Remove the `Leaf(pane_id)`; its parent `Split` node is
  replaced by the *sibling* subtree (rebalance = the sibling and everything
  under it expands to fill the freed space — no ratio recalculation needed
  elsewhere in the tree).
- **Resize(split, delta).** Adjust `ratio` on the `Split` node whose divider
  was dragged; clamp to `[0.05, 0.95]` so no pane can be dragged to zero size.
- **Focus(direction).** Requires computed geometry: walk the tree once per
  frame to compute each leaf's screen rect (recursive split of parent rect by
  orientation and ratio). Directional focus picks the visible pane whose rect
  is adjacent in the requested direction from the focused pane's rect —
  computed on demand from the current rects, not stored in the tree.
- **Zoom toggle.** Sets/clears `Layout.zoomed`. Does not restructure the tree.
  When set, the renderer and input router treat the zoomed pane as the sole
  visible leaf; toggling off returns to the tree's normal rendering with no
  data loss.

## Rationale

- Keeping `ratio` as the only per-split state (vs. storing absolute pixel
  sizes) means the tree is resolution-independent — a window resize just
  reruns the same rect computation against the new window size.
- Rebalancing on close is a pure tree operation (promote the sibling) with no
  special-casing, because there's no separate "size" field to redistribute.
- Zoom as a `Layout`-level flag rather than a tree mutation avoids having to
  reconstruct the pre-zoom tree on toggle-back, and keeps zoom orthogonal to
  every other operation (a zoomed pane can still be closed, resized in the
  background, etc.)

## Open questions

- Focus history: if the zoomed pane is closed while zoomed, does zoom clear
  automatically? (Proposed: yes — clear `zoomed` whenever the zoomed pane_id
  is removed from the tree.)
