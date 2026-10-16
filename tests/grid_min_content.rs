//! The min-content height of a grid is its max-content height (CSS Sizing 3 §5.1).
//!
//! Rows sized under a min-content height get no free space. A row of scroll-container items
//! starts at their zero minimum, so it would collapse.

use cephal::geometry::{AvailableSpace, Point, Size};
use cephal::style::{Dimension, Display, FlexDirection, LengthPercentage, Overflow, MaxTrackSizingFunction, MinTrackSizingFunction, TrackSizingFunction};
use cephal::tree::NodeId;
use cephal::{Style, Tree};

const VIEWPORT: Size<AvailableSpace> =
    Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(600.0) };

/// A grid of four clipping cells in two `minmax(0, 1fr)` columns, each cell holding a 200px tall
/// box, with `height`.
fn grid(tree: &mut Tree<()>, height: Dimension) -> NodeId {
    let mut cells = Vec::new();
    for _ in 0..4 {
        let mut content = Style::DEFAULT;
        content.size.height = Dimension::length(200.0);
        let content = tree.new_leaf(content);
        let mut cell = Style::DEFAULT;
        cell.overflow = Point { x: Overflow::Hidden, y: Overflow::Hidden };
        cells.push(tree.new_with_children(cell, &[content]));
    }
    let mut style = Style::DEFAULT;
    style.display = Display::Grid;
    style.size.height = height;
    style.gap.height = LengthPercentage::length(10.0);
    let column = TrackSizingFunction::minmax(MinTrackSizingFunction::length(0.0), MaxTrackSizingFunction::fr(1.0));
    style.grid_mut().template_columns = vec![column.into(), column.into()];
    tree.new_with_children(style, &cells)
}

/// Lays out a grid of `height` in a 260px tall scrolling column and returns the grid's height.
fn in_scrolling_column(height: Dimension) -> f32 {
    let mut tree: Tree<()> = Tree::new();
    let grid = grid(&mut tree, height);
    let mut column = Style::DEFAULT;
    column.display = Display::Flex;
    column.flex_direction = FlexDirection::Column;
    column.size = Size { width: Dimension::length(400.0), height: Dimension::length(260.0) };
    column.overflow = Point { x: Overflow::Visible, y: Overflow::Scroll };
    let column = tree.new_with_children(column, &[grid]);
    tree.compute_layout(column, VIEWPORT);
    tree.unrounded_layout(grid).size.height
}

#[test]
fn a_grid_in_a_scrolling_column_keeps_its_content_height() {
    assert_eq!(in_scrolling_column(Dimension::AUTO), 410.0, "two 200px rows and a gap");
}

#[test]
fn min_content_height_equals_max_content_height() {
    assert_eq!(in_scrolling_column(Dimension::MIN_CONTENT), 410.0);
    assert_eq!(in_scrolling_column(Dimension::MAX_CONTENT), 410.0);
}
