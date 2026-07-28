//! Binary split tree for arbitrarily nested pane layouts.
//!
//! See `.waypoint/design/layout-tree.md` for the design rationale. `ratio`
//! is the only per-split state (not absolute sizes), so the tree is
//! resolution-independent — a window resize just reruns [`Layout::geometry`]
//! against the new area.

mod geometry;

use serde::{Deserialize, Serialize};

pub use geometry::{DividerRect, Geometry, PaneRect, Rect};

const MIN_RATIO: f32 = 0.05;
const MAX_RATIO: f32 = 0.95;

/// Identifies a pane within a [`Layout`]. Assigned by the layout itself
/// (at creation or on split) — panes don't choose their own ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(u64);

/// Identifies a split (and so its divider) within a [`Layout`]. Needed to
/// address a specific divider for resize and hit-testing, since a divider
/// belongs to a split node, not to either of its panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SplitId(u64);

/// How a split's two children are arranged: `Horizontal` produces left/right
/// panes (matching tmux's `-h`); `Vertical` produces top/bottom panes.
/// Whether this matches Terminator's own "horizontal"/"vertical" labeling
/// (the two tools aren't guaranteed to agree) is unverified — Milestone 3
/// checks the exact terms against Terminator's current docs when it wires
/// up the real keybindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// A direction for focus movement or resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone)]
enum Node {
    Split {
        id: SplitId,
        orientation: Orientation,
        /// Position of the divider, 0.0-1.0 along the split axis, clamped
        /// to [`MIN_RATIO`]..[`MAX_RATIO`] so neither side can be dragged
        /// to nothing.
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
    Leaf(PaneId),
}

/// A serializable snapshot of a tree's shape, split orientations, and
/// ratios — everything needed to recreate the same layout, deliberately
/// without pane identity: `PaneId`s are meaningless across process
/// restarts (nothing on a fresh run could still mean the same thing), so
/// `Layout::from_snapshot` always assigns fresh ones. `Leaf` carries no
/// data itself; a session file's own per-pane state (cwd, group, ...) is
/// correlated positionally, by zipping its per-pane list against
/// `Layout::panes()`'s output — both that and this snapshot's own leaves
/// walk the tree in the same left-to-right, depth-first order.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum SavedNode {
    Split { orientation: Orientation, ratio: f32, first: Box<SavedNode>, second: Box<SavedNode> },
    Leaf,
}

/// The pane/split tree for one window.
pub struct Layout {
    root: Node,
    next_pane_id: u64,
    next_split_id: u64,
    /// If set, [`Layout::geometry`] and focus/close treat this pane as the
    /// only one in the tree, without altering the tree itself.
    zoomed: Option<PaneId>,
}

impl Layout {
    /// Creates a layout with a single pane filling the whole area.
    pub fn new() -> (Self, PaneId) {
        let id = PaneId(0);
        let layout = Self { root: Node::Leaf(id), next_pane_id: 1, next_split_id: 0, zoomed: None };
        (layout, id)
    }

    /// Splits `pane`, inserting a freshly created pane as its sibling.
    /// Returns the new pane's id, or `None` if `pane` isn't in the tree.
    pub fn split(&mut self, pane: PaneId, orientation: Orientation) -> Option<PaneId> {
        let new_pane = PaneId(self.next_pane_id);
        let split_id = SplitId(self.next_split_id);
        if !Self::split_node(&mut self.root, pane, orientation, new_pane, split_id) {
            return None;
        }
        self.next_pane_id += 1;
        self.next_split_id += 1;
        Some(new_pane)
    }

    fn split_node(
        node: &mut Node,
        target: PaneId,
        orientation: Orientation,
        new_pane: PaneId,
        split_id: SplitId,
    ) -> bool {
        match node {
            Node::Leaf(id) if *id == target => {
                *node = Node::Split {
                    id: split_id,
                    orientation,
                    ratio: 0.5,
                    first: Box::new(Node::Leaf(target)),
                    second: Box::new(Node::Leaf(new_pane)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => {
                Self::split_node(first, target, orientation, new_pane, split_id)
                    || Self::split_node(second, target, orientation, new_pane, split_id)
            }
        }
    }

    /// Closes `pane`, promoting its sibling to take its parent split's
    /// place, then rebalances every split in `pane`'s same visual row/
    /// column — the contiguous run of same-orientation splits containing
    /// it — so the survivors split the freed space equally instead of
    /// only the immediate sibling absorbing all of it while a pane
    /// further along the same row stays at its old size (the exact bug
    /// report this was built for: closing the middle of three equal
    /// horizontal panes used to leave the left one at 1/3 and balloon the
    /// right one to 2/3). Splits of a *different* orientation nested
    /// inside the row (e.g. a vertical stack sitting in one horizontal
    /// slot) are treated as one opaque slot and keep their own internal
    /// ratio untouched — only the row/column actually losing a pane
    /// rebalances. Returns `false` if `pane` is the tree's only pane —
    /// closing the last pane is a decision for the caller (e.g. quit),
    /// not something the tree can express.
    pub fn close(&mut self, pane: PaneId) -> bool {
        if matches!(&self.root, Node::Leaf(id) if *id == pane) {
            return false;
        }
        // Both read-only lookups have to happen *before* `close_node`
        // mutates the tree — `row_root_id` in particular identifies a
        // split that `close_node`'s sibling-promotion may collapse away
        // entirely (a plain 2-pane row), which is fine: `rebalance_row`
        // below simply finds nothing to do in that case.
        let row_root_id = Self::parent_orientation(&self.root, pane)
            .and_then(|orientation| Self::find_row_root_id(&self.root, pane, orientation));
        if !Self::close_node(&mut self.root, pane) {
            return false;
        }
        if self.zoomed == Some(pane) {
            self.zoomed = None;
        }
        if let Some(row_root_id) = row_root_id {
            Self::rebalance_row(&mut self.root, row_root_id);
        }
        true
    }

    fn close_node(node: &mut Node, target: PaneId) -> bool {
        match node {
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => {
                let first_is_target = matches!(first.as_ref(), Node::Leaf(id) if *id == target);
                let second_is_target = matches!(second.as_ref(), Node::Leaf(id) if *id == target);

                if first_is_target {
                    *node = std::mem::replace(second.as_mut(), Node::Leaf(target));
                    return true;
                }
                if second_is_target {
                    *node = std::mem::replace(first.as_mut(), Node::Leaf(target));
                    return true;
                }

                Self::close_node(first, target) || Self::close_node(second, target)
            }
        }
    }

    /// The orientation of `target`'s immediate parent split, if any —
    /// `None` for the tree's sole root pane (already handled by `close`'s
    /// own early return, but kept total here rather than panicking).
    fn parent_orientation(node: &Node, target: PaneId) -> Option<Orientation> {
        match node {
            Node::Leaf(_) => None,
            Node::Split { orientation, first, second, .. } => {
                let is_direct_child = |n: &Node| matches!(n, Node::Leaf(id) if *id == target);
                if is_direct_child(first) || is_direct_child(second) {
                    return Some(*orientation);
                }
                Self::parent_orientation(first, target).or_else(|| Self::parent_orientation(second, target))
            }
        }
    }

    /// The id of the outermost split of `orientation` containing
    /// `target` — the top of the contiguous same-orientation chain
    /// `target` sits in (its visual row or column), found by descending
    /// from the true root and stopping at the very first match: anything
    /// found first, walking top-down, can't be nested inside another
    /// match closer to the root.
    fn find_row_root_id(node: &Node, target: PaneId, orientation: Orientation) -> Option<SplitId> {
        match node {
            Node::Leaf(_) => None,
            Node::Split { id, orientation: o, first, second, .. } => {
                if *o == orientation && Self::contains_pane(node, target) {
                    return Some(*id);
                }
                Self::find_row_root_id(first, target, orientation)
                    .or_else(|| Self::find_row_root_id(second, target, orientation))
            }
        }
    }

    fn contains_pane(node: &Node, target: PaneId) -> bool {
        match node {
            Node::Leaf(id) => *id == target,
            Node::Split { first, second, .. } => {
                Self::contains_pane(first, target) || Self::contains_pane(second, target)
            }
        }
    }

    /// Finds the split identified by `row_root_id` within `node` (a no-op
    /// if it no longer exists — the whole row collapsed to one slot when
    /// `close_node` promoted the closed pane's sibling all the way up to
    /// what used to be the row's own root) and re-assigns every
    /// same-orientation ratio inside it via `assign_equal_shares`.
    fn rebalance_row(node: &mut Node, row_root_id: SplitId) -> bool {
        match node {
            Node::Leaf(_) => false,
            Node::Split { id, orientation, ratio, first, second } => {
                if *id == row_root_id {
                    let orientation = *orientation;
                    let left = Self::assign_equal_shares(first, orientation);
                    let right = Self::assign_equal_shares(second, orientation);
                    *ratio = (left as f32 / (left + right) as f32).clamp(MIN_RATIO, MAX_RATIO);
                    true
                } else {
                    Self::rebalance_row(first, row_root_id) || Self::rebalance_row(second, row_root_id)
                }
            }
        }
    }

    /// Recursively assigns ratios within a same-orientation chain so
    /// every slot — a leaf, or an opaque subtree belonging to a
    /// *different* orientation (its own internal ratios are left alone) —
    /// ends up an equal share of the whole. Returns how many slots this
    /// node represents, so its own caller (one level up the same chain)
    /// can compute its own ratio from the counts on each side.
    fn assign_equal_shares(node: &mut Node, orientation: Orientation) -> usize {
        match node {
            Node::Leaf(_) => 1,
            Node::Split { orientation: o, ratio, first, second, .. } => {
                if *o != orientation {
                    return 1;
                }
                let left = Self::assign_equal_shares(first, orientation);
                let right = Self::assign_equal_shares(second, orientation);
                *ratio = (left as f32 / (left + right) as f32).clamp(MIN_RATIO, MAX_RATIO);
                left + right
            }
        }
    }

    /// Adjusts a split's divider position by `delta` (same units as
    /// `ratio`: 0.0-1.0 of the split's axis), clamped so neither side
    /// shrinks to nothing.
    pub fn resize(&mut self, split: SplitId, delta: f32) -> bool {
        Self::resize_node(&mut self.root, split, delta)
    }

    fn resize_node(node: &mut Node, target: SplitId, delta: f32) -> bool {
        match node {
            Node::Leaf(_) => false,
            Node::Split { id, ratio, first, second, .. } => {
                if *id == target {
                    *ratio = (*ratio + delta).clamp(MIN_RATIO, MAX_RATIO);
                    true
                } else {
                    Self::resize_node(first, target, delta) || Self::resize_node(second, target, delta)
                }
            }
        }
    }

    /// Finds the nearest ancestor split of `pane` whose orientation matches
    /// `direction`'s axis (`Left`/`Right` → `Horizontal`, `Up`/`Down` →
    /// `Vertical`), for keyboard-driven resize. Returns that split's id and
    /// whether `pane` is on its `first` side — the caller decides the sign
    /// of the ratio delta from that (see `Graphics::resize_focused` in the
    /// app crate for the convention used).
    pub fn resize_target(&self, pane: PaneId, direction: Direction) -> Option<(SplitId, bool)> {
        let wanted = match direction {
            Direction::Left | Direction::Right => Orientation::Horizontal,
            Direction::Up | Direction::Down => Orientation::Vertical,
        };
        Self::resize_target_node(&self.root, pane, wanted)
    }

    fn resize_target_node(node: &Node, target: PaneId, wanted: Orientation) -> Option<(SplitId, bool)> {
        match node {
            Node::Leaf(_) => None,
            Node::Split { id, orientation, first, second, .. } => {
                let in_first = Self::contains_node(first, target);
                let in_second = !in_first && Self::contains_node(second, target);
                if (in_first || in_second) && *orientation == wanted {
                    return Some((*id, in_first));
                }
                if in_first {
                    Self::resize_target_node(first, target, wanted)
                } else if in_second {
                    Self::resize_target_node(second, target, wanted)
                } else {
                    None
                }
            }
        }
    }

    /// Toggles zoom for `pane`: zooms it if nothing (or a different pane)
    /// is zoomed, un-zooms if it's already the zoomed pane. No-op if `pane`
    /// isn't in the tree.
    pub fn toggle_zoom(&mut self, pane: PaneId) {
        if !self.contains(pane) {
            return;
        }
        self.zoomed = if self.zoomed == Some(pane) { None } else { Some(pane) };
    }

    /// The currently zoomed pane, if any.
    pub fn zoomed(&self) -> Option<PaneId> {
        self.zoomed
    }

    /// Whether `pane` is present in the tree.
    pub fn contains(&self, pane: PaneId) -> bool {
        Self::contains_node(&self.root, pane)
    }

    fn contains_node(node: &Node, target: PaneId) -> bool {
        match node {
            Node::Leaf(id) => *id == target,
            Node::Split { first, second, .. } => {
                Self::contains_node(first, target) || Self::contains_node(second, target)
            }
        }
    }

    /// Computes screen geometry for `area`: every pane's rect, and every
    /// divider's rect (each `divider_thickness` wide/tall). If a pane is
    /// zoomed, its rect is `area` and there are no dividers.
    pub fn geometry(&self, area: Rect, divider_thickness: f32) -> Geometry {
        if let Some(zoomed) = self.zoomed {
            return Geometry { panes: vec![PaneRect { pane: zoomed, rect: area }], dividers: vec![] };
        }
        geometry::compute(&self.root, area, divider_thickness)
    }

    /// Finds the pane adjacent to `current` in `direction`, or `None` if
    /// there isn't one. Computed from `area`'s geometry on demand, not
    /// stored in the tree.
    pub fn focus_neighbor(&self, current: PaneId, direction: Direction, area: Rect) -> Option<PaneId> {
        let geometry = self.geometry(area, 0.0);
        let current_rect = geometry.panes.iter().find(|p| p.pane == current)?.rect;

        geometry
            .panes
            .iter()
            .filter(|p| p.pane != current)
            .filter_map(|p| geometry::adjacency(current_rect, p.rect, direction).map(|dist| (dist, p.pane)))
            .min_by(|(a, _), (b, _)| a.total_cmp(b))
            .map(|(_, pane)| pane)
    }

    /// All pane ids currently in the tree, in tree order.
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        Self::collect_panes(&self.root, &mut out);
        out
    }

    fn collect_panes(node: &Node, out: &mut Vec<PaneId>) {
        match node {
            Node::Leaf(id) => out.push(*id),
            Node::Split { first, second, .. } => {
                Self::collect_panes(first, out);
                Self::collect_panes(second, out);
            }
        }
    }

    /// A serializable snapshot of this tree's shape, for session save.
    /// Ignores zoom state deliberately — restoring a session always starts
    /// unzoomed, the same as any other transient view state (focus,
    /// broadcast mode) this doesn't persist.
    pub fn snapshot(&self) -> SavedNode {
        Self::snapshot_node(&self.root)
    }

    fn snapshot_node(node: &Node) -> SavedNode {
        match node {
            Node::Leaf(_) => SavedNode::Leaf,
            Node::Split { orientation, ratio, first, second, .. } => SavedNode::Split {
                orientation: *orientation,
                ratio: *ratio,
                first: Box::new(Self::snapshot_node(first)),
                second: Box::new(Self::snapshot_node(second)),
            },
        }
    }

    /// Rebuilds a layout from a snapshot, assigning fresh pane/split ids in
    /// the same left-to-right, depth-first order the snapshot's leaves
    /// appear in. Returns the new layout and its panes in that order, for
    /// the caller to zip against whatever per-pane state (cwd, group, ...)
    /// was saved alongside the snapshot.
    pub fn from_snapshot(snapshot: &SavedNode) -> (Self, Vec<PaneId>) {
        let mut next_pane_id = 0;
        let mut next_split_id = 0;
        let mut panes = Vec::new();
        let root = Self::build_node(snapshot, &mut next_pane_id, &mut next_split_id, &mut panes);
        let layout = Self { root, next_pane_id, next_split_id, zoomed: None };
        (layout, panes)
    }

    fn build_node(
        snapshot: &SavedNode,
        next_pane_id: &mut u64,
        next_split_id: &mut u64,
        panes: &mut Vec<PaneId>,
    ) -> Node {
        match snapshot {
            SavedNode::Leaf => {
                let id = PaneId(*next_pane_id);
                *next_pane_id += 1;
                panes.push(id);
                Node::Leaf(id)
            }
            SavedNode::Split { orientation, ratio, first, second } => {
                let id = SplitId(*next_split_id);
                *next_split_id += 1;
                let first = Box::new(Self::build_node(first, next_pane_id, next_split_id, panes));
                let second = Box::new(Self::build_node(second, next_pane_id, next_split_id, panes));
                Node::Split { id, orientation: *orientation, ratio: *ratio, first, second }
            }
        }
    }

    /// Rebuilds the tree from scratch into `arrangement`'s preset shape,
    /// keeping exactly `panes` (in the given order) as its leaves — their
    /// existing ids, not fresh ones (unlike `from_snapshot`): this
    /// rearranges panes that already exist, rather than creating new
    /// ones, so nothing should respawn. Clears zoom — a full rearrangement
    /// has no single "the zoomed one" left to preserve.
    ///
    /// `panes` must be non-empty; a `Layout` always has at least one pane.
    pub fn arrange(panes: &[PaneId], arrangement: Arrangement) -> Self {
        assert!(!panes.is_empty(), "arrange requires at least one pane");
        let mut next_split_id = 0;
        let root = match arrangement {
            Arrangement::Horizontal => Self::chain(panes, Orientation::Horizontal, &mut next_split_id),
            Arrangement::Vertical => Self::chain(panes, Orientation::Vertical, &mut next_split_id),
            Arrangement::Grid => Self::grid(panes, &mut next_split_id),
        };
        let next_pane_id = panes.iter().map(|p| p.0).max().map_or(0, |max| max + 1);
        Self { root, next_pane_id, next_split_id, zoomed: None }
    }

    /// A flat chain of same-orientation splits — `panes[0]` first, then
    /// recursively dividing what's left. Every pane ends up the same
    /// size: at each step the split ratio is `1/n` for the `n` panes still
    /// left to place, which is always `1/(total)` of the *original* space
    /// once multiplied through the ratios of the splits already taken to
    /// get there (e.g. for 3 panes: the root's `1/3` is `1/3` of the
    /// total directly; the next split's `1/2` is `1/2` of the remaining
    /// `2/3`, which is `1/3` of the total; the last pane gets what's left,
    /// also `1/3`).
    fn chain(panes: &[PaneId], orientation: Orientation, next_split_id: &mut u64) -> Node {
        match panes {
            [] => unreachable!("caller guarantees at least one pane"),
            [only] => Node::Leaf(*only),
            [first, rest @ ..] => {
                let id = SplitId(*next_split_id);
                *next_split_id += 1;
                let ratio = 1.0 / panes.len() as f32;
                Node::Split {
                    id,
                    orientation,
                    ratio,
                    first: Box::new(Node::Leaf(*first)),
                    second: Box::new(Self::chain(rest, orientation, next_split_id)),
                }
            }
        }
    }

    /// Tiles panes into a roughly square grid, row-major (left to right,
    /// then top to bottom): outer splits stack rows (`Vertical`), each
    /// row built as its own left-to-right `chain` (`Horizontal`). The
    /// last row may hold fewer panes than the others if the count doesn't
    /// divide evenly into a square-ish shape; its panes still divide
    /// evenly among just themselves, same as any other row.
    fn grid(panes: &[PaneId], next_split_id: &mut u64) -> Node {
        let cols = (panes.len() as f64).sqrt().ceil() as usize;
        let rows: Vec<Node> =
            panes.chunks(cols.max(1)).map(|row| Self::chain(row, Orientation::Horizontal, next_split_id)).collect();
        Self::stack(rows, next_split_id)
    }

    /// Stacks pre-built subtrees vertically, evenly — the same evenly-
    /// dividing-chain technique `chain` uses for individual panes,
    /// generalized here to arbitrary subtrees (a grid's rows) instead.
    fn stack(mut nodes: Vec<Node>, next_split_id: &mut u64) -> Node {
        if nodes.len() == 1 {
            return nodes.pop().expect("checked len == 1");
        }
        let first = nodes.remove(0);
        let ratio = 1.0 / (nodes.len() + 1) as f32;
        let id = SplitId(*next_split_id);
        *next_split_id += 1;
        Node::Split {
            id,
            orientation: Orientation::Vertical,
            ratio,
            first: Box::new(first),
            second: Box::new(Self::stack(nodes, next_split_id)),
        }
    }
}

/// A preset way to rearrange every pane currently in the tree at once —
/// distinct from [`Layout::split`], which only ever adds one new pane to
/// the existing tree; [`Layout::arrange`] replaces the whole tree's shape
/// while keeping exactly the same panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrangement {
    /// All panes in a single row, side by side, equal width — the same
    /// `Horizontal` orientation `split`'s own "left/right" already uses.
    Horizontal,
    /// All panes in a single column, stacked top to bottom, equal height.
    Vertical,
    /// Panes tiled into a roughly square grid, row-major (left to right,
    /// then top to bottom) — the last row may hold fewer panes than the
    /// others if the count doesn't divide evenly.
    Grid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_creates_two_panes() {
        let (mut layout, root) = Layout::new();
        let second = layout.split(root, Orientation::Horizontal).expect("split root");

        let mut panes = layout.panes();
        panes.sort();
        let mut expected = vec![root, second];
        expected.sort();
        assert_eq!(panes, expected);
    }

    #[test]
    fn close_promotes_sibling() {
        let (mut layout, root) = Layout::new();
        let second = layout.split(root, Orientation::Horizontal).unwrap();
        let third = layout.split(second, Orientation::Vertical).unwrap();

        assert!(layout.close(second));
        let mut panes = layout.panes();
        panes.sort();
        let mut expected = vec![root, third];
        expected.sort();
        assert_eq!(panes, expected);
    }

    #[test]
    fn closing_the_middle_of_three_equal_horizontal_panes_rebalances_the_survivors_equally() {
        let (a, b, c) = (PaneId(0), PaneId(1), PaneId(2));
        let mut layout = Layout::arrange(&[a, b, c], Arrangement::Horizontal);
        let area = Rect { x: 0.0, y: 0.0, width: 900.0, height: 300.0 };

        // Confirms the setup actually reproduces the bug report's starting
        // point: three equal 300-wide panes.
        let before = layout.geometry(area, 0.0);
        for pane in before.panes {
            assert!((pane.rect.width - 300.0).abs() < 0.001, "expected an equal-thirds start, got {pane:?}");
        }

        assert!(layout.close(b));

        // Before this fix, `a` (structurally to the left, outside the
        // split `b` was promoted out of) stayed pinned at its old 1/3
        // share while `c` absorbed the entire freed space, landing at
        // 300/600 instead of the expected equal 450/450 split.
        let after = layout.geometry(area, 0.0);
        assert_eq!(after.panes.len(), 2);
        for pane in &after.panes {
            assert!((pane.rect.width - 450.0).abs() < 0.001, "expected both survivors at half width, got {pane:?}");
        }
    }

    #[test]
    fn closing_the_rightmost_of_three_equal_horizontal_panes_rebalances_the_survivors_equally() {
        let (a, b, c) = (PaneId(0), PaneId(1), PaneId(2));
        let mut layout = Layout::arrange(&[a, b, c], Arrangement::Horizontal);
        let area = Rect { x: 0.0, y: 0.0, width: 900.0, height: 300.0 };

        assert!(layout.close(c));

        let after = layout.geometry(area, 0.0);
        assert_eq!(after.panes.len(), 2);
        for pane in &after.panes {
            assert!((pane.rect.width - 450.0).abs() < 0.001, "expected both survivors at half width, got {pane:?}");
        }
    }

    #[test]
    fn closing_one_pane_in_a_four_pane_row_leaves_the_other_three_equal() {
        let ids: Vec<PaneId> = (0..4).map(PaneId).collect();
        let mut layout = Layout::arrange(&ids, Arrangement::Horizontal);
        let area = Rect { x: 0.0, y: 0.0, width: 1200.0, height: 300.0 };

        assert!(layout.close(ids[1]));

        let after = layout.geometry(area, 0.0);
        assert_eq!(after.panes.len(), 3);
        for pane in &after.panes {
            assert!(
                (pane.rect.width - 400.0).abs() < 0.001,
                "expected all three survivors at a third each, got {pane:?}"
            );
        }
    }

    #[test]
    fn closing_a_pane_outside_a_differently_oriented_nested_stack_leaves_the_stack_untouched() {
        // A horizontal row of two slots: `left`, and a vertical stack of
        // `top`/`bottom` occupying the right slot — the same shape a user
        // gets from splitting `left`'s neighbor vertically. Closing `left`
        // is a Horizontal-row close; the Vertical stack inside the other
        // slot is a different orientation and must keep its own ratio
        // exactly as the user left it, not get folded into the row's
        // equal-share math.
        let (mut layout, left) = Layout::new();
        let right = layout.split(left, Orientation::Horizontal).unwrap();
        let bottom = layout.split(right, Orientation::Vertical).unwrap();
        // `resize_target`, not a guessed `dividers[..]` index — robust to
        // whatever order `geometry` happens to emit dividers in.
        let (stack_split, _) = layout.resize_target(bottom, Direction::Up).unwrap();
        assert!(layout.resize(stack_split, 0.2)); // move the vertical divider off center

        let area = Rect { x: 0.0, y: 0.0, width: 1000.0, height: 1000.0 };
        let before_bottom_height =
            layout.geometry(area, 0.0).panes.iter().find(|p| p.pane == bottom).unwrap().rect.height;

        assert!(layout.close(left));

        let after = layout.geometry(area, 0.0);
        assert_eq!(after.panes.len(), 2);
        let after_bottom_height = after.panes.iter().find(|p| p.pane == bottom).unwrap().rect.height;
        // A `Horizontal` split only ever divides *width* — the vertical
        // stack already had the area's full height before the close
        // (Horizontal splitting `left` off doesn't touch it), and still
        // does after, so its absolute height — not just its ratio — is
        // expected to be exactly unchanged.
        assert!(
            (after_bottom_height - before_bottom_height).abs() < 0.001,
            "expected the vertical stack's own ratio (and so its height) to survive the horizontal row's rebalance untouched: before={before_bottom_height}, after={after_bottom_height}"
        );
    }

    #[test]
    fn close_last_pane_fails() {
        let (mut layout, root) = Layout::new();
        assert!(!layout.close(root));
        assert_eq!(layout.panes(), vec![root]);
    }

    #[test]
    fn resize_target_finds_matching_ancestor_axis() {
        let (mut layout, root) = Layout::new();
        let right = layout.split(root, Orientation::Horizontal).unwrap();
        let bottom = layout.split(right, Orientation::Vertical).unwrap();

        // `root` and `right` are split by a Horizontal (left/right) divider;
        // Left/Right resize should find that split, with `root` on its
        // first (left) side.
        let (split, is_first) = layout.resize_target(root, Direction::Right).unwrap();
        assert!(is_first);
        let (split_from_left, _) = layout.resize_target(root, Direction::Left).unwrap();
        assert_eq!(split, split_from_left);

        // `right`/`bottom` are split by a Vertical (top/bottom) divider;
        // Up/Down resize for `bottom` should find that split, on its
        // second (bottom) side.
        let (_, bottom_is_first) = layout.resize_target(bottom, Direction::Up).unwrap();
        assert!(!bottom_is_first);

        // `root` has no Vertical ancestor split at all.
        assert_eq!(layout.resize_target(root, Direction::Up), None);
    }

    #[test]
    fn resize_clamps_to_bounds() {
        let (mut layout, root) = Layout::new();
        layout.split(root, Orientation::Horizontal).unwrap();

        let area = Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };
        let split = layout.geometry(area, 0.0).dividers[0].split;

        assert!(layout.resize(split, -10.0));
        let ratio_after_min = layout.geometry(area, 0.0).panes[0].rect.width / area.width;
        assert!((ratio_after_min - MIN_RATIO).abs() < 0.001);

        assert!(layout.resize(split, 10.0));
        let ratio_after_max = layout.geometry(area, 0.0).panes[0].rect.width / area.width;
        assert!((ratio_after_max - MAX_RATIO).abs() < 0.001);
    }

    #[test]
    fn zoom_shows_only_the_zoomed_pane() {
        let (mut layout, root) = Layout::new();
        let second = layout.split(root, Orientation::Horizontal).unwrap();
        let area = Rect { x: 0.0, y: 0.0, width: 100.0, height: 50.0 };

        layout.toggle_zoom(second);
        let zoomed_geometry = layout.geometry(area, 2.0);
        assert_eq!(zoomed_geometry.panes.len(), 1);
        assert_eq!(zoomed_geometry.panes[0].pane, second);
        assert_eq!(zoomed_geometry.panes[0].rect, area);
        assert!(zoomed_geometry.dividers.is_empty());

        layout.toggle_zoom(second);
        assert_eq!(layout.geometry(area, 2.0).panes.len(), 2);

        let _ = root;
    }

    #[test]
    fn closing_the_zoomed_pane_clears_zoom() {
        let (mut layout, root) = Layout::new();
        let second = layout.split(root, Orientation::Horizontal).unwrap();
        layout.toggle_zoom(second);

        assert!(layout.close(second));
        assert_eq!(layout.zoomed(), None);
    }

    #[test]
    fn focus_neighbor_finds_adjacent_pane() {
        let (mut layout, root) = Layout::new();
        let right = layout.split(root, Orientation::Horizontal).unwrap();
        let area = Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };

        assert_eq!(layout.focus_neighbor(root, Direction::Right, area), Some(right));
        assert_eq!(layout.focus_neighbor(right, Direction::Left, area), Some(root));
        assert_eq!(layout.focus_neighbor(root, Direction::Down, area), None);
    }

    #[test]
    fn snapshot_then_from_snapshot_recreates_an_equivalent_tree() {
        let (mut layout, root) = Layout::new();
        let right = layout.split(root, Orientation::Horizontal).unwrap();
        let bottom_right = layout.split(right, Orientation::Vertical).unwrap();
        let area = Rect { x: 0.0, y: 0.0, width: 200.0, height: 100.0 };

        let split = layout.geometry(area, 0.0).dividers[0].split;
        layout.resize(split, 0.2);
        let original_geometry = layout.geometry(area, 0.0);

        let snapshot = layout.snapshot();
        let (restored, restored_panes) = Layout::from_snapshot(&snapshot);

        assert_eq!(restored_panes.len(), 3, "should have one restored pane per original leaf");
        let restored_geometry = restored.geometry(area, 0.0);

        // Same shape and ratios (same rects), even though the actual
        // `PaneId`s are freshly assigned and won't equal the originals.
        let mut original_rects: Vec<_> = original_geometry.panes.iter().map(|p| p.rect).collect();
        let mut restored_rects: Vec<_> = restored_geometry.panes.iter().map(|p| p.rect).collect();
        original_rects.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        restored_rects.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        assert_eq!(original_rects, restored_rects);

        let _ = (root, bottom_right);
    }

    #[test]
    fn from_snapshot_panes_are_in_the_same_order_as_the_original_layouts_panes() {
        // The whole point of returning an ordered `Vec<PaneId>` rather than
        // a set: a session file's per-pane data (cwd, group, ...) is
        // correlated by position against this order, not by any id, since
        // ids can't survive a restart. If this order ever drifted from
        // `Layout::panes()`'s own order, saved per-pane data would get
        // silently attached to the wrong pane on restore.
        let (mut layout, root) = Layout::new();
        let second = layout.split(root, Orientation::Horizontal).unwrap();
        let third = layout.split(root, Orientation::Vertical).unwrap();

        let original_order = layout.panes();
        let snapshot = layout.snapshot();
        let (_, restored_order) = Layout::from_snapshot(&snapshot);

        assert_eq!(restored_order.len(), original_order.len());
        let _ = (second, third);
    }

    #[test]
    fn snapshot_round_trips_through_real_toml_serialization() {
        let (mut layout, root) = Layout::new();
        layout.split(root, Orientation::Vertical).unwrap();

        let snapshot = layout.snapshot();
        let toml_text = toml::to_string_pretty(&snapshot).expect("snapshot should serialize");
        let restored: SavedNode = toml::from_str(&toml_text).expect("snapshot should round-trip through TOML");
        assert_eq!(restored, snapshot);
    }

    /// A representative starting layout for the `arrange` tests below: 4
    /// panes in a lopsided, arbitrarily-nested shape (not already a flat
    /// row/column/grid), so rearranging it is an actual, meaningful
    /// change rather than a no-op that happened to already match.
    fn four_panes() -> (Layout, Vec<PaneId>) {
        let (mut layout, root) = Layout::new();
        let second = layout.split(root, Orientation::Horizontal).unwrap();
        let third = layout.split(second, Orientation::Vertical).unwrap();
        let fourth = layout.split(third, Orientation::Vertical).unwrap();
        (layout, vec![root, second, third, fourth])
    }

    #[test]
    fn arrange_horizontal_tiles_all_panes_in_one_equal_width_row() {
        let (_, panes) = four_panes();
        let arranged = Layout::arrange(&panes, Arrangement::Horizontal);

        let area = Rect { x: 0.0, y: 0.0, width: 400.0, height: 100.0 };
        let geometry = arranged.geometry(area, 0.0);
        assert_eq!(geometry.panes.len(), 4);
        for pane_rect in &geometry.panes {
            assert!((pane_rect.rect.width - 100.0).abs() < 0.01, "expected equal-width columns, got {pane_rect:?}");
            assert_eq!(pane_rect.rect.height, 100.0, "a single row should use the full height");
        }
    }

    #[test]
    fn arrange_vertical_tiles_all_panes_in_one_equal_height_column() {
        let (_, panes) = four_panes();
        let arranged = Layout::arrange(&panes, Arrangement::Vertical);

        let area = Rect { x: 0.0, y: 0.0, width: 100.0, height: 400.0 };
        let geometry = arranged.geometry(area, 0.0);
        assert_eq!(geometry.panes.len(), 4);
        for pane_rect in &geometry.panes {
            assert!((pane_rect.rect.height - 100.0).abs() < 0.01, "expected equal-height rows, got {pane_rect:?}");
            assert_eq!(pane_rect.rect.width, 100.0, "a single column should use the full width");
        }
    }

    #[test]
    fn arrange_grid_tiles_four_panes_into_a_2x2_square() {
        let (_, panes) = four_panes();
        let arranged = Layout::arrange(&panes, Arrangement::Grid);

        let area = Rect { x: 0.0, y: 0.0, width: 200.0, height: 200.0 };
        let geometry = arranged.geometry(area, 0.0);
        assert_eq!(geometry.panes.len(), 4);
        for pane_rect in &geometry.panes {
            assert!((pane_rect.rect.width - 100.0).abs() < 0.01);
            assert!((pane_rect.rect.height - 100.0).abs() < 0.01);
        }
    }

    #[test]
    fn arrange_grid_gives_an_uneven_count_a_shorter_last_row() {
        // 3 panes: ceil(sqrt(3)) = 2 columns, so row 1 gets 2 panes
        // (each half-width) and row 2 gets the 1 remaining (full width) —
        // not 3 equal columns or some other split.
        let (mut layout, root) = Layout::new();
        let second = layout.split(root, Orientation::Horizontal).unwrap();
        let third = layout.split(second, Orientation::Vertical).unwrap();
        let panes = vec![root, second, third];

        let area = Rect { x: 0.0, y: 0.0, width: 200.0, height: 200.0 };
        let arranged = Layout::arrange(&panes, Arrangement::Grid);
        let geometry = arranged.geometry(area, 0.0);
        assert_eq!(geometry.panes.len(), 3);

        let full_width_panes = geometry.panes.iter().filter(|p| (p.rect.width - 200.0).abs() < 0.01).count();
        let half_width_panes = geometry.panes.iter().filter(|p| (p.rect.width - 100.0).abs() < 0.01).count();
        assert_eq!(full_width_panes, 1, "the shorter last row's one pane should span the full width");
        assert_eq!(half_width_panes, 2, "the first row's two panes should split its width evenly");
    }

    #[test]
    fn arrange_keeps_the_exact_same_pane_ids_not_fresh_ones() {
        let (mut layout, root) = Layout::new();
        let second = layout.split(root, Orientation::Horizontal).unwrap();
        layout.split(second, Orientation::Vertical).unwrap();
        // Close `second` so the surviving ids (`root`, its remaining
        // sibling) are no longer a simple contiguous `0..n` range — a
        // more realistic case than always starting fresh.
        layout.close(second);
        let panes = layout.panes();

        let arranged = Layout::arrange(&panes, Arrangement::Horizontal);
        let mut arranged_panes = arranged.panes();
        arranged_panes.sort();
        let mut original_panes = panes.clone();
        original_panes.sort();
        assert_eq!(arranged_panes, original_panes, "arrange must reuse the given ids exactly, not assign fresh ones");
    }

    #[test]
    fn arrange_with_a_single_pane_produces_one_full_area_leaf() {
        let (_, root) = Layout::new();
        let area = Rect { x: 0.0, y: 0.0, width: 300.0, height: 150.0 };
        for arrangement in [Arrangement::Horizontal, Arrangement::Vertical, Arrangement::Grid] {
            let arranged = Layout::arrange(&[root], arrangement);
            let geometry = arranged.geometry(area, 0.0);
            assert_eq!(geometry.panes.len(), 1);
            assert_eq!(geometry.panes[0].rect, area);
        }
    }

    #[test]
    fn after_arrange_a_new_split_does_not_collide_with_a_kept_pane_id() {
        let (mut layout, root) = Layout::new();
        let second = layout.split(root, Orientation::Horizontal).unwrap();
        layout.split(second, Orientation::Vertical).unwrap();
        layout.close(second); // leaves a non-contiguous id set behind
        let panes = layout.panes();

        let mut arranged = Layout::arrange(&panes, Arrangement::Horizontal);
        let new_pane = arranged.split(panes[0], Orientation::Vertical).expect("split should still work after arrange");
        assert!(!panes.contains(&new_pane), "a freshly split pane must not reuse a kept pane's id");
    }
}
