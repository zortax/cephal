//! `Layout::margin` of in-flow block children holds their used margins, which fixtures cannot check.

use cephal::geometry::{AvailableSpace, Rect, Size};
use cephal::style::{Dimension, Display, Float, LengthPercentageAuto, Position};
use cephal::tree::NodeId;
use cephal::{Style, Tree};

fn block() -> Style {
    let mut s = Style::DEFAULT;
    s.display = Display::Block;
    s
}

fn margins(top: f32, right: Option<f32>, bottom: f32, left: Option<f32>) -> Rect<LengthPercentageAuto> {
    let side = |v: Option<f32>| v.map_or(LengthPercentageAuto::AUTO, LengthPercentageAuto::length);
    Rect { top: LengthPercentageAuto::length(top), right: side(right), bottom: LengthPercentageAuto::length(bottom), left: side(left) }
}

fn layout(tree: &mut Tree<()>, root: NodeId) {
    tree.disable_rounding();
    tree.compute_layout(root, Size { width: AvailableSpace::Definite(400.0), height: AvailableSpace::MaxContent });
}

#[test]
fn collapsed_margins_stay_out_of_the_used_margin() {
    let mut tree: Tree<()> = Tree::new();
    let mut inner = block();
    inner.margin = margins(13.0, Some(0.0), 5.0, Some(0.0));
    inner.size.height = Dimension::length(10.0);
    let inner = tree.new_leaf(inner);
    let mut outer = block();
    outer.margin = margins(8.0, Some(0.0), 2.0, Some(0.0));
    let outer = tree.new_with_children(outer, &[inner]);
    let mut sibling = block();
    sibling.margin = margins(20.0, Some(0.0), 0.0, Some(0.0));
    let sibling = tree.new_leaf(sibling);
    let mut root = block();
    root.position = Position::Absolute;
    root.size.width = Dimension::length(100.0);
    let root = tree.new_with_children(root, &[outer, sibling]);
    layout(&mut tree, root);

    // The margins collapse: outer's border box sits at 13, its child at its top, the sibling 20 below.
    assert_eq!(tree.layout(outer).location.y, 13.0);
    assert_eq!(tree.layout(inner).location.y, 0.0);
    assert_eq!(tree.layout(sibling).location.y, 13.0 + 10.0 + 20.0);
    assert_eq!((tree.layout(outer).margin.top, tree.layout(outer).margin.bottom), (8.0, 2.0));
    assert_eq!((tree.layout(inner).margin.top, tree.layout(inner).margin.bottom), (13.0, 5.0));
    assert_eq!(tree.layout(sibling).margin.top, 20.0);
}

#[test]
fn auto_margins_beside_floats_reach_the_content_edges() {
    // csswg-drafts#9174: `margin: 0 auto` on a 50px flow root beside a 20px right float in 100px
    // is centred in the 80px left by the float, with used margins 15px and 35px.
    let mut tree: Tree<()> = Tree::new();
    let mut float = block();
    float.float = Float::Right;
    float.size = Size { width: Dimension::length(20.0), height: Dimension::length(20.0) };
    let float = tree.new_leaf(float);
    let mut boxed = block();
    boxed.display = Display::Flex;
    boxed.size = Size { width: Dimension::length(50.0), height: Dimension::length(10.0) };
    boxed.margin = margins(0.0, None, 0.0, None);
    let boxed = tree.new_leaf(boxed);
    let mut root = block();
    root.position = Position::Absolute;
    root.size.width = Dimension::length(100.0);
    let root = tree.new_with_children(root, &[float, boxed]);
    layout(&mut tree, root);

    assert_eq!(tree.layout(float).location.x, 80.0);
    assert_eq!(tree.layout(boxed).location.x, 15.0);
    let margin = tree.layout(boxed).margin;
    assert_eq!((margin.left, margin.right), (15.0, 35.0));
}
