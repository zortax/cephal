//! Incremental relayout must equal a cold layout of the same tree, step for step.

mod common;

use cephal::geometry::{AvailableSpace, Line, Rect, Size};
use cephal::style::{AlignItems, Dimension, Display, FlexWrap, GridPlacement, LengthPercentageAuto, Position};
use cephal::tree::{Layout, NodeId};
use cephal::{Parallel, Tree};
use common::*;

/// The shared-state form of `measure`.
fn measure_shared(ctx: &Text, input: cephal::tree::MeasureInput, style: &cephal::Style) -> cephal::tree::MeasureOutput {
    let mut ctx = *ctx;
    measure(&mut ctx, input, style)
}

/// An executor that distributes every batch it can, on `threads` threads.
fn executor(threads: usize) -> Parallel<fn(&Text, cephal::tree::MeasureInput, &cephal::Style) -> cephal::tree::MeasureOutput> {
    let mut e = Parallel::with_threads(measure_shared as fn(&Text, _, &_) -> _, threads);
    e.min_requests(1).min_work(0);
    e
}

fn layouts(tree: &Tree<Text>, root: NodeId) -> Vec<(NodeId, Layout, Layout)> {
    let mut nodes = Vec::new();
    all_nodes(tree, root, &mut nodes);
    nodes.into_iter().map(|n| (n, *tree.unrounded_layout(n), *tree.layout(n))).collect()
}

fn assert_same(step: &str, incremental: &Tree<Text>, cold: &Tree<Text>, root: NodeId) {
    let a = layouts(incremental, root);
    let b = layouts(cold, root);
    assert_eq!(a.len(), b.len(), "{step}: node count");
    // Bit for bit: `{:?}` tells `-0.0` from `0.0`, which `==` does not.
    let bits = |l: &Layout| format!("{l:?}");
    for ((n, ua, ra), (_, ub, rb)) in a.iter().zip(b.iter()) {
        assert!(bits(ua) == bits(ub), "{step}: unrounded layout of {n:?} differs\n incremental: {ua:?}\n cold:        {ub:?}");
        assert!(bits(ra) == bits(rb), "{step}: rounded layout of {n:?} differs\n incremental: {ra:?}\n cold:        {rb:?}");
    }
}

/// Edit kinds of the scripts that once failed (see [`regressions`]): their seeds replay only with
/// the edits that existed then.
const LEGACY_KINDS: u64 = 18;
/// Every edit kind, including the properties only a parent reads.
const ALL_KINDS: u64 = 25;

/// One random edit of the first `kinds` kinds; returns a description.
fn edit(rng: &mut Rng, tree: &mut Tree<Text>, root: NodeId, kinds: u64) -> String {
    let mut nodes = Vec::new();
    all_nodes(tree, root, &mut nodes);
    let target = rng.pick(&nodes);
    match rng.below(kinds) {
        0 | 1 => {
            let container = tree.child_count(target) > 0;
            tree.set_style(target, random_style(rng, container));
            format!("set_style {target:?}")
        }
        2 => {
            tree.patch_style(target, |s| s.size.width = dimension(rng));
            format!("patch width {target:?}")
        }
        3 => {
            if tree.context(target).is_some() {
                tree.set_context(target, Some(Text(rng.below(12) as u32 + 1)));
                format!("set_context {target:?}")
            } else {
                tree.patch_style(target, |s| s.margin.top = lpa(rng));
                format!("patch margin {target:?}")
            }
        }
        4 => {
            let parent = if tree.child_count(target) > 0 || target == root { target } else { tree.parent(target).unwrap() };
            let child = build_subtree(rng, tree, 2);
            let index = rng.below(tree.child_count(parent) as u64 + 1) as usize;
            tree.insert_child_at(parent, index, child);
            format!("insert under {parent:?} at {index}")
        }
        5 => {
            if target != root {
                tree.remove(target);
                format!("remove {target:?}")
            } else {
                tree.patch_style(target, |s| s.padding.left = lp(rng));
                "patch root padding".into()
            }
        }
        6 => {
            // Reparent a non-root node under another non-descendant container.
            if target == root {
                return "noop".into();
            }
            let mut sub = Vec::new();
            all_nodes(tree, target, &mut sub);
            let candidates: Vec<NodeId> =
                nodes.iter().copied().filter(|n| !sub.contains(n) && tree.child_count(*n) > 0).collect();
            if candidates.is_empty() {
                return "noop".into();
            }
            let new_parent = rng.pick(&candidates);
            tree.detach(target);
            tree.append_child(new_parent, target);
            format!("reparent {target:?} under {new_parent:?}")
        }
        7 if tree.style(target).position.is_out_of_flow() => {
            if rng.chance(1, 3) {
                // Absolute and fixed boxes are placed alike; only the container's overflow moves.
                let position = if tree.style(target).position == Position::Fixed { Position::Absolute } else { Position::Fixed };
                tree.patch_style(target, |s| s.position = position);
                return format!("patch position {target:?} to {position:?}");
            }
            tree.patch_style(target, |s| {
                s.inset.left = lpa(rng);
                s.inset.top = lpa(rng);
                s.size.width = dimension(rng);
            });
            format!("patch out-of-flow inset {target:?}")
        }
        // Transitions that change how questions are canonicalised: a leaf starting or stopping
        // to measure, a container's line mode, and clamps that tell min- from max-content apart.
        9 if tree.child_count(target) == 0 => {
            let context = if tree.context(target).is_some() { None } else { Some(Text(rng.below(12) as u32 + 1)) };
            let what = if context.is_some() { "add" } else { "remove" };
            tree.set_context(target, context);
            format!("{what} context {target:?}")
        }
        10 => {
            tree.patch_style(target, |s| s.flex_wrap = rng.pick(&[FlexWrap::NoWrap, FlexWrap::Wrap, FlexWrap::WrapReverse]));
            format!("patch flex-wrap {target:?}")
        }
        11 => {
            tree.patch_style(target, |s| {
                s.min_size.height = lpa(rng);
                s.max_size.width = lpa(rng);
                s.size.height = rng.pick(&[Dimension::AUTO, Dimension::MIN_CONTENT, Dimension::MAX_CONTENT, Dimension::length(20.0)]);
            });
            format!("patch clamps {target:?}")
        }
        // Out-of-flow edits; each falls back to `target` when the tree has no fitting node.
        12 => {
            // Containing blocks appearing and disappearing above out-of-flow boxes.
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| has_out_of_flow_below(tree, n)), target);
            let position = match tree.style(n).position {
                Position::Static => rng.pick(&[Position::Relative, Position::Sticky]),
                Position::Relative | Position::Sticky => Position::Static,
                other => other,
            };
            tree.patch_style(n, |s| s.position = position);
            format!("patch position {n:?} to {position:?}")
        }
        13 => {
            // Moves the static position of the out-of-flow box following it.
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| next_is_out_of_flow(tree, n)), target);
            tree.patch_style(n, |s| {
                s.size.width = dimension(rng);
                s.size.height = dimension(rng);
                s.margin.top = lpa(rng);
            });
            format!("resize static-position predecessor {n:?}")
        }
        14 => {
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| tree.style(n).position.is_out_of_flow()), target);
            tree.patch_style(n, |s| s.inset = cephal::geometry::Rect { left: lpa(rng), right: lpa(rng), top: lpa(rng), bottom: lpa(rng) });
            format!("patch insets {n:?}")
        }
        15 => {
            // An out-of-flow box with out-of-flow descendants: absolute in absolute, fixed in absolute.
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| n != root && has_out_of_flow_below(tree, n)), target);
            let position = rng.pick(&[Position::Absolute, Position::Fixed, Position::Static]);
            tree.patch_style(n, |s| {
                s.position = position;
                s.inset.left = lpa(rng);
                s.inset.bottom = lpa(rng);
            });
            format!("patch container {n:?} to {position:?}")
        }
        16 => {
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| tree.style(n).position.is_out_of_flow()), target);
            tree.patch_style(n, |s| {
                s.align_self = rng.pick(&[None, Some(AlignItems::CENTER), Some(AlignItems::END), Some(AlignItems::SAFE_END), Some(AlignItems::UNSAFE_CENTER)]);
                s.justify_self = rng.pick(&[None, Some(AlignItems::STRETCH), Some(AlignItems::END), Some(AlignItems::SAFE_CENTER)]);
            });
            format!("patch out-of-flow self-alignment {n:?}")
        }
        // Properties only the parent (or the containing block) reads, which leave the node's own
        // answers cached; mostly on items of flex and grid containers, which read most of them.
        18 => {
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| is_flex_or_grid_item(tree, n)), target);
            let value = rng.pick(&[
                None,
                Some(AlignItems::STRETCH),
                Some(AlignItems::CENTER),
                Some(AlignItems::END),
                Some(AlignItems::FLEX_START),
                Some(AlignItems::BASELINE),
                Some(AlignItems::SAFE_END),
            ]);
            tree.patch_style(n, |s| s.align_self = value);
            format!("patch align-self {n:?} to {value:?}")
        }
        19 => {
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| n != root), target);
            let value =
                rng.pick(&[None, Some(AlignItems::STRETCH), Some(AlignItems::CENTER), Some(AlignItems::END), Some(AlignItems::START), Some(AlignItems::SAFE_CENTER)]);
            tree.patch_style(n, |s| s.justify_self = value);
            format!("patch justify-self {n:?} to {value:?}")
        }
        20 => {
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| is_flex_or_grid_item(tree, n)), target);
            let (grow, shrink) = (rng.pick(&[0.0, 0.5, 1.0, 2.0]), rng.pick(&[0.0, 1.0, 3.0]));
            let basis = if rng.chance(1, 2) { tree.style(n).flex_basis } else { dimension(rng) };
            tree.patch_style(n, |s| {
                s.flex_grow = grow;
                s.flex_shrink = shrink;
                s.flex_basis = basis;
            });
            format!("patch flex {n:?} to {grow} {shrink} {basis:?}")
        }
        21 => {
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| is_flex_or_grid_item(tree, n)), target);
            let place = |rng: &mut Rng| match rng.below(4) {
                0 => GridPlacement::Auto,
                1 => GridPlacement::Line(rng.below(3) as i16 + 1),
                2 => GridPlacement::Line(-1),
                _ => GridPlacement::Span(rng.below(2) as u16 + 1),
            };
            let row = Line { start: place(rng), end: place(rng) };
            let column = Line { start: place(rng), end: place(rng) };
            tree.patch_style(n, |s| {
                s.grid_row = row;
                s.grid_column = column;
            });
            format!("patch grid placement {n:?}")
        }
        22 => {
            // Relative boxes' offsets; static boxes ignore them.
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| tree.style(n).position == Position::Relative), target);
            let inset = if rng.chance(1, 4) {
                Rect { left: lpa(rng), right: lpa(rng), top: lpa(rng), bottom: lpa(rng) }
            } else {
                // Lengths only: the query meta stays, so the node keeps its answers.
                let len = |rng: &mut Rng| if rng.chance(1, 3) { LengthPercentageAuto::AUTO } else { LengthPercentageAuto::length(rng.below(30) as f32) };
                Rect { left: len(rng), right: len(rng), top: len(rng), bottom: len(rng) }
            };
            tree.patch_style(n, |s| s.inset = inset);
            format!("patch relative insets {n:?}")
        }
        23 => {
            // Leaves and containers turning relative (or static again), so 22 finds them.
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| n != root && !tree.style(n).position.is_out_of_flow()), target);
            let position = if tree.style(n).position == Position::Relative { Position::Static } else { Position::Relative };
            let left = LengthPercentageAuto::length(rng.below(20) as f32);
            tree.patch_style(n, |s| {
                s.position = position;
                s.inset.left = left;
            });
            format!("patch position {n:?} to {position:?}")
        }
        24 => {
            let n = pick_or(rng, nodes.iter().copied().filter(|&n| n != root), target);
            let align = rng.pick(&[None, Some(AlignItems::CENTER), Some(AlignItems::STRETCH)]);
            let justify = rng.pick(&[None, Some(AlignItems::END)]);
            let grow = rng.pick(&[0.0, 1.0]);
            let top = rng.pick(&[LengthPercentageAuto::AUTO, LengthPercentageAuto::length(7.0)]);
            tree.patch_style(n, |s| {
                s.align_self = align;
                s.justify_self = justify;
                s.flex_grow = grow;
                s.inset.top = top;
            });
            format!("patch placement {n:?}")
        }
        _ => {
            tree.patch_style(target, |s| s.flex_grow = rng.pick(&[0.0, 1.0, 3.0]));
            format!("patch flex-grow {target:?}")
        }
    }
}

/// Whether `node`'s parent is a flex or grid container.
fn is_flex_or_grid_item(tree: &Tree<Text>, node: NodeId) -> bool {
    tree.parent(node).is_some_and(|p| matches!(tree.style(p).display, Display::Flex | Display::Grid))
}

/// A random node of `candidates`, or `fallback` when there is none.
fn pick_or(rng: &mut Rng, candidates: impl Iterator<Item = NodeId>, fallback: NodeId) -> NodeId {
    let candidates: Vec<NodeId> = candidates.collect();
    if candidates.is_empty() { fallback } else { rng.pick(&candidates) }
}

fn has_out_of_flow_below(tree: &Tree<Text>, node: NodeId) -> bool {
    tree.children(node).any(|c| tree.style(c).position.is_out_of_flow() || has_out_of_flow_below(tree, c))
}

/// Whether `node`'s next sibling is out of flow.
fn next_is_out_of_flow(tree: &Tree<Text>, node: NodeId) -> bool {
    let Some(parent) = tree.parent(node) else { return false };
    let mut siblings = tree.children(parent).skip_while(|&c| c != node).skip(1);
    siblings.next().is_some_and(|n| tree.style(n).position.is_out_of_flow())
}

fn run_script(seed: u64, steps: usize, kinds: u64) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut tree: Tree<Text> = Tree::new();
    let root = build_subtree(&mut rng, &mut tree, 0);
    let mut viewport = Size { width: AvailableSpace::Definite(400.0), height: AvailableSpace::Definite(300.0) };
    tree.compute_layout_with_measure(root, viewport, measure);
    let mut cold = tree.clone_cold();
    cold.compute_layout_with_measure(root, viewport, measure);
    assert_same(&format!("seed {seed} initial"), &tree, &cold, root);
    // The same tree driven by the parallel executor, cold and incrementally, and by the same
    // executor on one thread as the reference for cache contents.
    let threads = std::env::var("CEPHAL_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(if seed.is_multiple_of(2) { 2 } else { 8 });
    let single = executor(1);
    let executor = executor(threads);
    let mut par = tree.clone_cold();
    par.compute_layout_parallel(root, viewport, &executor);
    assert_same(&format!("seed {seed} initial parallel"), &par, &cold, root);
    let mut one = tree.clone_cold();
    one.compute_layout_parallel(root, viewport, &single);
    for step in 0..steps {
        let before = layouts(&tree, root);
        let what = if rng.chance(1, 10) {
            viewport = Size {
                width: AvailableSpace::Definite(rng.below(600) as f32 + 50.0),
                height: rng.pick(&[AvailableSpace::MaxContent, AvailableSpace::Definite(300.0)]),
            };
            "resize".to_owned()
        } else {
            // Identical rng state and slots: `par` and `one` receive the same edit.
            let mut replay = Rng(rng.0);
            edit(&mut replay, &mut par, root, kinds);
            let mut replay = Rng(rng.0);
            edit(&mut replay, &mut one, root, kinds);
            edit(&mut rng, &mut tree, root, kinds)
        };
        tree.compute_layout_with_measure(root, viewport, measure);
        let mut cold = tree.clone_cold();
        cold.compute_layout_with_measure(root, viewport, measure);
        let label = format!("seed {seed} step {step} ({what})");
        assert_same(&label, &tree, &cold, root);
        par.compute_layout_parallel(root, viewport, &executor);
        one.compute_layout_parallel(root, viewport, &single);
        assert_same(&format!("{label} one-thread incremental"), &one, &cold, root);
        // Threads must not change which answers are cached: same hits, same evictions.
        let mut nodes = Vec::new();
        all_nodes(&tree, root, &mut nodes);
        for n in nodes {
            let (a, b) = (one.cache_entries(n), par.cache_entries(n));
            assert!(a == b, "{label}: cache of {n:?} differs\n one thread: {a:#?}\n parallel: {b:#?}");
        }
        assert_same(&format!("{label} parallel incremental"), &par, &cold, root);
        let mut par_cold = tree.clone_cold();
        par_cold.compute_layout_parallel(root, viewport, &executor);
        assert_same(&format!("{label} parallel cold"), &par_cold, &cold, root);
        assert_eq!(par.changed().len(), tree.changed().len(), "{label}: parallel changed()");
        // `changed()` must be exactly the set of surviving nodes whose unrounded layout moved.
        let after = layouts(&tree, root);
        // A node inserted this step starts from the zero layout.
        let mut expected: Vec<NodeId> = after
            .iter()
            .filter(|(n, u, _)| before.iter().find(|(m, _, _)| m == n).map_or(*u != Layout::ZERO, |(_, bu, _)| bu != u))
            .map(|(n, _, _)| *n)
            .collect();
        let mut got: Vec<NodeId> = tree.changed().iter().copied().filter(|n| after.iter().any(|(m, _, _)| m == n)).collect();
        expected.sort();
        got.sort();
        got.dedup();
        assert_eq!(got, expected, "{label}: changed() mismatch");
    }
}

/// Scripts that once failed: a reparented subtree keeping its own layout under a parent at another
/// fractional offset was not rounded again.
#[test]
fn regressions() {
    run_script(368, 40, LEGACY_KINDS);
}

#[test]
fn incremental_matches_cold_layout() {
    let seeds: u64 = std::env::var("CEPHAL_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    let steps: usize = std::env::var("CEPHAL_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(25);
    let start: u64 = std::env::var("CEPHAL_SEED_START").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    for seed in start..=seeds {
        run_script(seed, steps, ALL_KINDS);
    }
}

/// Edits that often change answers only in their overflow (relative insets, text outgrowing fixed
/// boxes), under containers of both directions, clipped and contained ones, flow roots and floats,
/// which the shared random styles leave out. Floated text is asked speculatively for its size
/// (`speculate`), so its parent depends on answers its cache does not record. Floats sit beside
/// block containers too, whose answers depend on the floats' geometry.
#[test]
fn overflow_edits_match_cold_layout() {
    use cephal::style::{Contain, Direction, Float, LengthPercentage, Overflow};
    let seeds: u64 = std::env::var("CEPHAL_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    let steps: usize = std::env::var("CEPHAL_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(25);
    let start: u64 = std::env::var("CEPHAL_SEED_START").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    for seed in start..=seeds {
        let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
        let mut tree: Tree<Text> = Tree::new();
        let root = build_subtree(&mut rng, &mut tree, 0);
        let mut nodes = Vec::new();
        all_nodes(&tree, root, &mut nodes);
        // Parents come first, so a leaf sees whether its parent became a flow root.
        for &n in &nodes {
            let container = tree.child_count(n) > 0;
            let in_flow_root = tree.parent(n).is_some_and(|p| tree.style(p).display == Display::FlowRoot);
            let (rtl, float, clip, contain, border, flow_root, relative) =
                (rng.chance(1, 3), rng.below(3), rng.chance(1, 10), rng.below(12), rng.chance(1, 8), rng.chance(1, 2), rng.chance(1, 3));
            tree.patch_style(n, |s| {
                if rtl {
                    s.direction = Direction::Rtl;
                }
                if in_flow_root && !s.position.is_out_of_flow() {
                    s.float = match float {
                        0 => Float::Left,
                        1 => Float::Right,
                        _ => Float::None,
                    };
                }
                if clip {
                    s.overflow.x = Overflow::Clip;
                }
                s.contain = match contain {
                    0 => Contain::PAINT,
                    1 => Contain::LAYOUT,
                    _ => Contain::NONE,
                };
                if border {
                    s.border = Rect::splat(LengthPercentage::percent(0.02));
                }
                if flow_root && container && s.display == Display::Block {
                    s.display = Display::FlowRoot;
                }
                if relative && !s.position.is_out_of_flow() {
                    s.position = Position::Relative;
                }
            });
        }
        let viewport = Size { width: AvailableSpace::Definite(400.0), height: AvailableSpace::Definite(300.0) };
        tree.compute_layout_with_measure(root, viewport, measure);
        for step in 0..steps {
            let mut nodes = Vec::new();
            all_nodes(&tree, root, &mut nodes);
            let target = rng.pick(&nodes);
            let what = if rng.below(3) < 2 {
                let n = pick_or(&mut rng, nodes.iter().copied().filter(|&n| n != root && tree.style(n).position == Position::Relative), target);
                let (left, top) = (rng.below(40) as f32 - 15.0, rng.below(40) as f32 - 15.0);
                tree.patch_style(n, |s| {
                    s.inset.left = LengthPercentageAuto::length(left);
                    s.inset.top = LengthPercentageAuto::length(top);
                });
                format!("move {n:?} by {left} {top}")
            } else {
                let n = pick_or(&mut rng, nodes.iter().copied().filter(|&n| tree.context(n).is_some()), target);
                let text = rng.below(30) as u32 + 1;
                tree.set_context(n, Some(Text(text)));
                format!("text {n:?} to {text}")
            };
            tree.compute_layout_with_measure(root, viewport, measure);
            let mut cold = tree.clone_cold();
            cold.compute_layout_with_measure(root, viewport, measure);
            assert_same(&format!("seed {seed} step {step} ({what})"), &tree, &cold, root);
        }
    }
}

#[test]
fn wide_containers_are_distributed() {
    let mut rng = Rng(11);
    let mut tree: Tree<Text> = Tree::new();
    let root = build_subtree(&mut rng, &mut tree, 0);
    let executor = executor(4);
    let viewport = Size { width: AvailableSpace::Definite(400.0), height: AvailableSpace::MaxContent };
    tree.compute_layout_parallel(root, viewport, &executor);
    assert!(executor.distributed() > 0);
    let before = executor.distributed();
    tree.compute_layout_parallel(root, viewport, &executor);
    assert_eq!(executor.distributed(), before, "an unchanged frame asks nothing");
}

#[test]
fn unchanged_frames_recompute_nothing() {
    let mut rng = Rng(42);
    let mut tree: Tree<Text> = Tree::new();
    let root = build_subtree(&mut rng, &mut tree, 0);
    let viewport = Size { width: AvailableSpace::Definite(400.0), height: AvailableSpace::MaxContent };
    tree.compute_layout_with_measure(root, viewport, measure);
    tree.compute_layout_with_measure(root, viewport, measure);
    assert!(tree.changed().is_empty());
    // Re-setting an identical style leaves the tree clean.
    let style = tree.style(root).clone();
    tree.set_style(root, style);
    assert!(!tree.is_dirty(root));
}

/// `CEPHAL_SEED=59 CEPHAL_STEPS=12 cargo test --test incremental debug_seed -- --ignored --nocapture`
#[test]
#[ignore]
fn debug_seed() {
    let seed: u64 = std::env::var("CEPHAL_SEED").unwrap().parse().unwrap();
    let steps: usize = std::env::var("CEPHAL_STEPS").unwrap().parse().unwrap();
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut tree: Tree<Text> = Tree::new();
    let root = build_subtree(&mut rng, &mut tree, 0);
    let mut viewport = Size { width: AvailableSpace::Definite(400.0), height: AvailableSpace::Definite(300.0) };
    tree.compute_layout_with_measure(root, viewport, measure);
    for step in 0..=steps {
        let what = if rng.chance(1, 10) {
            viewport = Size {
                width: AvailableSpace::Definite(rng.below(600) as f32 + 50.0),
                height: rng.pick(&[AvailableSpace::MaxContent, AvailableSpace::Definite(300.0)]),
            };
            "resize".to_owned()
        } else {
            edit(&mut rng, &mut tree, root, ALL_KINDS)
        };
        println!("step {step}: {what}");
        if step == steps {
            let mut nodes = Vec::new();
            all_nodes(&tree, root, &mut nodes);
            for n in &nodes {
                println!("  before: {n:?} dirty={} parent={:?} style.display={:?} pos={:?} entries={}", tree.is_dirty(*n), tree.parent(*n), tree.style(*n).display, tree.style(*n).position, tree.cache_entries(*n).len());
            }
        }
        tree.compute_layout_with_measure(root, viewport, measure);
        if step == steps {
            let mut cold = tree.clone_cold();
            cold.compute_layout_with_measure(root, viewport, measure);
            let mut nodes = Vec::new();
            all_nodes(&tree, root, &mut nodes);
            for n in &nodes {
                let (a, b) = (tree.unrounded_layout(*n), cold.unrounded_layout(*n));
                if a != b {
                    println!("  DIFF {n:?}\n    inc:  {a:?}\n    cold: {b:?}");
                    for (i, o) in tree.cache_entries(*n) {
                        println!("    inc entry: {i:?} -> {o:?}");
                    }
                    for (i, o) in cold.cache_entries(*n) {
                        println!("    cold entry: {i:?} -> {o:?}");
                    }
                }
            }
            if let Ok(dump) = std::env::var("CEPHAL_DUMP") {
                for raw in dump.split(',') {
                    let n = nodes.iter().copied().find(|n| n.raw().to_string() == raw).unwrap();
                    println!("  DUMP {n:?} style={:?}", tree.style(n));
                    for (i, o) in tree.cache_entries(n) {
                        println!("    inc:  {:?} kd={:?} avail={:?} parent={:?} -> size={:?} baseline={:?}", i.run_mode, i.known_dimensions, i.available_space, i.parent_size, o.size, o.baselines.first);
                    }
                    for (i, o) in cold.cache_entries(n) {
                        println!("    cold: {:?} kd={:?} avail={:?} parent={:?} -> size={:?} baseline={:?}", i.run_mode, i.known_dimensions, i.available_space, i.parent_size, o.size, o.baselines.first);
                    }
                }
            }
            tree.compute_layout_with_measure(root, viewport, measure);
            for n in &nodes {
                let (a, b) = (tree.unrounded_layout(*n), cold.unrounded_layout(*n));
                if a != b {
                    println!("  STILL DIFF after idle frame {n:?}");
                }
            }
        }
    }
}
