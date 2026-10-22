//! How often leaves are measured, ported from Taffy 0.15's `measure_count_*` tests: sizing must not
//! measure what it does not use. The counts are cephal's own. `verify-cache` measures again what
//! shortcuts and cache hits answer, so the counts hold without it only.

use cephal::geometry::{AvailableSpace, Line, Size};
use cephal::style::{
    AlignItems, Dimension, Display, FlexDirection, GridPlacement, MaxTrackSizingFunction, MinTrackSizingFunction, TrackSizingFunction,
};
use cephal::tree::{MeasureInput, MeasureOutput, NodeId};
use cephal::{Style, Tree};

/// A leaf's content, counting its measurements: a fixed size, or two-glyph words of 10px glyphs on
/// 10px lines that wrap between words, as Taffy's `HH\u{200B}HH\u{200B}HH\u{200B}HH` in Ahem.
struct Content {
    fixed: Option<Size<f32>>,
    words: u32,
    count: u32,
}

impl Content {
    fn fixed(width: f32, height: f32) -> Self {
        Self { fixed: Some(Size { width, height }), words: 0, count: 0 }
    }
    /// Min-content width 20, max-content width 80.
    fn text() -> Self {
        Self { fixed: None, words: 4, count: 0 }
    }
}

fn measure(ctx: &mut Content, input: MeasureInput, _: &Style) -> MeasureOutput {
    ctx.count += 1;
    if let Some(size) = ctx.fixed {
        return MeasureOutput::from_size(input.known_dimensions.unwrap_or(size));
    }
    let word = 20.0;
    let total = word * ctx.words as f32;
    let width = input.known_dimensions.width.unwrap_or(match input.available_space.width {
        AvailableSpace::MinContent => word,
        AvailableSpace::MaxContent => total,
        AvailableSpace::Definite(w) => w.min(total).max(word),
    });
    let per_line = (width / word).floor().max(1.0);
    let lines = (ctx.words as f32 / per_line).ceil().max(1.0);
    let height = input.known_dimensions.height.unwrap_or(lines * 10.0);
    MeasureOutput::from_size(Size { width, height })
}

fn style(display: Display) -> Style {
    let mut s = Style::DEFAULT;
    s.display = display;
    s
}

/// Lays out `root` under a max-content viewport; returns how often `leaf` was measured.
fn layout(tree: &mut Tree<Content>, root: NodeId, leaf: NodeId) -> u32 {
    tree.compute_layout_with_measure(root, Size::MAX_CONTENT, measure);
    tree.context(leaf).unwrap().count
}

fn size(tree: &Tree<Content>, node: NodeId) -> Size<f32> {
    tree.unrounded_layout(node).size
}

/// A leaf under a hundred nested containers is measured a bounded number of times.
#[test]
#[cfg_attr(feature = "verify-cache", ignore)]
fn measure_count_flexbox() {
    let mut tree: Tree<Content> = Tree::new();
    let leaf = tree.new_leaf_with_context(Style::DEFAULT, Content::fixed(50.0, 50.0));
    let mut node = tree.new_with_children(Style::DEFAULT, &[leaf]);
    for _ in 0..100 {
        node = tree.new_with_children(Style::DEFAULT, &[node]);
    }
    assert_eq!(layout(&mut tree, node, leaf), 2);
    assert_eq!(size(&tree, leaf), Size { width: 50.0, height: 50.0 });
}

/// The same with a grid-styled leaf.
#[test]
#[cfg_attr(feature = "verify-cache", ignore)]
fn measure_count_grid() {
    let mut tree: Tree<Content> = Tree::new();
    let leaf = tree.new_leaf_with_context(style(Display::Grid), Content::fixed(50.0, 50.0));
    let mut node = tree.new_with_children(Style::DEFAULT, &[leaf]);
    for _ in 0..100 {
        node = tree.new_with_children(Style::DEFAULT, &[node]);
    }
    assert_eq!(layout(&mut tree, node, leaf), 2);
    assert_eq!(size(&tree, leaf), Size { width: 50.0, height: 50.0 });
}

/// A grid asked only for its width, as an item of a grid whose columns are being sized, does not
/// size its rows: that would measure its items' heights at widths no later question asks about.
/// (Sizing them measures the leaf 10 times.)
#[test]
#[cfg_attr(feature = "verify-cache", ignore)]
fn measure_count_nested_grid() {
    let mut tree: Tree<Content> = Tree::new();
    let leaf = tree.new_leaf_with_context(Style::DEFAULT, Content::text());
    let inner = tree.new_with_children(style(Display::Grid), &[leaf]);
    let middle = tree.new_with_children(style(Display::Grid), &[inner]);
    let mut outer = style(Display::Grid);
    outer.size.width = Dimension::length(50.0);
    let outer = tree.new_with_children(outer, &[middle]);
    assert_eq!(layout(&mut tree, outer, leaf), 6);
    assert_eq!(size(&tree, leaf), Size { width: 50.0, height: 20.0 });
}

/// An item whose width is known contributes that width to its column without being measured.
/// (Measuring it makes 5.)
#[test]
#[cfg_attr(feature = "verify-cache", ignore)]
fn measure_count_grid_item_with_known_width() {
    let mut tree: Tree<Content> = Tree::new();
    let mut s = Style::DEFAULT;
    s.size.width = Dimension::length(40.0);
    let leaf = tree.new_leaf_with_context(s, Content::text());
    let grid = tree.new_with_children(style(Display::Grid), &[leaf]);
    assert_eq!(layout(&mut tree, grid, leaf), 3);
    assert_eq!(size(&tree, grid), Size { width: 40.0, height: 20.0 });
    assert_eq!(size(&tree, leaf), Size { width: 40.0, height: 20.0 });
}

/// An item in a fixed column contributes nothing to it, so sizing the columns does not measure
/// it, even with an intrinsic column beside it. (Measuring it makes 5.)
#[test]
#[cfg_attr(feature = "verify-cache", ignore)]
fn measure_count_grid_item_in_fixed_track() {
    let mut tree: Tree<Content> = Tree::new();
    let mut s = Style::DEFAULT;
    s.justify_self = Some(AlignItems::START);
    s.grid_column = Line { start: GridPlacement::Line(1), end: GridPlacement::Auto };
    let leaf = tree.new_leaf_with_context(s, Content::text());
    let mut s = Style::DEFAULT;
    s.size = Size { width: Dimension::length(50.0), height: Dimension::length(50.0) };
    s.grid_column = Line { start: GridPlacement::Line(2), end: GridPlacement::Auto };
    let sibling = tree.new_leaf(s);
    let mut grid = style(Display::Grid);
    grid.grid_mut().template_columns = vec![TrackSizingFunction::length(100.0).into(), TrackSizingFunction::AUTO.into()];
    let grid = tree.new_with_children(grid, &[leaf, sibling]);
    assert_eq!(layout(&mut tree, grid, leaf), 3);
    assert_eq!(size(&tree, grid), Size { width: 150.0, height: 50.0 });
    assert_eq!(size(&tree, leaf), Size { width: 80.0, height: 50.0 });
}

/// A flex container asked only for its main size does not measure its items' cross sizes.
#[test]
#[cfg_attr(feature = "verify-cache", ignore)]
fn measure_count_flex_in_grid() {
    for (direction, expected) in [(FlexDirection::Row, 9), (FlexDirection::Column, 9)] {
        let mut tree: Tree<Content> = Tree::new();
        let leaf = tree.new_leaf_with_context(Style::DEFAULT, Content::text());
        let mut flex = style(Display::Flex);
        flex.flex_direction = direction;
        let flex = tree.new_with_children(flex, &[leaf]);
        let mut grid = style(Display::Grid);
        grid.size.width = Dimension::length(50.0);
        let grid = tree.new_with_children(grid, &[flex]);
        assert_eq!(layout(&mut tree, grid, leaf), expected, "{direction:?}");
        assert_eq!(size(&tree, leaf), Size { width: 50.0, height: 20.0 }, "{direction:?}");
    }
}

/// An item spanning only tracks no intrinsic step sizes (a definite minimum, a flexible maximum)
/// is measured by its final layout alone.
#[test]
#[cfg_attr(feature = "verify-cache", ignore)]
fn grid_item_spanning_only_unaffected_tracks_is_not_measured() {
    let mut tree: Tree<Content> = Tree::new();
    let mut s = Style::DEFAULT;
    s.grid_column = Line { start: GridPlacement::Line(2), end: GridPlacement::Line(3) };
    let leaf = tree.new_leaf_with_context(s, Content::text());
    let mut grid = style(Display::Grid);
    grid.size = Size { width: Dimension::length(400.0), height: Dimension::length(50.0) };
    let flexible = TrackSizingFunction::minmax(MinTrackSizingFunction::length(0.0), MaxTrackSizingFunction::fr(1.0));
    grid.grid_mut().template_columns = vec![TrackSizingFunction::length(100.0).into(), flexible.into()];
    grid.grid_mut().template_rows = vec![TrackSizingFunction::length(50.0).into()];
    let grid = tree.new_with_children(grid, &[leaf]);
    assert_eq!(layout(&mut tree, grid, leaf), 1);
    assert_eq!(size(&tree, leaf).width, 300.0);
}
