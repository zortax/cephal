//! A fixed box is placed like an absolute one but adds nothing to its container's overflow.

use cephal::geometry::{AvailableSpace, Point, Rect, Size};
use cephal::style::{Dimension, Display, LengthPercentageAuto, Overflow, Position};
use cephal::tree::NodeId;
use cephal::{Style, Tree};

const VIEWPORT: Size<AvailableSpace> = Size { width: AvailableSpace::Definite(400.0), height: AvailableSpace::Definite(300.0) };

/// A 100×100 scroll container holding one in-flow child and one out-of-flow child that overflows
/// it to 300×250.
fn build(display: Display, position: Position) -> (Tree<()>, NodeId, NodeId) {
    let mut tree: Tree<()> = Tree::new();
    let mut inflow = Style::DEFAULT;
    inflow.size = Size { width: Dimension::length(50.0), height: Dimension::length(50.0) };
    let mut out = Style::DEFAULT;
    out.position = position;
    out.inset.left = LengthPercentageAuto::length(200.0);
    out.inset.top = LengthPercentageAuto::length(150.0);
    out.size = Size { width: Dimension::length(100.0), height: Dimension::length(100.0) };
    let mut container = Style::DEFAULT;
    container.display = display;
    container.overflow = Point { x: Overflow::Scroll, y: Overflow::Scroll };
    container.size = Size { width: Dimension::length(100.0), height: Dimension::length(100.0) };
    let a = tree.new_leaf(inflow);
    let b = tree.new_leaf(out);
    let root = tree.new_with_children(container, &[a, b]);
    tree.compute_layout(root, VIEWPORT);
    (tree, root, b)
}

fn overflow(tree: &Tree<()>, node: NodeId) -> Rect<f32> {
    tree.unrounded_layout(node).scrollable_overflow_rect
}

#[test]
fn a_fixed_child_adds_no_overflow_and_is_placed_like_an_absolute_one() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let (absolute, root, abs_child) = build(display, Position::Absolute);
        let (fixed, _, fixed_child) = build(display, Position::Fixed);
        assert_eq!(overflow(&absolute, root).right, 300.0, "{display:?}: the absolute child overflows");
        // What remains is the in-flow child's 50×50.
        assert_eq!(overflow(&fixed, root).right, 50.0, "{display:?}: the fixed child adds nothing");
        assert_eq!(overflow(&fixed, root).bottom, 50.0, "{display:?}: the fixed child adds nothing");
        assert_eq!(absolute.unrounded_layout(abs_child), fixed.unrounded_layout(fixed_child), "{display:?}: same placement");
    }
}

#[test]
fn toggling_fixed_and_moving_a_fixed_child_match_a_cold_layout() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let (mut tree, root, child) = build(display, Position::Absolute);
        tree.patch_style(child, |s| s.position = Position::Fixed);
        tree.compute_layout(root, VIEWPORT);
        assert_eq!(overflow(&tree, root).right, 50.0, "{display:?}: toggled to fixed");
        // A position-only edit takes the re-placement path, which must not reintroduce overflow.
        tree.patch_style(child, |s| s.inset.left = LengthPercentageAuto::length(250.0));
        tree.compute_layout(root, VIEWPORT);
        let mut cold = tree.clone_cold();
        cold.compute_layout(root, VIEWPORT);
        assert_eq!(overflow(&tree, root), overflow(&cold, root), "{display:?}: moved fixed child");
        assert_eq!(overflow(&tree, root).right, 50.0, "{display:?}: moved fixed child");
        tree.patch_style(child, |s| s.position = Position::Absolute);
        tree.compute_layout(root, VIEWPORT);
        assert_eq!(overflow(&tree, root).right, 350.0, "{display:?}: toggled back to absolute");
    }
}

#[test]
fn fixed_parses() {
    assert_eq!(cephal::style::parse::parse_position("fixed"), Some(Position::Fixed));
}
