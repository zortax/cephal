//! `normal` self-alignment sizes replaced grid items as for `start`, not `stretch` (css-grid-1 §6.2).

use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{AlignItems, Display, TrackSizingFunction};
use cephal::{Style, Tree};

/// The size of a replaced item without a size in a 100px × 100px grid area.
fn replaced_item_size(align_items: Option<AlignItems>, justify_items: Option<AlignItems>) -> Size<f32> {
    let mut tree: Tree<()> = Tree::new();
    let mut item = Style::DEFAULT;
    item.item_is_replaced = true;
    let item = tree.new_leaf(item);
    let mut grid = Style::DEFAULT;
    grid.display = Display::Grid;
    grid.align_items = align_items;
    grid.justify_items = justify_items;
    grid.grid_mut().template_columns = vec![TrackSizingFunction::length(100.0).into()];
    grid.grid_mut().template_rows = vec![TrackSizingFunction::length(100.0).into()];
    let grid = tree.new_with_children(grid, &[item]);
    tree.compute_layout(grid, Size { width: AvailableSpace::MaxContent, height: AvailableSpace::MaxContent });
    tree.unrounded_layout(item).size
}

#[test]
fn normal_does_not_stretch_replaced_items() {
    assert_eq!(replaced_item_size(None, None), Size { width: 0.0, height: 0.0 });
}

#[test]
fn stretch_stretches_replaced_items() {
    assert_eq!(replaced_item_size(Some(AlignItems::STRETCH), Some(AlignItems::STRETCH)), Size { width: 100.0, height: 100.0 });
}
