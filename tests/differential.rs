//! Random trees laid out by cephal and taffy 0.15 must agree, apart from recorded divergences.

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
    run_with(seed, relaxed, &[])
}

/// Removes one feature from every node before both engines see the tree, to attribute a difference.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Strip {
    /// Margins and insets of the root.
    RootOffsets,
    /// `position: absolute`.
    Absolute,
    /// `align-self` / `justify-self` of absolutely positioned boxes.
    AbsoluteSelfAlignment,
    /// Percentage `flex-basis`.
    PercentBasis,
    /// Percentage padding.
    PercentPadding,
    /// Percentage grid tracks.
    PercentTracks,
    /// Margins of grid items.
    GridItemMargins,
    /// `min-content` grid tracks.
    MinContentTracks,
}

const STRIPS: &[Strip] = &[
    Strip::RootOffsets,
    Strip::Absolute,
    Strip::AbsoluteSelfAlignment,
    Strip::PercentBasis,
    Strip::PercentPadding,
    Strip::PercentTracks,
    Strip::GridItemMargins,
    Strip::MinContentTracks,
];

fn strip(tree: &mut Tree<Text>, root: cephal::tree::NodeId, what: Strip) {
    use cephal::style::{GridTemplateComponent, LengthPercentage, LengthPercentageAuto, Position, TrackSizingFunction};
    let mut nodes = Vec::new();
    all_nodes(tree, root, &mut nodes);
    for node in nodes {
        let parent_is_grid = tree.parent(node).is_some_and(|p| tree.style(p).display == Display::Grid);
        tree.patch_style(node, |s| match what {
            Strip::RootOffsets if node == root => {
                s.margin = cephal::geometry::Rect::splat(LengthPercentageAuto::ZERO);
                s.inset = cephal::geometry::Rect::splat(LengthPercentageAuto::AUTO);
                s.position = Position::Relative;
            }
            Strip::Absolute if s.position != Position::Relative => {
                s.position = Position::Relative;
                s.inset = cephal::geometry::Rect::splat(LengthPercentageAuto::AUTO);
            }
            Strip::AbsoluteSelfAlignment if s.position != Position::Relative => {
                s.align_self = None;
                s.justify_self = None;
            }
            Strip::PercentBasis if s.flex_basis.0.uses_percentage() => s.flex_basis = Dimension::AUTO,
            Strip::PercentPadding => {
                for side in [&mut s.padding.left, &mut s.padding.right, &mut s.padding.top, &mut s.padding.bottom] {
                    if side.0.uses_percentage() {
                        *side = LengthPercentage::ZERO;
                    }
                }
            }
            Strip::PercentTracks | Strip::MinContentTracks if s.display == Display::Grid => {
                let percent = what == Strip::PercentTracks;
                let fix = |t: &mut TrackSizingFunction| {
                    let hit = if percent { t.min.0.uses_percentage() || t.max.0.uses_percentage() } else { *t == TrackSizingFunction::MIN_CONTENT };
                    if hit {
                        *t = TrackSizingFunction::AUTO;
                    }
                };
                let g = s.grid_mut();
                for c in g.template_columns.iter_mut().chain(g.template_rows.iter_mut()) {
                    if let GridTemplateComponent::Single(t) = c {
                        fix(t);
                    }
                }
                g.auto_rows.iter_mut().for_each(fix);
            }
            Strip::GridItemMargins if parent_is_grid => s.margin = cephal::geometry::Rect::splat(LengthPercentageAuto::ZERO),
            _ => {}
        });
    }
}

fn run_with(seed: u64, relaxed: bool, strips: &[Strip]) -> Vec<String> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut tree: Tree<Text> = Tree::new();
    let root = build_subtree(&mut rng, &mut tree, 0);
    for &what in strips {
        strip(&mut tree, root, what);
    }
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

/// Seeds where cephal and taffy 0.15 disagree even relaxed, one per line with the census category
/// (see [`census`]): the single feature whose removal makes them agree, `combination` when only
/// removing all of them does, `unexplained` otherwise. Regenerate with
/// `CEPHAL_SEEDS=1000 CEPHAL_CENSUS_WRITE=tests/differential_known.txt cargo test --release --test differential census -- --ignored`.
const KNOWN_DIVERGENT: &str = include_str!("differential_known.txt");

fn known_divergent() -> std::collections::BTreeMap<u64, &'static str> {
    KNOWN_DIVERGENT
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (seed, category) = l.split_once(' ').unwrap_or((l, ""));
            (seed.parse().unwrap(), category)
        })
        .collect()
}

/// A tree that differs is compared again relaxed (see [`run`]):
///
/// - A flex or grid container transfers a known axis through `aspect-ratio`, as a block container
///   does and as browsers do. Taffy applies the ratio to style sizes only.
/// - A flex item with a sizing keyword on its cross size has its main size measured at that size.
///   Taffy measures it stretched, and then lays it out at the keyword's size.
///
/// Every other difference must be listed in `differential_known.txt`, and every listed seed must
/// still differ, so the list only shrinks on purpose.
#[test]
fn matches_taffy_on_random_trees() {
    let seeds: u64 = std::env::var("CEPHAL_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let known = known_divergent();
    let mut failing = Vec::new();
    let mut agreeing = Vec::new();
    for seed in 1..=seeds {
        let differs = !run(seed, false).is_empty() && {
            let errors = run(seed, true);
            let differs = !errors.is_empty();
            if differs && !known.contains_key(&seed) {
                failing.push(errors);
            }
            differs
        };
        if !differs && known.contains_key(&seed) {
            agreeing.push(seed);
        }
    }
    let shown: Vec<String> = failing.iter().take(10).map(|e| e.join("\n")).collect();
    assert!(failing.is_empty(), "{} of {seeds} trees differ from taffy\n{}", failing.len(), shown.join("\n\n"));
    assert!(agreeing.is_empty(), "now agree with taffy; remove from differential_known.txt: {agreeing:?}");
}

/// Attributes every differing seed to the single feature whose removal makes the engines agree.
///
/// `CEPHAL_SEEDS=1000 cargo test --release --test differential census -- --ignored --nocapture`
#[test]
#[ignore]
fn census() {
    let seeds: u64 = std::env::var("CEPHAL_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let mut by_strip: std::collections::BTreeMap<String, Vec<u64>> = Default::default();
    for seed in 1..=seeds {
        if run(seed, false).is_empty() || run(seed, true).is_empty() {
            continue;
        }
        let single: Vec<Strip> = STRIPS.iter().copied().filter(|&w| run_with(seed, true, &[w]).is_empty()).collect();
        let key = if !single.is_empty() {
            format!("{single:?}")
        } else if run_with(seed, true, STRIPS).is_empty() {
            "combination".to_string()
        } else {
            "unexplained".to_string()
        };
        by_strip.entry(key).or_default().push(seed);
    }
    let mut lines: Vec<(u64, String)> = Vec::new();
    for (key, seeds) in &by_strip {
        println!("{:4} {key}: {:?}", seeds.len(), &seeds[..seeds.len().min(40)]);
        lines.extend(seeds.iter().map(|&s| (s, key.replace(' ', ""))));
    }
    if let Ok(path) = std::env::var("CEPHAL_CENSUS_WRITE") {
        lines.sort();
        let mut out = String::from("# seed census-category (generated by the `census` test; see differential.rs)\n");
        for (seed, key) in lines {
            out += &format!("{seed} {key}\n");
        }
        std::fs::write(path, out).unwrap();
    }
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
