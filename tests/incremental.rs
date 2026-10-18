//! Incremental relayout must equal a cold layout of the same tree, step for step.

mod common;

use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{Dimension, FlexWrap, Position};
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
    for ((n, ua, ra), (_, ub, rb)) in a.iter().zip(b.iter()) {
        assert!(ua == ub, "{step}: unrounded layout of {n:?} differs\n incremental: {ua:?}\n cold:        {ub:?}");
        assert!(ra == rb, "{step}: rounded layout of {n:?} differs\n incremental: {ra:?}\n cold:        {rb:?}");
    }
}

/// One random edit; returns a description.
fn edit(rng: &mut Rng, tree: &mut Tree<Text>, root: NodeId) -> String {
    let mut nodes = Vec::new();
    all_nodes(tree, root, &mut nodes);
    let target = rng.pick(&nodes);
    match rng.below(12) {
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
        _ => {
            tree.patch_style(target, |s| s.flex_grow = rng.pick(&[0.0, 1.0, 3.0]));
            format!("patch flex-grow {target:?}")
        }
    }
}

fn run_script(seed: u64, steps: usize) {
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
            edit(&mut replay, &mut par, root);
            let mut replay = Rng(rng.0);
            edit(&mut replay, &mut one, root);
            edit(&mut rng, &mut tree, root)
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
    run_script(368, 40);
}

#[test]
fn incremental_matches_cold_layout() {
    let seeds: u64 = std::env::var("CEPHAL_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    let steps: usize = std::env::var("CEPHAL_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(25);
    let start: u64 = std::env::var("CEPHAL_SEED_START").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    for seed in start..=seeds {
        run_script(seed, steps);
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
            edit(&mut rng, &mut tree, root)
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
