//! Random trees laid out by cephal and taffy 0.14 must agree.

mod common;

use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{Dimension, Display};
use cephal::Tree;
use common::*;

/// Lays out the tree of `seed` in both engines and lists every node where they disagree.
///
/// `relaxed` first removes what the engines treat differently by design: `aspect-ratio` on flex and
/// grid containers, and sizing keywords on the cross size of flex items.
fn run(seed: u64, relaxed: bool) -> Vec<String> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut tree: Tree<Text> = Tree::new();
    let root = build_subtree(&mut rng, &mut tree, 0);
    if relaxed {
        let mut nodes = Vec::new();
        all_nodes(&tree, root, &mut nodes);
        for node in nodes {
            if tree.children(node).next().is_none() {
                continue;
            }
            let display = tree.style(node).display;
            let is_row = tree.style(node).flex_direction.is_row();
            if matches!(display, Display::Flex | Display::Grid) {
                tree.patch_style(node, |style| style.aspect_ratio = None);
            }
            if display == Display::Flex {
                for child in tree.children(node).collect::<Vec<_>>() {
                    tree.patch_style(child, |style| {
                        let cross = if is_row { &mut style.size.height } else { &mut style.size.width };
                        if cross.is_sizing_keyword() {
                            *cross = Dimension::AUTO;
                        }
                    });
                }
            }
        }
    }
    tree.disable_rounding();
    let viewport = Size {
        width: AvailableSpace::Definite(rng.below(600) as f32 + 50.0),
        height: rng.pick(&[AvailableSpace::MaxContent, AvailableSpace::Definite(300.0)]),
    };
    tree.compute_layout_with_measure(root, viewport, measure);
    let (mut taffy_tree, taffy_root, map) = mirror_in_taffy(&tree, root);
    taffy_tree.disable_rounding();
    taffy_tree.compute_layout_with_measure(taffy_root, to_taffy_space(viewport), taffy_measure).unwrap();
    let mut errors = Vec::new();
    for (ours, theirs) in map {
        let a = tree.unrounded_layout(ours);
        let b = taffy_tree.unrounded_layout(theirs);
        let near = |x: f32, y: f32| (x - y).abs() <= 0.01 * (1.0 + x.abs().max(y.abs()));
        if !(near(a.location.x, b.location.x) && near(a.location.y, b.location.y) && near(a.size.width, b.size.width) && near(a.size.height, b.size.height)) {
            errors.push(format!(
                "seed {seed} node {ours:?} ({:?}): cephal x={} y={} w={} h={} / taffy x={} y={} w={} h={}",
                tree.style(ours).display, a.location.x, a.location.y, a.size.width, a.size.height, b.location.x, b.location.y, b.size.width, b.size.height
            ));
        }
    }
    errors
}

/// Seeds whose divergence is recorded but not yet resolved.
///
/// 106: a grid item with `width: fit-content(114px)`, `height: 72%`, `aspect-ratio: 2` and
/// `margin-left: auto` inside a `min-content` row; both engines keep the ratio but pick
/// different heights.
/// The others are grids with percentage `grid-auto-rows` in indefinite-height containers.
///
/// 188, 236 and 299: a grid measured under a min-content height. Cephal sizes it as under a
/// max-content height (CSS Sizing 3 §5.1). Taffy gets no free space for the rows there, so its
/// answer is smaller than the height the grid takes when it is laid out.
///
/// 2 and 168: a stretched item in a column container whose width is indefinite. Cephal measures
/// the item's height at the container's fit-content width, which the item gets in the end. Taffy
/// measures it at the whole available width.
const KNOWN_DIVERGENT: &[u64] = &[2, 106, 168, 188, 236, 299, 306, 563, 603, 1000, 1210, 1384, 1490];

/// A tree that differs is compared again relaxed (see [`run`]):
///
/// - A flex or grid container transfers a known axis through `aspect-ratio`, as a block container
///   does and as browsers do. Taffy 0.14 applies the ratio to style sizes only.
/// - A flex item with a sizing keyword on its cross size has its main size measured at that size.
///   Taffy 0.14 measures it stretched, and then lays it out at the keyword's size.
#[test]
fn matches_taffy_on_random_trees() {
    let seeds: u64 = std::env::var("CEPHAL_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let mut failing = Vec::new();
    for seed in (1..=seeds).filter(|s| !KNOWN_DIVERGENT.contains(s)) {
        if run(seed, false).is_empty() {
            continue;
        }
        let errors = run(seed, true);
        if !errors.is_empty() {
            failing.push(errors);
        }
    }
    let shown: Vec<String> = failing.iter().take(10).map(|e| e.join("\n")).collect();
    assert!(failing.is_empty(), "{} of {seeds} trees differ from taffy\n{}", failing.len(), shown.join("\n\n"));
}

/// `CEPHAL_SEED=106 cargo test --test differential debug_diff -- --ignored --nocapture`
#[test]
#[ignore]
fn debug_diff() {
    let seed: u64 = std::env::var("CEPHAL_SEED").unwrap().parse().unwrap();
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut tree: Tree<Text> = Tree::new();
    let root = build_subtree(&mut rng, &mut tree, 0);
    tree.disable_rounding();
    let viewport = Size {
        width: AvailableSpace::Definite(rng.below(600) as f32 + 50.0),
        height: rng.pick(&[AvailableSpace::MaxContent, AvailableSpace::Definite(300.0)]),
    };
    println!("viewport {viewport:?}");
    tree.compute_layout_with_measure(root, viewport, measure);
    let (mut taffy_tree, taffy_root, map) = mirror_in_taffy(&tree, root);
    taffy_tree.disable_rounding();
    taffy_tree.compute_layout_with_measure(taffy_root, to_taffy_space(viewport), taffy_measure).unwrap();
    for (ours, theirs) in map {
        let s = tree.style(ours);
        let a = tree.unrounded_layout(ours);
        let b = taffy_tree.unrounded_layout(theirs);
        let flag = if (a.size.width - b.size.width).abs() > 0.05 || (a.size.height - b.size.height).abs() > 0.05 || (a.location.x - b.location.x).abs() > 0.05 || (a.location.y - b.location.y).abs() > 0.05 { "DIFF" } else { "ok  " };
        println!(
            "{flag} {ours:?} depth={} parent={:?} ctx={:?} display={:?} pos={:?} size={:?} min={:?} max={:?} margin={:?} padding={:?} ar={:?} ov={:?} fd={:?} fw={:?} grow={} shrink={} basis={:?} ai={:?} as={:?} jc={:?} ac={:?} grid={:?} col={:?} row={:?}",
            tree.depth(ours), tree.parent(ours), tree.context(ours).map(|t| t.0), s.display, s.position, s.size, s.min_size, s.max_size, s.margin, s.padding, s.aspect_ratio, s.overflow, s.flex_direction, s.flex_wrap, s.flex_grow, s.flex_shrink, s.flex_basis, s.align_items, s.align_self, s.justify_content, s.align_content, s.grid, s.grid_column, s.grid_row
        );
        println!("       cephal x={} y={} w={} h={} | taffy x={} y={} w={} h={}", a.location.x, a.location.y, a.size.width, a.size.height, b.location.x, b.location.y, b.size.width, b.size.height);
    }
}
