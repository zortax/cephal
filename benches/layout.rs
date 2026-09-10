//! cephal against taffy 0.14: cold layout, tree construction and incremental updates.

#[path = "../tests/common/mod.rs"]
mod common;

use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{Dimension, Display, FlexDirection, LengthPercentageAuto, Position, Style};
use cephal::tree::NodeId;
use cephal::{Parallel, Tree};

fn measure_shared(ctx: &Text, input: cephal::tree::MeasureInput, style: &Style) -> cephal::tree::MeasureOutput {
    let mut ctx = *ctx;
    measure(&mut ctx, input, style)
}
use common::*;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

fn fixed_style(display: Display) -> Style {
    let mut s = Style::DEFAULT;
    s.display = display;
    s.size = Size { width: Dimension::length(10.0), height: Dimension::length(10.0) };
    s.flex_grow = 1.0;
    s
}

/// Branching-factor `b` hierarchy with `levels` levels (taffy's "huge nested" shape).
fn build_nested(tree: &mut Tree<Text>, b: usize, levels: usize) -> NodeId {
    if levels == 0 {
        return tree.new_leaf(fixed_style(Display::Flex));
    }
    let node = tree.new_leaf(fixed_style(Display::Flex));
    for _ in 0..b {
        let c = build_nested(tree, b, levels - 1);
        tree.append_child(node, c);
    }
    node
}

fn build_flat(tree: &mut Tree<Text>, n: usize) -> NodeId {
    let root = tree.new_leaf(fixed_style(Display::Flex));
    for _ in 0..n {
        let c = tree.new_leaf(fixed_style(Display::Flex));
        tree.append_child(root, c);
    }
    root
}

fn build_random(tree: &mut Tree<Text>, seed: u64, target: usize) -> NodeId {
    let mut rng = Rng(seed | 1);
    let root = tree.new_leaf(random_style(&mut rng, true));
    let mut count = 1;
    while count < target {
        let sub = build_subtree(&mut rng, tree, 1);
        let mut n = Vec::new();
        all_nodes(tree, sub, &mut n);
        count += n.len();
        tree.append_child(root, sub);
    }
    root
}

fn build_grid(tree: &mut Tree<Text>, n: usize) -> NodeId {
    use cephal::style::{GridTemplateComponent, TrackSizingFunction};
    let mut s = Style::DEFAULT;
    s.display = Display::Grid;
    let g = s.grid_mut();
    g.template_columns = (0..n).map(|i| GridTemplateComponent::Single(if i % 3 == 0 { TrackSizingFunction::fr(1.0) } else { TrackSizingFunction::AUTO })).collect();
    g.template_rows = (0..n).map(|_| GridTemplateComponent::Single(TrackSizingFunction::AUTO)).collect();
    let root = tree.new_leaf(s);
    for i in 0..n * n {
        let mut cs = Style::DEFAULT;
        cs.size = Size { width: Dimension::length(10.0 + (i % 7) as f32), height: Dimension::length(10.0) };
        let c = tree.new_leaf(cs);
        tree.append_child(root, c);
    }
    root
}

const VIEWPORT: Size<AvailableSpace> = Size { width: AvailableSpace::Definite(1000.0), height: AvailableSpace::Definite(800.0) };

fn cold(c: &mut Criterion, name: &str, build: impl Fn(&mut Tree<Text>) -> NodeId) {
    let mut group = c.benchmark_group(format!("cold/{name}"));
    group.sample_size(20);
    group.bench_function("cephal", |b| {
        b.iter_batched(
            || {
                let mut tree = Tree::new();
                let root = build(&mut tree);
                (tree, root)
            },
            |(mut tree, root)| tree.compute_layout_with_measure(root, VIEWPORT, measure),
            BatchSize::LargeInput,
        )
    });
    let executor = Parallel::new(measure_shared);
    group.bench_function("cephal_parallel", |b| {
        b.iter_batched(
            || {
                let mut tree = Tree::new();
                let root = build(&mut tree);
                (tree, root)
            },
            |(mut tree, root)| tree.compute_layout_parallel(root, VIEWPORT, &executor),
            BatchSize::LargeInput,
        )
    });
    group.bench_function("taffy", |b| {
        b.iter_batched(
            || {
                let mut tree = Tree::new();
                let root = build(&mut tree);
                let (t, troot, _) = mirror_in_taffy(&tree, root);
                (t, troot)
            },
            |(mut t, troot)| t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap(),
            BatchSize::LargeInput,
        )
    });
    group.finish();
}

fn build_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("build/nested_10k");
    group.sample_size(20);
    group.bench_function("cephal", |b| {
        b.iter(|| {
            let mut tree: Tree<Text> = Tree::new();
            build_nested(&mut tree, 10, 4)
        })
    });
    group.bench_function("taffy", |b| {
        b.iter(|| {
            let mut t: taffy::TaffyTree<Text> = taffy::TaffyTree::new();
            fn go(t: &mut taffy::TaffyTree<Text>, b: usize, levels: usize, style: &taffy::Style) -> taffy::NodeId {
                if levels == 0 {
                    return t.new_leaf(style.clone()).unwrap();
                }
                let node = t.new_leaf(style.clone()).unwrap();
                for _ in 0..b {
                    let c = go(t, b, levels - 1, style);
                    t.add_child(node, c).unwrap();
                }
                node
            }
            let style = cephal::compat::taffy::to_taffy(&fixed_style(Display::Flex), &|_| String::new());
            go(&mut t, 10, 4, &style)
        })
    });
    group.finish();
}

/// Incremental scenarios on a ~10k node random tree.
fn incremental(c: &mut Criterion) {
    let mut tree: Tree<Text> = Tree::new();
    let root = build_random(&mut tree, 7, 10_000);
    let mut nodes = Vec::new();
    all_nodes(&tree, root, &mut nodes);
    // A leaf deep in the tree, and a mid-level container.
    let leaf = *nodes.iter().filter(|n| tree.child_count(**n) == 0 && tree.context(**n).is_some()).nth(nodes.len() / 3).unwrap();
    let container = *nodes.iter().find(|n| tree.child_count(**n) > 2 && tree.depth(**n) == 2).unwrap();
    let abs = {
        let mut s = Style::DEFAULT;
        s.position = Position::Absolute;
        s.size = Size { width: Dimension::length(50.0), height: Dimension::length(50.0) };
        s.inset.left = LengthPercentageAuto::length(10.0);
        s.inset.top = LengthPercentageAuto::length(10.0);
        let n = tree.new_leaf(s);
        tree.append_child(root, n);
        n
    };
    tree.compute_layout_with_measure(root, VIEWPORT, measure);
    let (mut t, troot, map) = mirror_in_taffy(&tree, root);
    let tmap = |n: NodeId| map.iter().find(|(a, _)| *a == n).unwrap().1;
    t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap();

    let mut group = c.benchmark_group("incremental/random_10k");
    group.sample_size(50);

    let mut flip = false;
    group.bench_function("hover_leaf_width/cephal", |b| {
        b.iter(|| {
            flip = !flip;
            let w = if flip { 37.0 } else { 41.0 };
            tree.patch_style(leaf, |s| s.size.width = Dimension::length(w));
            tree.compute_layout_with_measure(root, VIEWPORT, measure);
        })
    });
    let tleaf = tmap(leaf);
    group.bench_function("hover_leaf_width/taffy", |b| {
        b.iter(|| {
            flip = !flip;
            let w = if flip { 37.0 } else { 41.0 };
            let mut s = t.style(tleaf).unwrap().clone();
            s.size.width = taffy::Dimension::length(w);
            t.set_style(tleaf, s).unwrap();
            t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap();
        })
    });

    group.bench_function("typing_leaf_text/cephal", |b| {
        b.iter(|| {
            flip = !flip;
            tree.set_context(leaf, Some(Text(if flip { 5 } else { 6 })));
            tree.compute_layout_with_measure(root, VIEWPORT, measure);
        })
    });
    group.bench_function("typing_leaf_text/taffy", |b| {
        b.iter(|| {
            flip = !flip;
            t.set_node_context(tleaf, Some(Text(if flip { 5 } else { 6 }))).unwrap();
            t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap();
        })
    });

    group.bench_function("animate_absolute/cephal", |b| {
        b.iter(|| {
            flip = !flip;
            tree.patch_style(abs, |s| s.inset.left = LengthPercentageAuto::length(if flip { 10.0 } else { 20.0 }));
            tree.compute_layout_with_measure(root, VIEWPORT, measure);
        })
    });
    let tabs = tmap(abs);
    group.bench_function("animate_absolute/taffy", |b| {
        b.iter(|| {
            flip = !flip;
            let mut s = t.style(tabs).unwrap().clone();
            s.inset.left = taffy::LengthPercentageAuto::length(if flip { 10.0 } else { 20.0 });
            t.set_style(tabs, s).unwrap();
            t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap();
        })
    });

    group.bench_function("toggle_display/cephal", |b| {
        b.iter(|| {
            flip = !flip;
            tree.patch_style(container, |s| s.display = if flip { Display::None } else { Display::Flex });
            tree.compute_layout_with_measure(root, VIEWPORT, measure);
        })
    });
    let tcontainer = tmap(container);
    group.bench_function("toggle_display/taffy", |b| {
        b.iter(|| {
            flip = !flip;
            let mut s = t.style(tcontainer).unwrap().clone();
            s.display = if flip { taffy::Display::None } else { taffy::Display::Flex };
            t.set_style(tcontainer, s).unwrap();
            t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap();
        })
    });

    group.bench_function("resize/cephal", |b| {
        b.iter(|| {
            flip = !flip;
            let vp = Size { width: AvailableSpace::Definite(if flip { 1000.0 } else { 1100.0 }), height: VIEWPORT.height };
            tree.compute_layout_with_measure(root, vp, measure);
        })
    });
    let executor = Parallel::new(measure_shared);
    group.bench_function("resize/cephal_parallel", |b| {
        b.iter(|| {
            flip = !flip;
            let vp = Size { width: AvailableSpace::Definite(if flip { 1000.0 } else { 1100.0 }), height: VIEWPORT.height };
            tree.compute_layout_parallel(root, vp, &executor);
        })
    });
    group.bench_function("resize/taffy", |b| {
        b.iter(|| {
            flip = !flip;
            let vp = Size { width: AvailableSpace::Definite(if flip { 1000.0 } else { 1100.0 }), height: VIEWPORT.height };
            t.compute_layout_with_measure(troot, to_taffy_space(vp), taffy_measure).unwrap();
        })
    });

    group.bench_function("idle_frame/cephal", |b| b.iter(|| tree.compute_layout_with_measure(root, VIEWPORT, measure)));
    group.bench_function("idle_frame/taffy", |b| {
        b.iter(|| t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap())
    });
    group.finish();
}

fn cold_benches(c: &mut Criterion) {
    cold(c, "nested_1k", |t| build_nested(t, 10, 3));
    cold(c, "nested_10k", |t| build_nested(t, 10, 4));
    cold(c, "nested_100k", |t| build_nested(t, 10, 5));
    cold(c, "flat_1k", |t| build_flat(t, 1_000));
    cold(c, "flat_10k", |t| build_flat(t, 10_000));
    cold(c, "random_10k", |t| build_random(t, 3, 10_000));
    cold(c, "grid_31x31", |t| build_grid(t, 31));
    cold(c, "grid_100x100", |t| build_grid(t, 100));
    let _ = FlexDirection::Row;
}

criterion_group!(benches, cold_benches, build_bench, incremental);
criterion_main!(benches);
