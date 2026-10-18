//! Incremental pixel snapping equals a cold one after structural edits and rounding toggles.

use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{Dimension, Display, FlexDirection, Style};
use cephal::tree::NodeId;
use cephal::Tree;

const VIEWPORT: Size<AvailableSpace> = Size { width: AvailableSpace::Definite(400.0), height: AvailableSpace::MaxContent };

fn sized(width: f32, height: f32) -> Style {
    let mut s = Style::DEFAULT;
    s.size = Size { width: Dimension::length(width), height: Dimension::length(height) };
    s
}

fn row() -> Style {
    let mut s = Style::DEFAULT;
    s.display = Display::Flex;
    s.flex_direction = FlexDirection::Row;
    s
}

fn assert_matches_cold(tree: &Tree<()>, root: NodeId, nodes: &[NodeId]) {
    let mut cold = tree.clone_cold();
    cold.compute_layout(root, VIEWPORT);
    for &n in nodes {
        assert_eq!(tree.layout(n), cold.layout(n), "rounded layout of {n:?}");
    }
}

/// Frames laid out without rounding leave nothing to re-round from.
#[test]
fn reenabled_rounding_rounds_everything() {
    let mut tree: Tree<()> = Tree::new();
    let a = tree.new_leaf(sized(10.4, 5.0));
    let b = tree.new_leaf(sized(10.4, 5.0));
    let root = tree.new_with_children(row(), &[a, b]);
    tree.compute_layout(root, VIEWPORT);
    tree.disable_rounding();
    tree.patch_style(a, |s| s.size.width = Dimension::length(20.3));
    tree.compute_layout(root, VIEWPORT);
    tree.enable_rounding();
    tree.compute_layout(root, VIEWPORT);
    assert_matches_cold(&tree, root, &[root, a, b]);
}

