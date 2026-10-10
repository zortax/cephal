//! Containers report a last baseline of their own, not a copy of their first.

use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{Dimension, Display, FlexDirection, FlexWrap, GridTemplateComponent, TrackSizingFunction};
use cephal::tree::{Baselines, MeasureInput, MeasureOutput, NodeId, RunMode};
use cephal::{Style, Tree};

/// Two lines of text: 60×20, baselines at 5 and 15.
fn measure(_: &mut (), input: MeasureInput, _: &Style) -> MeasureOutput {
    MeasureOutput {
        size: input.known_dimensions.unwrap_or(Size { width: 60.0, height: 20.0 }),
        baselines: Baselines { first: Some(5.0), last: Some(15.0) },
    }
}

/// Lays out a 100px wide container holding three text leaves and returns its own baselines.
fn baselines(container: Style) -> Baselines {
    let mut tree: Tree<()> = Tree::new();
    let leaves: Vec<NodeId> = (0..3).map(|_| tree.new_leaf_with_context(Style::DEFAULT, ())).collect();
    let mut container = container;
    container.size.width = Dimension::length(100.0);
    let root = tree.new_with_children(container, &leaves);
    let viewport = Size { width: AvailableSpace::Definite(400.0), height: AvailableSpace::MaxContent };
    tree.compute_layout_with_measure(root, viewport, measure);
    let (_, output) = tree
        .cache_entries(root)
        .into_iter()
        .find(|(input, _)| input.run_mode == RunMode::PerformLayout)
        .expect("the root's final answer is cached");
    output.baselines
}

#[test]
fn block_last_baseline_is_the_last_childs() {
    // Children stack at 0, 20 and 40.
    let mut style = Style::DEFAULT;
    style.display = Display::Block;
    assert_eq!(baselines(style), Baselines { first: Some(5.0), last: Some(55.0) });
}

#[test]
fn flex_last_baseline_comes_from_the_last_line() {
    // One 60px item per line: lines at 0, 20 and 40.
    let mut style = Style::DEFAULT;
    style.display = Display::Flex;
    style.flex_wrap = FlexWrap::Wrap;
    assert_eq!(baselines(style.clone()), Baselines { first: Some(5.0), last: Some(55.0) });
    // Reversed lines stack upwards, so the first line is the bottom one; baselines stay visual.
    style.flex_wrap = FlexWrap::WrapReverse;
    assert_eq!(baselines(style), Baselines { first: Some(5.0), last: Some(55.0) });
}

#[test]
fn flex_column_last_baseline_is_the_endmost_items() {
    let mut style = Style::DEFAULT;
    style.display = Display::Flex;
    style.flex_direction = FlexDirection::Column;
    assert_eq!(baselines(style.clone()), Baselines { first: Some(5.0), last: Some(55.0) });
    style.flex_direction = FlexDirection::ColumnReverse;
    assert_eq!(baselines(style), Baselines { first: Some(5.0), last: Some(55.0) });
}

#[test]
fn grid_last_baseline_comes_from_the_last_row() {
    // Two 50px columns: rows at 0 and 20, the third item alone in the second.
    let mut style = Style::DEFAULT;
    style.display = Display::Grid;
    let column = GridTemplateComponent::Single(TrackSizingFunction::length(50.0));
    style.grid_mut().template_columns = vec![column.clone(), column];
    let out = baselines(style);
    assert_eq!(out.last, Some(35.0));
}
