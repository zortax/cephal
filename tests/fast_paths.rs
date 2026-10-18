//! Fast paths must leave every result as the general algorithms produce it: layouts, cached
//! answers and `changed()`, bit for bit, cold and incrementally, serially and in parallel.

mod common;

use cephal::compute::flex::set_fused_flex;
use cephal::geometry::{AvailableSpace, Rect, Size};
use cephal::style::{
    AlignContent, AlignItems, BoxSizing, Dimension, Direction, Display, FlexDirection, FlexWrap, LengthPercentage,
    LengthPercentageAuto, Overflow, Position, Style,
};
use cephal::tree::NodeId;
use cephal::{Parallel, Tree};
use common::*;

fn executor(threads: usize) -> Parallel<fn(&Text, cephal::tree::MeasureInput, &cephal::Style) -> cephal::tree::MeasureOutput> {
    fn shared(ctx: &Text, input: cephal::tree::MeasureInput, style: &cephal::Style) -> cephal::tree::MeasureOutput {
        let mut ctx = *ctx;
        measure(&mut ctx, input, style)
    }
    let mut e = Parallel::with_threads(shared as fn(&Text, _, &_) -> _, threads);
    e.min_requests(1).min_work(0);
    e
}

fn small_length(rng: &mut Rng) -> f32 {
    rng.pick(&[0.0, 1.0, 2.5, 5.0, 10.0, 0.3, 17.0])
}

fn length_or_auto(rng: &mut Rng, max: u64) -> Dimension {
    match rng.below(5) {
        0 | 1 => Dimension::AUTO,
        2 => Dimension::length(rng.below(max) as f32 + rng.pick(&[0.0, 0.5, 0.25])),
        3 => Dimension::length(rng.below(max / 4 + 1) as f32),
        _ => Dimension::ZERO,
    }
}

fn clamp(rng: &mut Rng) -> LengthPercentageAuto {
    match rng.below(4) {
        0 => LengthPercentageAuto::length(rng.below(80) as f32),
        1 => LengthPercentageAuto::ZERO,
        _ => LengthPercentageAuto::AUTO,
    }
}

/// A plain item: lengths only, so the fused flex path applies, unless `rare` adds something that
/// sends the container down the general path.
fn item_style(rng: &mut Rng, container: bool) -> Style {
    let mut s = Style::DEFAULT;
    s.display =
        if container { rng.pick(&[Display::Flex, Display::Flex, Display::Flex, Display::Block, Display::Grid]) } else { Display::Block };
    s.size = Size { width: length_or_auto(rng, 120), height: length_or_auto(rng, 120) };
    if rng.chance(1, 3) {
        s.min_size = Size { width: clamp(rng), height: clamp(rng) };
    }
    if rng.chance(1, 3) {
        s.max_size = Size { width: clamp(rng), height: clamp(rng) };
    }
    if rng.chance(1, 2) {
        let m = small_length(rng);
        s.margin = Rect::splat(LengthPercentageAuto::length(m));
        if rng.chance(1, 2) {
            s.margin.left = LengthPercentageAuto::length(small_length(rng));
            s.margin.bottom = LengthPercentageAuto::length(small_length(rng));
        }
    }
    if rng.chance(1, 2) {
        s.padding = Rect { left: lp_len(rng), right: lp_len(rng), top: lp_len(rng), bottom: lp_len(rng) };
    }
    if rng.chance(1, 4) {
        s.border = Rect::splat(LengthPercentage::length(rng.below(4) as f32));
    }
    if rng.chance(1, 4) {
        s.box_sizing = BoxSizing::ContentBox;
    }
    if rng.chance(1, 6) {
        s.overflow = rng.pick(&[
            cephal::geometry::Point { x: Overflow::Hidden, y: Overflow::Hidden },
            cephal::geometry::Point { x: Overflow::Visible, y: Overflow::Scroll },
            cephal::geometry::Point { x: Overflow::Scroll, y: Overflow::Clip },
        ]);
        s.scrollbar_width = rng.pick(&[0.0, 10.0]);
    }
    if rng.chance(1, 6) {
        s.inset = Rect { left: clamp(rng), right: clamp(rng), top: clamp(rng), bottom: clamp(rng) };
    }
    if rng.chance(1, 8) {
        s.direction = Direction::Rtl;
    }
    s.flex_grow = rng.pick(&[0.0, 0.0, 1.0, 2.0, 0.5, 0.25]);
    s.flex_shrink = rng.pick(&[1.0, 1.0, 0.0, 3.0, 0.5]);
    s.flex_basis = match rng.below(5) {
        0 => Dimension::length(rng.below(60) as f32),
        1 => Dimension::ZERO,
        _ => Dimension::AUTO,
    };
    s.align_self = rng.pick(&[
        None,
        None,
        None,
        Some(AlignItems::STRETCH),
        Some(AlignItems::CENTER),
        Some(AlignItems::END),
        Some(AlignItems::SAFE_CENTER),
        Some(AlignItems::FLEX_END),
        Some(AlignItems::START),
        Some(AlignItems::SELF_END),
        Some(AlignItems::SAFE_SELF_START),
    ]);
    // Anything below keeps the container on the general path.
    if rng.chance(1, 25) {
        match rng.below(9) {
            0 => s.size.width = Dimension::percent(0.5),
            1 => s.margin.left = LengthPercentageAuto::AUTO,
            2 => s.aspect_ratio = Some(2.0),
            3 => s.align_self = Some(AlignItems::BASELINE),
            4 => s.position = Position::Absolute,
            5 => s.display = Display::None,
            6 => s.size.height = Dimension::MIN_CONTENT,
            7 => s.flex_basis = Dimension::CONTENT,
            _ => s.flex_basis = Dimension::percent(0.3),
        }
    }
    s
}

fn lp_len(rng: &mut Rng) -> LengthPercentage {
    LengthPercentage::length(small_length(rng))
}

fn container_style(rng: &mut Rng) -> Style {
    let mut s = item_style(rng, true);
    s.display = if rng.chance(9, 10) { Display::Flex } else { rng.pick(&[Display::Block, Display::Grid]) };
    s.flex_direction = rng.pick(&[FlexDirection::Row, FlexDirection::Column, FlexDirection::RowReverse, FlexDirection::ColumnReverse]);
    s.flex_wrap = if rng.chance(1, 8) { rng.pick(&[FlexWrap::Wrap, FlexWrap::WrapReverse]) } else { FlexWrap::NoWrap };
    if rng.chance(1, 2) {
        s.gap = Size { width: lp_len(rng), height: lp_len(rng) };
    }
    if rng.chance(1, 10) {
        s.gap.width = LengthPercentage::percent(0.1);
    }
    s.justify_content = rng.pick(&[
        None,
        None,
        Some(AlignContent::CENTER),
        Some(AlignContent::SPACE_BETWEEN),
        Some(AlignContent::SPACE_AROUND),
        Some(AlignContent::SPACE_EVENLY),
        Some(AlignContent::END),
        Some(AlignContent::FLEX_END),
        Some(AlignContent::SAFE_CENTER),
    ]);
    s.align_items = rng.pick(&[None, None, Some(AlignItems::CENTER), Some(AlignItems::FLEX_END), Some(AlignItems::START), Some(AlignItems::END)]);
    s.align_content = rng.pick(&[None, None, Some(AlignContent::SPACE_AROUND), Some(AlignContent::CENTER), Some(AlignContent::END)]);
    if rng.chance(1, 6) {
        s.direction = Direction::Rtl;
    }
    if rng.chance(1, 8) {
        s.overflow = cephal::geometry::Point { x: Overflow::Scroll, y: Overflow::Scroll };
        s.scrollbar_width = 8.0;
    }
    if rng.chance(1, 10) {
        s.aspect_ratio = Some(1.5);
    }
    s
}

fn build(rng: &mut Rng, tree: &mut Tree<Text>, depth: u32) -> NodeId {
    let children = if depth >= 3 || (depth > 0 && rng.chance(1, 2)) { 0 } else { rng.below(7) + 1 };
    if children == 0 {
        let style = item_style(rng, false);
        let text = Text(rng.below(8) as u32 + 1);
        return if rng.chance(1, 3) { tree.new_leaf_with_context(style, text) } else { tree.new_leaf(style) };
    }
    let node = tree.new_leaf(container_style(rng));
    for _ in 0..children {
        let child = if rng.chance(1, 10) { build_subtree(rng, tree, 3) } else { build(rng, tree, depth + 1) };
        tree.append_child(node, child);
    }
    node
}

/// Every node's layouts and, with `caches`, its cache entries, as exact text.
fn snapshot(tree: &Tree<Text>, root: NodeId, caches: bool) -> Vec<String> {
    let mut nodes = Vec::new();
    all_nodes(tree, root, &mut nodes);
    nodes
        .into_iter()
        .map(|n| {
            let layouts = format!("{n:?} {:?} {:?}", tree.unrounded_layout(n), tree.layout(n));
            if caches { format!("{layouts} {:?}", tree.cache_entries(n)) } else { layouts }
        })
        .collect()
}

fn compare(label: &str, a: &[String], b: &[String]) {
    assert_eq!(a.len(), b.len(), "{label}: node count");
    for (x, y) in a.iter().zip(b) {
        assert!(x == y, "{label}: differs\n fused:   {x}\n generic: {y}");
    }
}

fn viewport(rng: &mut Rng) -> Size<AvailableSpace> {
    let space = |rng: &mut Rng| match rng.below(5) {
        0 => AvailableSpace::MaxContent,
        1 => AvailableSpace::MinContent,
        _ => AvailableSpace::Definite(rng.below(500) as f32 + 0.5 * rng.below(2) as f32),
    };
    Size { width: space(rng), height: space(rng) }
}

fn edit(rng: &mut Rng, tree: &mut Tree<Text>, root: NodeId) {
    let mut nodes = Vec::new();
    all_nodes(tree, root, &mut nodes);
    let target = rng.pick(&nodes);
    let container = tree.child_count(target) > 0;
    match rng.below(4) {
        0 => {
            let style = if container { container_style(rng) } else { item_style(rng, false) };
            tree.set_style(target, style);
        }
        1 => tree.patch_style(target, |s| s.size.width = length_or_auto(rng, 150)),
        2 => tree.patch_style(target, |s| s.flex_grow = rng.pick(&[0.0, 1.0, 3.0])),
        _ => {
            if container {
                let child = build(rng, tree, 2);
                tree.append_child(target, child);
            } else {
                tree.patch_style(target, |s| s.margin.top = LengthPercentageAuto::length(small_length(rng)));
            }
        }
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut fused: Tree<Text> = Tree::new();
    let root = build(&mut rng, &mut fused, 0);
    let mut generic = fused.clone_cold();
    let threads = if seed.is_multiple_of(2) { 3 } else { 1 };
    let exec = executor(threads);
    let mut parallel = fused.clone_cold();
    for step in 0..=steps {
        let view = viewport(&mut rng);
        if step > 0 {
            let state = rng.0;
            edit(&mut rng, &mut fused, root);
            edit(&mut Rng(state), &mut generic, root);
            edit(&mut Rng(state), &mut parallel, root);
        }
        let label = format!("seed {seed} step {step}");
        set_fused_flex(false);
        generic.compute_layout_with_measure(root, view, measure);
        set_fused_flex(true);
        fused.compute_layout_with_measure(root, view, measure);
        parallel.compute_layout_parallel(root, view, &exec);
        compare(&label, &snapshot(&fused, root, true), &snapshot(&generic, root, true));
        assert_eq!(fused.changed(), generic.changed(), "{label}: changed()");
        let mut cold = fused.clone_cold();
        cold.compute_layout_with_measure(root, view, measure);
        set_fused_flex(false);
        let mut generic_cold = fused.clone_cold();
        generic_cold.compute_layout_with_measure(root, view, measure);
        set_fused_flex(true);
        compare(&format!("{label} cold"), &snapshot(&cold, root, true), &snapshot(&generic_cold, root, true));
        assert_eq!(cold.changed(), generic_cold.changed(), "{label}: cold changed()");
        // Layouts only: caches of a distributing executor are compared with a one-thread one in
        // tests/incremental.rs.
        compare(&format!("{label} parallel"), &snapshot(&parallel, root, false), &snapshot(&generic, root, false));
    }
}

#[test]
fn fused_flex_matches_general_flex() {
    let seeds: u64 = std::env::var("CEPHAL_FAST_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(400);
    let steps: usize = std::env::var("CEPHAL_FAST_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    for seed in 1..=seeds {
        run(seed, steps);
    }
    set_fused_flex(true);
}
