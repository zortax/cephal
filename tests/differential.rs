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
    /// Sizing keywords of the root's size: taffy takes them as `auto`.
    RootSizingKeywords,
    /// Min and max widths of the root: taffy does not clamp a block root's width by them.
    RootMinMaxWidth,
    /// Out-of-flow positioning: such boxes become static.
    Absolute,
    /// `align-self` / `justify-self` of absolutely positioned boxes.
    AbsoluteSelfAlignment,
    /// Grid placement of out-of-flow boxes under a grid root: taffy resolves it against the root's
    /// grid for boxes the initial containing block lays out, which is no grid (css-grid-1 §9.1).
    InitialContainingBlockGridPlacement,
    /// Margins of out-of-flow boxes.
    OutOfFlowMargins,
    /// `position: relative` and `sticky`: such boxes become static.
    Positioned,
    /// Percentage `flex-basis`.
    PercentBasis,
    /// Percentage padding.
    PercentPadding,
    /// Percentage gaps: taffy resolves a column flex container's row gap against the height it
    /// finds for it.
    PercentGaps,
    /// Percentage margins: taffy resolves those of a block's children against the available
    /// space when it finds the block's intrinsic width.
    PercentMargins,
    /// Percentage grid tracks.
    PercentTracks,
    /// Margins of grid items.
    GridItemMargins,
    /// Padding and borders of grid items: taffy lets a definite preferred, minimum or maximum size
    /// make an item's minimum contribution smaller than its padding and border.
    GridItemPaddingBorder,
    /// `min-content` grid tracks.
    MinContentTracks,
    /// `align-content` of block containers: taffy aligns content that overflows unsafely.
    BlockAlignContent,
    /// `space-between`: taffy falls back to `start` instead of `flex-start` when the items
    /// overflow a reversed flex container.
    SpaceBetween,
    /// `flex-wrap: wrap-reverse`: taffy places lines that overflow at the cross-end edge.
    WrapReverse,
    /// Multi-line flex containers: taffy puts the items of one whose main size is not definite on
    /// a single line, whatever size it is laid out at.
    FlexWrap,
    /// Cross-axis alignment in flex containers (items are stretched instead): taffy lets the
    /// percentage heights inside a row flex item that is not stretched resolve against its height.
    FlexCrossAlignment,
    /// Padding, borders and scrollbars of measured leaves: taffy measures a leaf of known size at
    /// its border-box size instead of its content-box size.
    LeafInsets,
}

const STRIPS: &[Strip] = &[
    Strip::RootMinMaxWidth,
    Strip::RootSizingKeywords,
    Strip::RootOffsets,
    Strip::Absolute,
    Strip::AbsoluteSelfAlignment,
    Strip::InitialContainingBlockGridPlacement,
    Strip::OutOfFlowMargins,
    Strip::Positioned,
    Strip::PercentBasis,
    Strip::PercentPadding,
    Strip::PercentGaps,
    Strip::PercentMargins,
    Strip::PercentTracks,
    Strip::GridItemMargins,
    Strip::GridItemPaddingBorder,
    Strip::MinContentTracks,
    Strip::SpaceBetween,
    Strip::BlockAlignContent,
    Strip::WrapReverse,
    Strip::FlexWrap,
    Strip::FlexCrossAlignment,
    Strip::LeafInsets,
];

fn strip(tree: &mut Tree<Text>, root: cephal::tree::NodeId, what: Strip) {
    use cephal::style::{GridTemplateComponent, LengthPercentage, LengthPercentageAuto, Position, TrackSizingFunction};
    let mut nodes = Vec::new();
    all_nodes(tree, root, &mut nodes);
    let root_is_grid = tree.style(root).display == Display::Grid;
    for node in nodes {
        let parent_is_grid = tree.parent(node).is_some_and(|p| tree.style(p).display == Display::Grid);
        let initial_containing_block = tree.containing_block(node).is_none();
        let is_text = tree.context(node).is_some();
        let parent_is_flex = tree.parent(node).is_some_and(|p| tree.style(p).display == Display::Flex);
        tree.patch_style(node, |s| match what {
            Strip::RootMinMaxWidth if node == root => {
                s.min_size.width = LengthPercentageAuto::AUTO;
                s.max_size.width = LengthPercentageAuto::AUTO;
            }
            Strip::RootSizingKeywords if node == root => {
                for d in [&mut s.size.width, &mut s.size.height] {
                    if d.is_sizing_keyword() {
                        *d = Dimension::AUTO;
                    }
                }
            }
            Strip::RootOffsets if node == root => {
                s.margin = cephal::geometry::Rect::splat(LengthPercentageAuto::ZERO);
                s.inset = cephal::geometry::Rect::splat(LengthPercentageAuto::AUTO);
                if s.position.is_out_of_flow() {
                    s.position = Position::Static;
                }
            }
            Strip::Absolute if s.position.is_out_of_flow() => {
                s.position = Position::Static;
                s.inset = cephal::geometry::Rect::splat(LengthPercentageAuto::AUTO);
            }
            Strip::AbsoluteSelfAlignment if s.position.is_out_of_flow() => {
                s.align_self = None;
                s.justify_self = None;
            }
            Strip::InitialContainingBlockGridPlacement if root_is_grid && s.position.is_out_of_flow() && initial_containing_block => {
                s.grid_row = Default::default();
                s.grid_column = Default::default();
            }
            Strip::OutOfFlowMargins if s.position.is_out_of_flow() => s.margin = cephal::geometry::Rect::splat(LengthPercentageAuto::ZERO),
            Strip::Positioned if matches!(s.position, Position::Relative | Position::Sticky) => {
                s.position = Position::Static;
                s.inset = cephal::geometry::Rect::splat(LengthPercentageAuto::AUTO);
            }
            Strip::PercentBasis if s.flex_basis.0.uses_percentage() => s.flex_basis = Dimension::AUTO,
            Strip::PercentPadding => {
                for side in [&mut s.padding.left, &mut s.padding.right, &mut s.padding.top, &mut s.padding.bottom] {
                    if side.0.uses_percentage() {
                        *side = LengthPercentage::ZERO;
                    }
                }
            }
            Strip::PercentGaps => {
                for gap in [&mut s.gap.width, &mut s.gap.height] {
                    if gap.0.uses_percentage() {
                        *gap = LengthPercentage::ZERO;
                    }
                }
            }
            Strip::PercentMargins => {
                for side in [&mut s.margin.left, &mut s.margin.right, &mut s.margin.top, &mut s.margin.bottom] {
                    if side.0.uses_percentage() {
                        *side = LengthPercentageAuto::ZERO;
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
            Strip::SpaceBetween => {
                for a in [&mut s.justify_content, &mut s.align_content] {
                    if a.is_some_and(|a| a.keyword == cephal::style::AlignContentKeyword::SpaceBetween) {
                        *a = None;
                    }
                }
            }
            Strip::BlockAlignContent if s.display == Display::Block => s.align_content = None,
            Strip::WrapReverse if s.flex_wrap == cephal::style::FlexWrap::WrapReverse => s.flex_wrap = cephal::style::FlexWrap::Wrap,
            Strip::FlexWrap => s.flex_wrap = cephal::style::FlexWrap::NoWrap,
            Strip::FlexCrossAlignment => {
                if s.display == Display::Flex {
                    s.align_items = None;
                }
                if parent_is_flex {
                    s.align_self = None;
                    for m in [&mut s.margin.top, &mut s.margin.bottom, &mut s.margin.left, &mut s.margin.right] {
                        if m.is_auto() {
                            *m = LengthPercentageAuto::ZERO;
                        }
                    }
                }
            }
            Strip::LeafInsets if is_text => {
                s.padding = cephal::geometry::Rect::splat(LengthPercentage::ZERO);
                s.border = cephal::geometry::Rect::splat(LengthPercentage::ZERO);
                s.overflow = cephal::geometry::Point { x: cephal::style::Overflow::Visible, y: cephal::style::Overflow::Visible };
            }
            Strip::GridItemPaddingBorder if parent_is_grid => {
                s.padding = cephal::geometry::Rect::splat(LengthPercentage::ZERO);
                s.border = cephal::geometry::Rect::splat(LengthPercentage::ZERO);
            }
            _ => {}
        });
    }
}

/// The tree of `seed` and its viewport, as [`run_with`] lays them out.
fn seed_tree(seed: u64, relaxed: bool, strips: &[Strip]) -> (Tree<Text>, cephal::tree::NodeId, Size<AvailableSpace>) {
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
    (tree, root, viewport)
}

fn run_with(seed: u64, relaxed: bool, strips: &[Strip]) -> Vec<String> {
    let (mut tree, root, viewport) = seed_tree(seed, relaxed, strips);
    tree.compute_layout_with_measure(root, viewport, measure);
    let (mut taffy_tree, taffy_root, map) = mirror_in_taffy(&tree, root);
    taffy_tree.disable_rounding();
    taffy_tree.compute_layout_with_measure(taffy_root, to_taffy_space(viewport), taffy_measure).unwrap();
    let mut errors = Vec::new();
    let theirs_relative = taffy_parent_relative(&tree, &taffy_tree, &map);
    for (&(ours, _), b) in map.iter().zip(theirs_relative) {
        let a = tree.unrounded_layout(ours);
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

/// Taffy's unrounded layouts with locations relative to the parent, as cephal's: taffy places an
/// out-of-flow box relative to its containing block's border box (the root's for the initial
/// containing block). `map` is in tree order.
fn taffy_parent_relative(tree: &Tree<Text>, taffy_tree: &taffy::TaffyTree<Text>, map: &[(cephal::tree::NodeId, taffy::NodeId)]) -> Vec<taffy::Layout> {
    use cephal::style::Position;
    let mut abs: std::collections::HashMap<cephal::tree::NodeId, (f32, f32)> = Default::default();
    let mut out = Vec::new();
    let root = map[0].0;
    for &(ours, theirs) in map {
        let mut layout = *taffy_tree.unrounded_layout(theirs);
        let hidden = std::iter::successors(Some(ours), |&n| tree.parent(n)).any(|n| tree.style(n).display == Display::None);
        let position = tree.style(ours).position;
        let here = match tree.parent(ours) {
            None => (layout.location.x, layout.location.y),
            Some(parent) => {
                let p = abs[&parent];
                let cb = match position {
                    _ if hidden => None,
                    Position::Absolute => Some(
                        std::iter::successors(Some(parent), |&n| tree.parent(n)).find(|&n| tree.style(n).position != Position::Static).unwrap_or(root),
                    ),
                    Position::Fixed => Some(root),
                    _ => None,
                };
                match cb {
                    Some(cb) => {
                        let c = abs[&cb];
                        let here = (c.0 + layout.location.x, c.1 + layout.location.y);
                        layout.location = taffy::Point { x: here.0 - p.0, y: here.1 - p.1 };
                        here
                    }
                    None => (p.0 + layout.location.x, p.1 + layout.location.y),
                }
            }
        };
        abs.insert(ours, here);
        out.push(layout);
    }
    out
}

/// Seeds where cephal and taffy 0.15 disagree even relaxed, one per line with the census category
/// (see [`census`]): the features each of whose removal makes them agree (`[A,B]`), else the
/// features that must all go together (`A+B`), or `unexplained` when removing all is not enough.
/// Regenerate with
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
                failing.push((seed, errors));
            }
            differs
        };
        if !differs && known.contains_key(&seed) {
            agreeing.push(seed);
        }
    }
    let shown: Vec<String> = failing.iter().take(10).map(|(_, e)| e.join("\n")).collect();
    let listed: Vec<u64> = failing.iter().map(|(s, _)| *s).collect();
    assert!(failing.is_empty(), "{} of {seeds} trees differ from taffy: {listed:?}\n{}", failing.len(), shown.join("\n\n"));
    assert!(agreeing.is_empty(), "now agree with taffy; remove from differential_known.txt: {agreeing:?}");
}

/// Attributes every differing seed to the features whose removal makes the engines agree.
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
            // The features that must all go: drop every strip the engines agree without.
            let mut needed = STRIPS.to_vec();
            for &w in STRIPS {
                let without: Vec<Strip> = needed.iter().copied().filter(|&n| n != w).collect();
                if run_with(seed, true, &without).is_empty() {
                    needed = without;
                }
            }
            format!("{}", needed.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>().join("+"))
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

/// `CEPHAL_SEED=106 cargo test --test differential debug_diff -- --ignored --nocapture`; the relaxed
/// tree with `CEPHAL_RELAXED=1`.
#[test]
#[ignore]
fn debug_diff() {
    let seed: u64 = std::env::var("CEPHAL_SEED").unwrap().parse().unwrap();
    let (mut tree, root, viewport) = seed_tree(seed, std::env::var_os("CEPHAL_RELAXED").is_some(), &[]);
    println!("viewport {viewport:?}");
    tree.compute_layout_with_measure(root, viewport, measure);
    let (mut taffy_tree, taffy_root, map) = mirror_in_taffy(&tree, root);
    taffy_tree.disable_rounding();
    taffy_tree.compute_layout_with_measure(taffy_root, to_taffy_space(viewport), taffy_measure).unwrap();
    let theirs_relative = taffy_parent_relative(&tree, &taffy_tree, &map);
    for (&(ours, _), b) in map.iter().zip(theirs_relative) {
        let s = tree.style(ours);
        let a = tree.unrounded_layout(ours);
        let flag = if (a.size.width - b.size.width).abs() > 0.05 || (a.size.height - b.size.height).abs() > 0.05 || (a.location.x - b.location.x).abs() > 0.05 || (a.location.y - b.location.y).abs() > 0.05 { "DIFF" } else { "ok  " };
        println!(
            "{flag} {ours:?} depth={} parent={:?} ctx={:?} display={:?} pos={:?} inset={:?} size={:?} min={:?} max={:?} margin={:?} padding={:?} ar={:?} ov={:?} fd={:?} fw={:?} grow={} shrink={} basis={:?} ai={:?} as={:?} jc={:?} ac={:?} grid={:?} col={:?} row={:?}",
            tree.depth(ours), tree.parent(ours), tree.context(ours).map(|t| t.0), s.display, s.position, s.inset, s.size, s.min_size, s.max_size, s.margin, s.padding, s.aspect_ratio, s.overflow, s.flex_direction, s.flex_wrap, s.flex_grow, s.flex_shrink, s.flex_basis, s.align_items, s.align_self, s.justify_content, s.align_content, s.grid, s.grid_column, s.grid_row
        );
        println!("       cephal x={} y={} w={} h={} | taffy x={} y={} w={} h={}", a.location.x, a.location.y, a.size.width, a.size.height, b.location.x, b.location.y, b.size.width, b.size.height);
    }
}

// --- Chrome adjudication ------------------------------------------------------------------------
//
//   CEPHAL_SEEDS=721,940 cargo test --release --test differential to_html -- --ignored --nocapture
//   scripts/gen_fixtures.sh target/diff_seeds/html target/diff_seeds/chrome.xml
//   CEPHAL_SEEDS=721,940 cargo test --release --test differential adjudicate -- --ignored --nocapture
//
// `CEPHAL_SEEDS` is a list of seeds and ranges (`1-300,721`); all seeds of differential_known.txt
// by default. `to_html` writes `diff_seed_<n>.html` (the tree `run` compares unrelaxed) and
// `diff_seed_<n>_relaxed.html` into `CEPHAL_HTML_DIR` (default target/diff_seeds/html); gentest
// renders them in Chrome; `adjudicate` lays the same trees out in cephal and taffy and compares
// both with Chrome (`CEPHAL_CHROME_XML`, default target/diff_seeds/chrome.xml). `CEPHAL_VERBOSE=1`
// lists agreeing boxes too.

#[path = "common/chrome.rs"]
mod chrome;

fn chrome_seeds() -> Vec<u64> {
    match std::env::var("CEPHAL_SEEDS") {
        Ok(v) => v
            .split(',')
            .flat_map(|part| match part.split_once('-') {
                Some((a, b)) => (a.trim().parse::<u64>().unwrap()..=b.trim().parse().unwrap()).collect::<Vec<_>>(),
                None => vec![part.trim().parse().unwrap()],
            })
            .collect(),
        Err(_) => known_divergent().keys().copied().collect(),
    }
}

/// Unrounded layouts of every node in `all_nodes` order, parent-relative: cephal's and taffy's.
/// `chrome_text` measures text as Chrome sets Ahem text (see `chrome_text`).
fn both_layouts(
    tree: &Tree<Text>,
    root: cephal::tree::NodeId,
    viewport: Size<AvailableSpace>,
    chrome_text: bool,
) -> (Vec<chrome::Rect4>, Vec<chrome::Rect4>) {
    let mut ours = tree.clone_cold();
    ours.disable_rounding();
    if chrome_text {
        ours.compute_layout_with_measure(root, viewport, measure_chrome);
    } else {
        ours.compute_layout_with_measure(root, viewport, measure);
    }
    let (mut taffy_tree, taffy_root, map) = mirror_in_taffy(&ours, root);
    taffy_tree.disable_rounding();
    if chrome_text {
        taffy_tree.compute_layout_with_measure(taffy_root, to_taffy_space(viewport), taffy_measure_chrome).unwrap();
    } else {
        taffy_tree.compute_layout_with_measure(taffy_root, to_taffy_space(viewport), taffy_measure).unwrap();
    }
    let theirs = taffy_parent_relative(&ours, &taffy_tree, &map);
    let a = map.iter().map(|&(n, _)| {
        let l = ours.unrounded_layout(n);
        chrome::Rect4 { x: l.location.x, y: l.location.y, w: l.size.width, h: l.size.height }
    });
    let b = theirs.iter().map(|l| chrome::Rect4 { x: l.location.x, y: l.location.y, w: l.size.width, h: l.size.height });
    (a.collect(), b.collect())
}

/// Ahem text as Chrome sets it: `common::measure`'s glyphs, of which a line holds one more when
/// it falls short of them by less than two layout units (Chrome lays out in 1/64px); the first
/// baseline 8px below the content box's top (the font's ascent), the last one a line further down
/// per wrapped line. Returns the content size and the baselines.
fn chrome_text(n: u32, known: Size<Option<f32>>, available_width: AvailableSpace) -> (Size<f32>, f32, f32) {
    let glyph = 10.0;
    let total = n as f32 * glyph;
    let width = known.width.unwrap_or(match available_width {
        AvailableSpace::MinContent => glyph,
        AvailableSpace::MaxContent => total,
        AvailableSpace::Definite(w) => w.min(total).max(glyph),
    });
    let per_line = ((width + 2.0 / 64.0) / glyph).floor().max(1.0);
    let lines = (n as f32 / per_line).ceil().max(1.0);
    let height = known.height.unwrap_or(lines * glyph);
    (Size { width, height }, 8.0, (lines - 1.0) * glyph + 8.0)
}

fn measure_chrome(ctx: &mut Text, input: cephal::tree::MeasureInput, _: &cephal::style::Style) -> cephal::tree::MeasureOutput {
    let (size, first, last) = chrome_text(ctx.0, input.known_dimensions, input.available_space.width);
    cephal::tree::MeasureOutput { size, baselines: cephal::tree::Baselines { first: Some(first), last: Some(last) } }
}

fn taffy_measure_chrome(
    inputs: taffy::LayoutInput,
    _node: taffy::NodeId,
    context: Option<&mut Text>,
    style: &taffy::Style,
) -> taffy::LayoutOutput {
    use taffy::util::ResolveOrZero;
    let text = context.map(|t| t.0);
    let mut baselines = None;
    let mut out = taffy::compute_leaf_layout(inputs, style, |_, _| 0.0, |known, available| {
        let Some(n) = text else { return known.map(|d| d.unwrap_or(0.0)) };
        let available_width = match available.width {
            taffy::AvailableSpace::Definite(v) => AvailableSpace::Definite(v),
            taffy::AvailableSpace::MinContent => AvailableSpace::MinContent,
            taffy::AvailableSpace::MaxContent => AvailableSpace::MaxContent,
        };
        let (size, first, last) = chrome_text(n, Size { width: known.width, height: known.height }, available_width);
        baselines = Some((first, last));
        taffy::Size { width: size.width, height: size.height }
    });
    if let Some((first, last)) = baselines {
        let basis = inputs.parent_size.width;
        let padding: taffy::Rect<f32> = style.padding.resolve_or_zero(basis, |_, _| 0.0);
        let border: taffy::Rect<f32> = style.border.resolve_or_zero(basis, |_, _| 0.0);
        let top = padding.top + border.top;
        out.baselines = taffy::Baselines { first: Some(top + first), last: Some(top + last) };
    }
    out
}

fn near(a: chrome::Rect4, b: chrome::Rect4, tolerance: f32) -> bool {
    (a.x - b.x).abs() <= tolerance && (a.y - b.y).abs() <= tolerance && (a.w - b.w).abs() <= tolerance && (a.h - b.h).abs() <= tolerance
}

/// The tree of `seed` with the sizes Chrome cannot express (`fit-content(<length>)`) replaced by
/// values under which cephal and taffy each lay the tree out exactly as before; sizes with no such
/// replacement stay, and `chrome::caveats` reports them. Returns the substitutions made.
fn expressible(seed: u64, relaxed: bool) -> (Tree<Text>, cephal::tree::NodeId, Size<AvailableSpace>, Vec<String>) {
    use cephal::style::LengthKind;
    let (mut tree, root, viewport) = seed_tree(seed, relaxed, &[]);
    let reference = both_layouts(&tree, root, viewport, false);
    let same = |t: &Tree<Text>| {
        let (a, b) = both_layouts(t, root, viewport, false);
        a.iter().zip(&reference.0).all(|(x, y)| near(*x, *y, 0.001)) && b.iter().zip(&reference.1).all(|(x, y)| near(*x, *y, 0.001))
    };
    let mut nodes = Vec::new();
    all_nodes(&tree, root, &mut nodes);
    let mut done = Vec::new();
    let names = ["width", "height", "flex-basis"];
    let ix = |n: cephal::tree::NodeId| nodes.iter().position(|&m| m == n).unwrap();
    let set = |s: &mut cephal::style::Style, field: usize, v: Dimension| *[&mut s.size.width, &mut s.size.height, &mut s.flex_basis][field] = v;
    for &n in &nodes {
        for field in 0..3 {
            let value = [tree.style(n).size.width, tree.style(n).size.height, tree.style(n).flex_basis][field];
            if !matches!(value.raw().kind(), LengthKind::FitContentPx(_) | LengthKind::FitContentPercent(_)) {
                continue;
            }
            let i = nodes.iter().position(|&m| m == n).unwrap();
            let size = reference.0[i];
            let mut candidates = vec![Dimension::FIT_CONTENT, Dimension::MAX_CONTENT, Dimension::MIN_CONTENT, Dimension::AUTO];
            if field < 2 {
                candidates.push(Dimension::length(if field == 0 { size.w } else { size.h }));
            }
            let found = candidates.into_iter().find(|&c| {
                let mut t = tree.clone_cold();
                t.patch_style(n, |s| set(s, field, c));
                same(&t)
            });
            if let Some(c) = found {
                tree.patch_style(n, |s| set(s, field, c));
                done.push(format!("#{} {}: {value:?} as {c:?}", ix(n), names[field]));
            }
        }
    }
    // The root model of both engines (see TAFFY_DIVERGENCES.md): an in-flow root's `auto` margins
    // count as zero, and an in-flow flex or grid root of `auto` width takes the width its algorithm
    // gives it rather than filling the viewport as CSS's block-level root box does. Pin both, so
    // Chrome judges the rest of the tree.
    let style = tree.style(root).clone();
    if !(style.position.is_out_of_flow() && matches!(viewport.height, AvailableSpace::Definite(_))) {
        let mut t = tree.clone_cold();
        t.patch_style(root, |s| {
            for m in [&mut s.margin.left, &mut s.margin.right, &mut s.margin.top, &mut s.margin.bottom] {
                if m.is_auto() {
                    *m = cephal::style::LengthPercentageAuto::ZERO;
                }
            }
        });
        if style.margin != t.style(root).margin && same(&t) {
            tree = t;
            done.push("#0 auto margins as 0".into());
        }
        if matches!(style.display, Display::Flex | Display::Grid) && style.size.width.is_auto() && (reference.0[0].w - reference.1[0].w).abs() < 0.001 {
            let width = Dimension::length(reference.0[0].w);
            let mut t = tree.clone_cold();
            t.patch_style(root, |s| s.size.width = width);
            if same(&t) {
                tree = t;
                done.push(format!("#0 width: auto as {width:?}"));
            }
        }
    }
    (tree, root, viewport, done)
}

/// Writes the fixtures of `chrome_seeds()` for gentest (see above).
#[test]
#[ignore]
fn to_html() {
    let dir = std::env::var("CEPHAL_HTML_DIR").unwrap_or_else(|_| format!("{}/target/diff_seeds/html", env!("CARGO_MANIFEST_DIR")));
    std::fs::create_dir_all(&dir).unwrap();
    for seed in chrome_seeds() {
        for relaxed in [false, true] {
            let (tree, root, viewport, substituted) = expressible(seed, relaxed);
            let caveats = chrome::caveats(&tree, root, viewport);
            let name = if relaxed { format!("diff_seed_{seed}_relaxed") } else { format!("diff_seed_{seed}") };
            let mut title = format!("Random tree {seed}{} of tests/differential.rs, viewport {viewport:?}", if relaxed { ", relaxed" } else { "" });
            if !substituted.is_empty() {
                title += &format!(". Substituted, leaving cephal and taffy unchanged: {}", substituted.join("; "));
            }
            if !caveats.is_empty() {
                title += &format!(". Not expressible exactly: {}", caveats.join("; "));
            }
            std::fs::write(format!("{dir}/{name}.html"), chrome::to_html(&tree, root, viewport, &title)).unwrap();
            println!("{name}: {} substituted, {} caveats{}", substituted.len(), caveats.len(), caveats.iter().map(|c| format!("\n    {c}")).collect::<String>());
        }
    }
}

/// Compares cephal and taffy with Chrome on the fixtures `to_html` wrote (see above).
#[test]
#[ignore]
fn adjudicate() {
    let path = std::env::var("CEPHAL_CHROME_XML").unwrap_or_else(|_| format!("{}/target/diff_seeds/chrome.xml", env!("CARGO_MANIFEST_DIR")));
    let xml = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let verbose = std::env::var_os("CEPHAL_VERBOSE").is_some();
    for seed in chrome_seeds() {
        for relaxed in [false, true] {
            let (tree, root, viewport, _) = expressible(seed, relaxed);
            let name = if relaxed { format!("diff_seed_{seed}_relaxed") } else { format!("diff_seed_{seed}") };
            let Some(expected) = chrome::expectations(&xml, &format!("{name}__border_box_ltr"), &tree, root, viewport) else {
                println!("{name}: not rendered");
                continue;
            };
            let (ours, theirs) = both_layouts(&tree, root, viewport, true);
            let (plain_ours, plain_theirs) = both_layouts(&tree, root, viewport, false);
            let baselines_matter = ours.iter().zip(&plain_ours).any(|(a, b)| !near(*a, *b, 0.001))
                || theirs.iter().zip(&plain_theirs).any(|(a, b)| !near(*a, *b, 0.001));
            let mut nodes = Vec::new();
            all_nodes(&tree, root, &mut nodes);
            let tolerance = 0.1;
            let mut lines = Vec::new();
            let (mut cephal_wrong, mut taffy_wrong) = (0, 0);
            // Where the engines differ: which one Chrome agrees with.
            let mut verdicts = Vec::new();
            // Wrong boxes whose parent cephal gets right: where its mistakes start.
            let mut first_wrong = Vec::new();
            for (i, &n) in nodes.iter().enumerate() {
                let Some(c) = expected[i] else { continue };
                let (a, b) = (ours[i], theirs[i]);
                let (ca, tb, ab) = (near(a, c, tolerance), near(b, c, tolerance), near(a, b, 0.01));
                let parent_right = tree.parent(n).is_none_or(|p| {
                    let j = nodes.iter().position(|&m| m == p).unwrap();
                    expected[j].is_none_or(|e| near(ours[j], e, tolerance))
                });
                if !ca && parent_right {
                    first_wrong.push(format!("#{i}"));
                }
                cephal_wrong += !ca as usize;
                taffy_wrong += !tb as usize;
                if !ab {
                    verdicts.push(format!("#{i} {}", match (ca, tb) {
                        (true, false) => "cephal",
                        (false, true) => "taffy",
                        (false, false) => "neither",
                        (true, true) => "both",
                    }));
                }
                if verbose || !ca || !tb {
                    let s = tree.style(n);
                    let mark = match (ca, tb) {
                        (true, true) => "ok    ",
                        (false, true) => "CEPHAL",
                        (true, false) => "TAFFY ",
                        (false, false) => "BOTH  ",
                    };
                    let parent = tree.parent(n).map_or("-".to_string(), |p| format!("#{}", nodes.iter().position(|&m| m == p).unwrap()));
                    lines.push(format!(
                        "  {mark} #{i} depth={} parent={parent} {:?} {:?}\n         chrome {:>9.3} {:>9.3} {:>9.3} {:>9.3}\n         cephal {:>9.3} {:>9.3} {:>9.3} {:>9.3}\n         taffy  {:>9.3} {:>9.3} {:>9.3} {:>9.3}",
                        tree.depth(n), s.display, s.position, c.x, c.y, c.w, c.h, a.x, a.y, a.w, a.h, b.x, b.y, b.w, b.h
                    ));
                }
            }
            println!(
                "{name}: cephal {cephal_wrong} wrong (from {}), taffy {taffy_wrong} wrong; where they differ, Chrome agrees with: {}{}",
                if first_wrong.is_empty() { "-".to_string() } else { first_wrong.join(" ") },
                if verdicts.is_empty() { "-".to_string() } else { verdicts.join(", ") },
                if baselines_matter { " (Chrome's text model matters)" } else { "" }
            );
            for l in lines {
                println!("{l}");
            }
        }
    }
}

/// One edit the reducer tries.
#[derive(Clone, Copy, Debug)]
enum Edit {
    /// Removes the box and its subtree.
    Remove(usize),
    /// Replaces the box by its children.
    Hoist(usize),
    /// Removes or shortens the text.
    Text(usize, Option<u32>),
    /// Resets one property (see `reset`) to its initial value.
    Reset(usize, usize),
}

const RESETS: &[&str] = &[
    "display", "width", "height", "min-width", "min-height", "max-width", "max-height", "margin-left", "margin-right", "margin-top",
    "margin-bottom", "padding-left", "padding-right", "padding-top", "padding-bottom", "border", "gap", "aspect-ratio", "position",
    "left", "right", "top", "bottom", "overflow", "flex-direction", "flex-wrap", "flex-grow", "flex-shrink", "flex-basis",
    "align-items", "align-self", "justify-content", "align-content", "grid-template-columns", "grid-template-rows", "grid-auto-rows",
    "grid-row", "grid-column",
];

/// Resets property `which` of `RESETS`; false when it already has its initial value.
fn reset(s: &mut cephal::style::Style, which: usize) -> bool {
    use cephal::style::{LengthPercentage, LengthPercentageAuto, Position, Style};
    let d = Style::DEFAULT;
    let before = s.clone();
    match RESETS[which] {
        "display" => s.display = Display::Block,
        "width" => s.size.width = d.size.width,
        "height" => s.size.height = d.size.height,
        "min-width" => s.min_size.width = d.min_size.width,
        "min-height" => s.min_size.height = d.min_size.height,
        "max-width" => s.max_size.width = d.max_size.width,
        "max-height" => s.max_size.height = d.max_size.height,
        "margin-left" => s.margin.left = LengthPercentageAuto::ZERO,
        "margin-right" => s.margin.right = LengthPercentageAuto::ZERO,
        "margin-top" => s.margin.top = LengthPercentageAuto::ZERO,
        "margin-bottom" => s.margin.bottom = LengthPercentageAuto::ZERO,
        "padding-left" => s.padding.left = LengthPercentage::ZERO,
        "padding-right" => s.padding.right = LengthPercentage::ZERO,
        "padding-top" => s.padding.top = LengthPercentage::ZERO,
        "padding-bottom" => s.padding.bottom = LengthPercentage::ZERO,
        "border" => s.border = d.border,
        "gap" => s.gap = d.gap,
        "aspect-ratio" => s.aspect_ratio = None,
        "position" => {
            s.position = Position::Static;
            s.inset = d.inset;
        }
        "left" => s.inset.left = LengthPercentageAuto::AUTO,
        "right" => s.inset.right = LengthPercentageAuto::AUTO,
        "top" => s.inset.top = LengthPercentageAuto::AUTO,
        "bottom" => s.inset.bottom = LengthPercentageAuto::AUTO,
        "overflow" => {
            s.overflow = d.overflow;
            s.scrollbar_width = d.scrollbar_width;
        }
        "flex-direction" => s.flex_direction = d.flex_direction,
        "flex-wrap" => s.flex_wrap = d.flex_wrap,
        "flex-grow" => s.flex_grow = d.flex_grow,
        "flex-shrink" => s.flex_shrink = d.flex_shrink,
        "flex-basis" => s.flex_basis = d.flex_basis,
        "align-items" => s.align_items = None,
        "align-self" => s.align_self = None,
        "justify-content" => s.justify_content = None,
        "align-content" => s.align_content = None,
        "grid-template-columns" if s.grid.is_some() => s.grid_mut().template_columns.clear(),
        "grid-template-rows" if s.grid.is_some() => s.grid_mut().template_rows.clear(),
        "grid-auto-rows" if s.grid.is_some() => s.grid_mut().auto_rows.clear(),
        "grid-row" => s.grid_row = d.grid_row,
        "grid-column" => s.grid_column = d.grid_column,
        _ => {}
    }
    *s != before
}

/// Applies `edits` in order to a copy of `tree`, skipping those whose box is gone.
fn apply_edits(tree: &Tree<Text>, root: cephal::tree::NodeId, edits: &[Edit]) -> Tree<Text> {
    let mut nodes = Vec::new();
    all_nodes(tree, root, &mut nodes);
    let mut t = tree.clone_cold();
    let alive = |t: &Tree<Text>, n: cephal::tree::NodeId| std::iter::successors(Some(n), |&a| t.parent(a)).last() == Some(root);
    for &e in edits {
        match e {
            Edit::Remove(i) if alive(&t, nodes[i]) && i != 0 => t.remove(nodes[i]),
            Edit::Hoist(i) if alive(&t, nodes[i]) && i != 0 => {
                let n = nodes[i];
                let parent = t.parent(n).unwrap();
                let children: Vec<_> = t.children(n).collect();
                for c in children {
                    t.detach(c);
                    t.insert_child_before(parent, c, Some(n));
                }
                t.remove(n);
            }
            Edit::Text(i, text) if alive(&t, nodes[i]) => t.set_context(nodes[i], text.map(Text)),
            Edit::Reset(i, which) if alive(&t, nodes[i]) => t.patch_style(nodes[i], |s| {
                reset(s, which);
            }),
            _ => {}
        }
    }
    t
}

/// The single edits worth trying on `tree`, biggest first.
fn candidate_edits(tree: &Tree<Text>, root: cephal::tree::NodeId) -> Vec<Edit> {
    let mut nodes = Vec::new();
    all_nodes(tree, root, &mut nodes);
    let mut edits = Vec::new();
    for i in 1..nodes.len() {
        edits.push(Edit::Remove(i));
    }
    for (i, &n) in nodes.iter().enumerate().skip(1) {
        if tree.children(n).next().is_some() {
            edits.push(Edit::Hoist(i));
        }
    }
    for (i, &n) in nodes.iter().enumerate() {
        if let Some(t) = tree.context(n) {
            edits.push(Edit::Text(i, None));
            if t.0 > 1 {
                edits.push(Edit::Text(i, Some(t.0 / 2)));
            }
        }
        for which in 0..RESETS.len() {
            let mut s = tree.style(n).clone();
            if reset(&mut s, which) {
                edits.push(Edit::Reset(i, which));
            }
        }
    }
    edits
}

/// Which disagreement the reducer keeps.
#[derive(Clone, Copy, PartialEq)]
enum Keep {
    /// cephal differs from Chrome somewhere.
    Cephal,
    /// cephal differs from Chrome where taffy agrees with it.
    TaffyRight,
    /// taffy differs from Chrome where cephal agrees with it.
    TaffyWrong,
}

/// Renders `trees` in Chrome; per tree, whether the disagreement `keep` is there.
/// The kinds of `chrome::caveats` of a tree: the notes without the boxes they name.
fn caveat_kinds(tree: &Tree<Text>, root: cephal::tree::NodeId, viewport: Size<AvailableSpace>) -> std::collections::BTreeSet<String> {
    chrome::caveats(tree, root, viewport).iter().map(|c| c.split(' ').filter(|w| !w.starts_with('#')).collect::<Vec<_>>().join(" ")).collect()
}

fn chrome_check(
    dir: &str,
    trees: &[Tree<Text>],
    root: cephal::tree::NodeId,
    viewport: Size<AvailableSpace>,
    keep: Keep,
    allowed_caveats: &std::collections::BTreeSet<String>,
) -> Vec<bool> {
    if trees.is_empty() {
        return Vec::new();
    }
    let html = format!("{dir}/html");
    let _ = std::fs::remove_dir_all(&html);
    std::fs::create_dir_all(&html).unwrap();
    for (i, t) in trees.iter().enumerate() {
        std::fs::write(format!("{html}/c{i}.html"), chrome::to_html(t, root, viewport, &format!("candidate {i}"))).unwrap();
    }
    let xml = format!("{dir}/chrome.xml");
    let status = std::process::Command::new(format!("{}/scripts/gen_fixtures.sh", env!("CARGO_MANIFEST_DIR")))
        .args([&html, &xml])
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let xml = std::fs::read_to_string(&xml).unwrap();
    trees
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if !caveat_kinds(t, root, viewport).is_subset(allowed_caveats) {
                return false;
            }
            let Some(expected) = chrome::expectations(&xml, &format!("c{i}__border_box_ltr"), t, root, viewport) else { return false };
            let (ours, theirs) = both_layouts(t, root, viewport, true);
            expected.iter().zip(ours.iter().zip(&theirs)).any(|(e, (a, b))| {
                let Some(e) = *e else { return false };
                let (ca, tb) = (near(*a, e, 0.1), near(*b, e, 0.1));
                match keep {
                    Keep::Cephal => !ca,
                    Keep::TaffyRight => !ca && tb,
                    Keep::TaffyWrong => ca && !tb,
                }
            })
        })
        .collect()
}

/// Shrinks the tree of a seed while Chrome still disagrees with cephal (`CEPHAL_KEEP=cephal`, the
/// default), with cephal where taffy agrees with Chrome (`taffy-right`), or with taffy where cephal
/// agrees (`taffy-wrong`); writes the result to target/reduce/<seed>/reduced.html.
///
/// `CEPHAL_SEED=990 cargo test --release --test differential reduce -- --ignored --nocapture`
/// (`CEPHAL_RELAXED=1` for the relaxed tree).
#[test]
#[ignore]
fn reduce() {
    let seed: u64 = std::env::var("CEPHAL_SEED").unwrap().parse().unwrap();
    let relaxed = std::env::var_os("CEPHAL_RELAXED").is_some();
    let keep = match std::env::var("CEPHAL_KEEP").as_deref() {
        Ok("taffy-right") => Keep::TaffyRight,
        Ok("taffy-wrong") => Keep::TaffyWrong,
        _ => Keep::Cephal,
    };
    let dir = format!("{}/target/reduce/{seed}", env!("CARGO_MANIFEST_DIR"));
    let (mut tree, root, viewport, _) = expressible(seed, relaxed);
    // Candidates may not bring in what the fixtures cannot express.
    let allowed = caveat_kinds(&tree, root, viewport);
    assert!(chrome_check(&dir, &[tree.clone_cold()], root, viewport, keep, &allowed)[0], "Chrome does not show the disagreement");
    loop {
        let edits = candidate_edits(&tree, root);
        let trees: Vec<Tree<Text>> = edits.iter().map(|&e| apply_edits(&tree, root, &[e])).collect();
        let ok = chrome_check(&dir, &trees, root, viewport, keep, &allowed);
        let passing: Vec<Edit> = edits.iter().zip(&ok).filter(|(_, ok)| **ok).map(|(e, _)| *e).collect();
        if passing.is_empty() {
            break;
        }
        // Try ever shorter prefixes of all passing edits together; the single first one passes.
        let mut prefixes = Vec::new();
        let mut n = passing.len();
        while n > 1 {
            prefixes.push(n);
            n /= 2;
        }
        let combined: Vec<Tree<Text>> = prefixes.iter().map(|&n| apply_edits(&tree, root, &passing[..n])).collect();
        let ok = chrome_check(&dir, &combined, root, viewport, keep, &allowed);
        let n = prefixes.iter().zip(&ok).find(|(_, ok)| **ok).map_or(1, |(n, _)| *n);
        tree = apply_edits(&tree, root, &passing[..n]);
        let mut nodes = Vec::new();
        all_nodes(&tree, root, &mut nodes);
        println!("applied {n} of {} passing edits; {} boxes left", passing.len(), nodes.len());
    }
    let path = format!("{dir}/reduced.html");
    std::fs::write(&path, chrome::to_html(&tree, root, viewport, &format!("Reduced from random tree {seed}"))).unwrap();
    let (ours, theirs) = both_layouts(&tree, root, viewport, true);
    let mut nodes = Vec::new();
    all_nodes(&tree, root, &mut nodes);
    for (i, (a, b)) in ours.iter().zip(&theirs).enumerate() {
        println!("#{i} {:?}: cephal {a:?} taffy {b:?}", tree.style(nodes[i]).display);
    }
    println!("{path}");
}
