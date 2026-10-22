//! Block flow cases the Chrome fixtures do not cover.

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

#[test]
fn floats_count_from_a_childs_collapsed_position() {
    // A flow root holds an empty box with a 30px bottom margin, then a box with a 20px top margin
    // holding a 50px float. The margins collapse to 30px, so the float spans 30..80.
    let mut tree: Tree<()> = Tree::new();
    let mut empty = block();
    empty.margin = margins(0.0, Some(0.0), 30.0, Some(0.0));
    let empty = tree.new_leaf(empty);
    let mut float = block();
    float.float = Float::Left;
    float.size = Size { width: Dimension::length(20.0), height: Dimension::length(50.0) };
    let float = tree.new_leaf(float);
    let mut holder = block();
    holder.margin = margins(20.0, Some(0.0), 0.0, Some(0.0));
    let holder = tree.new_with_children(holder, &[float]);
    let mut root = block();
    root.position = Position::Absolute;
    root.size.width = Dimension::length(100.0);
    let root = tree.new_with_children(root, &[empty, holder]);
    layout(&mut tree, root);

    assert_eq!(tree.layout(holder).location.y, 30.0);
    assert_eq!(tree.layout(float).location.y, 0.0);
    assert_eq!(tree.layout(root).size.height, 80.0);
}

#[test]
fn align_content_makes_a_child_avoid_floats() {
    // A child with `align-content` establishes an independent formatting context, so it sits
    // beside a float instead of overlapping it.
    let mut tree: Tree<()> = Tree::new();
    let mut float = block();
    float.float = Float::Left;
    float.size = Size { width: Dimension::length(30.0), height: Dimension::length(20.0) };
    let float = tree.new_leaf(float);
    let mut aligned = block();
    aligned.align_content = Some(cephal::style::AlignContent::CENTER);
    aligned.size.height = Dimension::length(10.0);
    let aligned = tree.new_leaf(aligned);
    let mut root = block();
    root.position = Position::Absolute;
    root.size.width = Dimension::length(100.0);
    let root = tree.new_with_children(root, &[float, aligned]);
    layout(&mut tree, root);

    assert_eq!(tree.layout(aligned).location.x, 30.0);
    assert_eq!(tree.layout(aligned).size.width, 70.0);
}

/// A flex row measuring a block item, then laying it out at the measured height.
fn measured_item(children: &[Style]) -> (Tree<()>, NodeId, Vec<NodeId>) {
    let mut tree: Tree<()> = Tree::new();
    let children: Vec<NodeId> = children.iter().map(|s| tree.new_leaf(s.clone())).collect();
    let item = tree.new_with_children(block(), &children);
    let mut root = Style::DEFAULT;
    root.display = Display::Flex;
    root.position = Position::Absolute;
    root.size.width = Dimension::length(100.0);
    let root = tree.new_with_children(root, &[item]);
    layout(&mut tree, root);
    (tree, item, children)
}

#[test]
fn measuring_a_block_keeps_margins_collapsing_through_sized_children() {
    // A sized child's top margin collapses with its own first child's, inside the flow-root item.
    let mut tree: Tree<()> = Tree::new();
    let mut grandchild = block();
    grandchild.margin = margins(20.0, Some(0.0), 0.0, Some(0.0));
    grandchild.size.height = Dimension::length(10.0);
    let grandchild = tree.new_leaf(grandchild);
    let mut child = block();
    child.size.height = Dimension::length(50.0);
    let child = tree.new_with_children(child, &[grandchild]);
    let item = tree.new_with_children(block(), &[child]);
    let mut root = Style::DEFAULT;
    root.display = Display::Flex;
    root.position = Position::Absolute;
    root.size.width = Dimension::length(100.0);
    let root = tree.new_with_children(root, &[item]);
    layout(&mut tree, root);
    assert_eq!(tree.layout(child).location.y, 20.0);
    assert_eq!(tree.layout(item).size.height, 70.0);

    // Margins collapse through a child whose used height is zero.
    let mut first = block();
    first.size.height = Dimension::length(10.0);
    first.margin = margins(0.0, Some(0.0), 10.0, Some(0.0));
    let mut empty = block();
    empty.size.height = Dimension::length(1.0);
    empty.max_size.height = LengthPercentageAuto::length(0.0);
    empty.margin = margins(10.0, Some(0.0), 10.0, Some(0.0));
    let mut last = block();
    last.size.height = Dimension::length(10.0);
    last.margin = margins(10.0, Some(0.0), 0.0, Some(0.0));
    let (tree, item, children) = measured_item(&[first, empty, last]);
    assert_eq!(tree.layout(children[2]).location.y, 20.0);
    assert_eq!(tree.layout(item).size.height, 30.0);
}

/// A block beside a float is asked under a key that counts the floats placed before it, not their
/// geometry; after a float resizes, the next pass must not serve the answer it got beside the old one.
#[test]
fn a_resized_float_reaches_the_blocks_beside_it() {
    #[derive(Clone, Copy)]
    struct Text(u32);
    fn measure(t: &mut Text, input: cephal::tree::MeasureInput, _: &Style) -> cephal::tree::MeasureOutput {
        let width = input.known_dimensions.width.unwrap_or(match input.available_space.width {
            AvailableSpace::MinContent => 10.0,
            AvailableSpace::MaxContent => t.0 as f32 * 10.0,
            AvailableSpace::Definite(w) => w.min(t.0 as f32 * 10.0).max(10.0),
        });
        let lines = (t.0 as f32 / (width / 10.0).floor().max(1.0)).ceil();
        cephal::tree::MeasureOutput::from_size(Size { width, height: input.known_dimensions.height.unwrap_or(lines * 10.0) })
    }
    let mut tree: Tree<Text> = Tree::new();
    let mut root = block();
    root.display = Display::FlowRoot;
    root.size.width = Dimension::length(200.0);
    let root = tree.new_leaf(root);
    let mut float = block();
    float.float = Float::Left;
    float.size = Size { width: Dimension::length(50.0), height: Dimension::length(30.0) };
    let float = tree.new_leaf(float);
    // Same formatting context as the float: its own content avoids it.
    let wrapper = tree.new_leaf(block());
    let mut avoiding = block();
    avoiding.overflow = cephal::geometry::Point { x: cephal::style::Overflow::Hidden, y: cephal::style::Overflow::Hidden };
    let avoiding = tree.new_leaf_with_context(avoiding, Text(30));
    tree.append_child(root, float);
    tree.append_child(root, wrapper);
    tree.append_child(wrapper, avoiding);
    let space = Size { width: AvailableSpace::Definite(400.0), height: AvailableSpace::MaxContent };
    tree.disable_rounding();
    tree.compute_layout_with_measure(root, space, measure);
    for width in [120.0, 50.0, 90.0] {
        tree.patch_style(float, |s| s.size.width = Dimension::length(width));
        tree.compute_layout_with_measure(root, space, measure);
        let mut cold = tree.clone_cold();
        cold.disable_rounding();
        cold.compute_layout_with_measure(root, space, measure);
        for node in [root, float, wrapper, avoiding] {
            assert_eq!(format!("{:?}", tree.unrounded_layout(node)), format!("{:?}", cold.unrounded_layout(node)), "float width {width}");
        }
    }
}
