//! Turns a [`Node`] tree into screen rects: one per pane, one per divider.

use crate::{Direction, Node, Orientation, PaneId, SplitId};

/// A rectangle in caller-defined units (pixels, typically).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Where a pane's grid should be drawn.
#[derive(Debug, Clone, Copy)]
pub struct PaneRect {
    pub pane: PaneId,
    pub rect: Rect,
}

/// Where a split's divider should be drawn (and hit-tested for drag-resize).
#[derive(Debug, Clone, Copy)]
pub struct DividerRect {
    pub split: SplitId,
    pub orientation: Orientation,
    pub rect: Rect,
    /// Length, in the same units as `rect`, of the area this divider splits
    /// along its resize axis (width for `Horizontal`, height for
    /// `Vertical`) — the parent area *before* subdivision, not either
    /// child's rect. Converts a drag's pixel delta into a ratio delta:
    /// `delta_ratio = pixel_delta / axis_extent`.
    pub axis_extent: f32,
}

/// The full geometry of a layout for one frame.
#[derive(Debug, Clone)]
pub struct Geometry {
    pub panes: Vec<PaneRect>,
    pub dividers: Vec<DividerRect>,
}

pub(crate) fn compute(root: &Node, area: Rect, divider_thickness: f32) -> Geometry {
    let mut panes = Vec::new();
    let mut dividers = Vec::new();
    layout_node(root, area, divider_thickness, &mut panes, &mut dividers);
    Geometry { panes, dividers }
}

fn layout_node(
    node: &Node,
    area: Rect,
    divider_thickness: f32,
    panes: &mut Vec<PaneRect>,
    dividers: &mut Vec<DividerRect>,
) {
    match node {
        Node::Leaf(id) => panes.push(PaneRect { pane: *id, rect: area }),
        Node::Split {
            id,
            orientation,
            ratio,
            first,
            second,
        } => {
            let (first_rect, divider_rect, second_rect) = split_area(area, *orientation, *ratio, divider_thickness);
            let axis_extent = match orientation {
                Orientation::Horizontal => area.width,
                Orientation::Vertical => area.height,
            };
            dividers.push(DividerRect {
                split: *id,
                orientation: *orientation,
                rect: divider_rect,
                axis_extent,
            });
            layout_node(first, first_rect, divider_thickness, panes, dividers);
            layout_node(second, second_rect, divider_thickness, panes, dividers);
        }
    }
}

/// Splits `area` along `orientation`'s axis at `ratio`, leaving a gap of
/// `divider_thickness` between the two halves for the divider itself.
fn split_area(area: Rect, orientation: Orientation, ratio: f32, divider_thickness: f32) -> (Rect, Rect, Rect) {
    match orientation {
        Orientation::Horizontal => {
            let first_width = ((area.width - divider_thickness).max(0.0) * ratio).max(0.0);
            let second_width = (area.width - divider_thickness - first_width).max(0.0);
            let first = Rect { x: area.x, y: area.y, width: first_width, height: area.height };
            let divider = Rect {
                x: area.x + first_width,
                y: area.y,
                width: divider_thickness,
                height: area.height,
            };
            let second = Rect {
                x: area.x + first_width + divider_thickness,
                y: area.y,
                width: second_width,
                height: area.height,
            };
            (first, divider, second)
        }
        Orientation::Vertical => {
            let first_height = ((area.height - divider_thickness).max(0.0) * ratio).max(0.0);
            let second_height = (area.height - divider_thickness - first_height).max(0.0);
            let first = Rect { x: area.x, y: area.y, width: area.width, height: first_height };
            let divider = Rect {
                x: area.x,
                y: area.y + first_height,
                width: area.width,
                height: divider_thickness,
            };
            let second = Rect {
                x: area.x,
                y: area.y + first_height + divider_thickness,
                width: area.width,
                height: second_height,
            };
            (first, divider, second)
        }
    }
}

/// Returns the gap distance from `from` to `to` if `to` lies in `direction`
/// from `from` and their perpendicular ranges overlap, `None` otherwise.
pub(crate) fn adjacency(from: Rect, to: Rect, direction: Direction) -> Option<f32> {
    let vertical_overlap = to.y < from.y + from.height && to.y + to.height > from.y;
    let horizontal_overlap = to.x < from.x + from.width && to.x + to.width > from.x;

    match direction {
        Direction::Right if vertical_overlap && to.x >= from.x + from.width => Some(to.x - (from.x + from.width)),
        Direction::Left if vertical_overlap && to.x + to.width <= from.x => Some(from.x - (to.x + to.width)),
        Direction::Down if horizontal_overlap && to.y >= from.y + from.height => Some(to.y - (from.y + from.height)),
        Direction::Up if horizontal_overlap && to.y + to.height <= from.y => Some(from.y - (to.y + to.height)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Layout, Orientation};

    #[test]
    fn panes_tile_the_area_with_no_gaps_or_overlaps() {
        let (mut layout, root) = Layout::new();
        let right = layout.split(root, Orientation::Horizontal).unwrap();
        layout.split(right, Orientation::Vertical).unwrap();

        let area = Rect { x: 0.0, y: 0.0, width: 200.0, height: 100.0 };
        let geometry = layout.geometry(area, 0.0);

        assert_eq!(geometry.panes.len(), 3);
        let total_area: f32 = geometry.panes.iter().map(|p| p.rect.width * p.rect.height).sum();
        assert!((total_area - area.width * area.height).abs() < 0.01);

        for pane in &geometry.panes {
            assert!(pane.rect.x >= area.x && pane.rect.x + pane.rect.width <= area.x + area.width);
            assert!(pane.rect.y >= area.y && pane.rect.y + pane.rect.height <= area.y + area.height);
        }
    }

    #[test]
    fn nested_divider_axis_extent_is_the_local_parent_span_not_the_window() {
        let (mut layout, root) = Layout::new();
        let right = layout.split(root, Orientation::Horizontal).unwrap();
        layout.split(right, Orientation::Vertical).unwrap();

        // The root split (Horizontal) divides the full 200-wide window, so
        // its divider's axis extent is the window width. The nested split
        // (Vertical) divides the right pane's height, which the root split
        // never touched — its axis extent is the window height, not
        // anything derived from the root split's width math.
        let area = Rect { x: 0.0, y: 0.0, width: 200.0, height: 100.0 };
        let geometry = layout.geometry(area, 4.0);

        let root_divider = geometry.dividers.iter().find(|d| d.orientation == Orientation::Horizontal).unwrap();
        assert_eq!(root_divider.axis_extent, 200.0);

        let nested_divider = geometry.dividers.iter().find(|d| d.orientation == Orientation::Vertical).unwrap();
        assert_eq!(nested_divider.axis_extent, 100.0);
    }

    #[test]
    fn divider_occupies_the_gap_between_panes() {
        let (mut layout, root) = Layout::new();
        layout.split(root, Orientation::Horizontal).unwrap();

        let area = Rect { x: 0.0, y: 0.0, width: 100.0, height: 50.0 };
        let geometry = layout.geometry(area, 4.0);

        assert_eq!(geometry.panes.len(), 2);
        assert_eq!(geometry.dividers.len(), 1);

        let first = geometry.panes[0].rect;
        let divider = geometry.dividers[0].rect;
        let second = geometry.panes[1].rect;

        assert_eq!(first.x + first.width, divider.x);
        assert_eq!(divider.x + divider.width, second.x);
        assert_eq!(first.width + divider.width + second.width, area.width);
    }
}
