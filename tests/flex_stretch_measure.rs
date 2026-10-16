//! A flex item's main size is measured at the cross size the item gets in the final layout.
//!
//! A stretched item in a column container with an indefinite width gets the container's
//! fit-content width (§9.8), and an item with a sizing keyword on its cross size gets that size.

mod common;

use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{AlignItems, Dimension, Display, FlexDirection, MaxTrackSizingFunction, MinTrackSizingFunction, TrackSizingFunction};
use cephal::tree::NodeId;
use cephal::{Style, Tree};
use common::{Text, measure};

const VIEWPORT: Size<AvailableSpace> =
    Size { width: AvailableSpace::Definite(800.0), height: AvailableSpace::Definite(600.0) };

fn column() -> Style {
    let mut style = Style::DEFAULT;
    style.display = Display::Flex;
    style.flex_direction = FlexDirection::Column;
    style
}

/// A month: a grid of seven `minmax(32px, 1fr)` columns holding 42 square days.
fn month(tree: &mut Tree<()>) -> NodeId {
    let mut day = Style::DEFAULT;
    day.aspect_ratio = Some(1.0);
    day.size.width = Dimension::percent(1.0);
    let days: Vec<NodeId> = (0..42).map(|_| tree.new_leaf(day.clone())).collect();
    let mut grid = Style::DEFAULT;
    grid.display = Display::Grid;
    let track = TrackSizingFunction::minmax(MinTrackSizingFunction::length(32.0), MaxTrackSizingFunction::fr(1.0));
    grid.grid_mut().template_columns = vec![track.into(); 7];
    tree.new_with_children(grid, &days)
}

#[test]
fn a_stretched_grid_in_a_centred_column_is_measured_at_the_column_width() {
    let mut tree: Tree<()> = Tree::new();
    let grid = month(&mut tree);
    let calendar = tree.new_with_children(column(), &[grid]);
    let mut page = column();
    page.size.width = Dimension::length(800.0);
    page.align_items = Some(AlignItems::CENTER);
    let page = tree.new_with_children(page, &[calendar]);
    tree.compute_layout(page, VIEWPORT);
    assert_eq!(tree.unrounded_layout(calendar).size, Size { width: 224.0, height: 192.0 });
    assert_eq!(tree.unrounded_layout(grid).size, Size { width: 224.0, height: 192.0 });
}

#[test]
fn a_cross_size_keyword_is_used_to_measure_the_main_size() {
    // Twenty 10px glyphs, one per line at their min-content width.
    let mut tree: Tree<Text> = Tree::new();
    let mut item = Style::DEFAULT;
    item.size.width = Dimension::MIN_CONTENT;
    let item = tree.new_leaf_with_context(item, Text(20));
    let mut container = column();
    container.size.width = Dimension::length(300.0);
    let container = tree.new_with_children(container, &[item]);
    tree.compute_layout_with_measure(container, VIEWPORT, measure);
    assert_eq!(tree.unrounded_layout(item).size, Size { width: 10.0, height: 200.0 });
}
