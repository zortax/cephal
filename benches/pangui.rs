//! Cold layout of PanGui's published layout benchmarks, ported to CSS, plus grids and block flow.
//!
//! `cargo bench --bench pangui [-- <filter>]` prints the median nanoseconds per node of
//! `compute_layout` after `clear_layout` (not timed). `CEPHAL_NO_ROUNDING=1` disables pixel
//! snapping; `CEPHAL_BENCH_SECS` sets the sampling time per workload (default 1). Pin the process
//! to one performance core (`taskset -c 2`) on hybrid CPUs, or results vary by ±20 %.
//!
//! PanGui's `Expand(w)` maps to `flex: w 1 0`; its `Fit` to an auto size. CSS gives such items an
//! automatic minimum size, which PanGui does not have; the `*_min0` variants add `min-width: 0` and
//! `min-height: 0` to match PanGui's semantics.

#[path = "../tests/common/mod.rs"]
mod common;

use cephal::Tree;
use cephal::geometry::{AvailableSpace, Rect, Size};
use cephal::style::{Dimension, Display, FlexDirection, FlexWrap, LengthPercentage, LengthPercentageAuto, Style};
use cephal::tree::NodeId;
use common::{Text, build_subtree, measure, Rng};
use std::time::Instant;

fn px(v: f32) -> Dimension {
    Dimension::length(v)
}

fn flex(dir: FlexDirection) -> Style {
    let mut s = Style::DEFAULT;
    s.display = Display::Flex;
    s.flex_direction = dir;
    s
}

/// `Expand(weight)`: grows from a zero basis.
fn expand(s: &mut Style, weight: f32, min0: bool) {
    s.flex_grow = weight;
    s.flex_basis = Dimension::length(0.0);
    if min0 {
        s.min_size = Size { width: LengthPercentageAuto::length(0.0), height: LengthPercentageAuto::length(0.0) };
    }
}

fn add(t: &mut Tree<Text>, parent: NodeId, style: &Style) -> NodeId {
    let n = t.new_leaf(style.clone());
    t.append_child(parent, n);
    n
}

fn build(name: &str, t: &mut Tree<Text>) -> NodeId {
    use FlexDirection::{Column, Row};
    let min0 = name.ends_with("_min0");
    let base = name.trim_end_matches("_min0");
    match base {
        "wide_no_wrap_simple_few" | "wide_no_wrap_simple_many" => {
            let n = if base.ends_with("many") { 100_000 } else { 1_000 };
            let mut r = flex(Row);
            r.size = Size { width: px(100.0), height: px(100.0) };
            let root = t.new_leaf(r);
            let mut c = Style::DEFAULT;
            c.size = Size { width: px(10.0), height: px(10.0) };
            for _ in 0..n {
                add(t, root, &c);
            }
            root
        }
        "flex_expand_equal_weights" => {
            let mut r = flex(Row);
            r.size = Size { width: px(10_000.0), height: px(100.0) };
            let root = t.new_leaf(r);
            let mut c = Style::DEFAULT;
            expand(&mut c, 1.0, min0);
            for _ in 0..15_000 {
                add(t, root, &c);
            }
            root
        }
        "flex_expand_weights" => {
            let mut r = flex(Row);
            r.size = Size { width: px(10_000.0), height: px(100.0) };
            let root = t.new_leaf(r);
            let mut m = flex(Row);
            m.size = Size { width: px(100.0), height: px(100.0) };
            let (mut a, mut b) = (Style::DEFAULT, Style::DEFAULT);
            expand(&mut a, 1.0, min0);
            expand(&mut b, 2.0, min0);
            for _ in 0..5_000 {
                let mm = add(t, root, &m);
                add(t, mm, &a);
                add(t, mm, &b);
            }
            root
        }
        "nested_vertical_stack" => {
            let mut r = flex(Column);
            r.size.width = px(200.0);
            r.padding = Rect::splat(LengthPercentage::length(10.0));
            r.gap = Size { width: LengthPercentage::length(5.0), height: LengthPercentage::length(5.0) };
            let root = t.new_leaf(r);
            let mut c = Style::DEFAULT;
            c.size.height = px(1.0);
            for _ in 0..10_000 {
                add(t, root, &c);
            }
            root
        }
        "percentage_and_ratio" => {
            let mut r = flex(Column);
            r.size = Size { width: px(1000.0), height: px(1000.0) };
            let root = t.new_leaf(r);
            let mut c = Style::DEFAULT;
            c.size.width = Dimension::percent(0.5);
            c.aspect_ratio = Some(2.0);
            c.flex_shrink = 0.0;
            for _ in 0..10_000 {
                add(t, root, &c);
            }
            root
        }
        "expand_with_max_constraint" | "expand_with_min_constraint" => {
            let mut r = flex(Row);
            r.size.width = px(100.0);
            let root = t.new_leaf(r);
            let mut m = flex(Row);
            m.size.width = px(100.0);
            let mut a = Style::DEFAULT;
            expand(&mut a, 1.0, min0);
            a.size.height = px(10.0);
            let mut b = a.clone();
            if base.contains("max") {
                b.max_size.width = LengthPercentageAuto::length(40.0);
            } else {
                b.min_size.width = LengthPercentageAuto::length(60.0);
            }
            for _ in 0..1_000 {
                let mm = add(t, root, &m);
                add(t, mm, &a);
                add(t, mm, &b);
            }
            root
        }
        "fit_nesting" => {
            let mut r = flex(Column);
            r.size.width = px(1000.0);
            r.padding = Rect::splat(LengthPercentage::length(5.0));
            let root = t.new_leaf(r);
            let row = flex(Row);
            let mut col = flex(Column);
            expand(&mut col, 1.0, min0);
            let row2 = flex(Row);
            let mut leaf = Style::DEFAULT;
            expand(&mut leaf, 1.0, min0);
            leaf.size.height = px(10.0);
            for _ in 0..10 {
                let a = add(t, root, &row);
                for _ in 0..10 {
                    let b = add(t, a, &col);
                    for _ in 0..10 {
                        let c = add(t, b, &row2);
                        for _ in 0..100 {
                            add(t, c, &leaf);
                        }
                    }
                }
            }
            root
        }
        "wide_wrapping" => {
            let mut r = flex(Row);
            r.size.width = px(100.0);
            r.flex_wrap = FlexWrap::Wrap;
            let root = t.new_leaf(r);
            let mut c = Style::DEFAULT;
            c.size = Size { width: px(10.0), height: px(10.0) };
            c.flex_shrink = 0.0;
            for _ in 0..10_000 {
                add(t, root, &c);
            }
            root
        }
        "perpendicular_expand_with_wrap" => {
            let mut r = flex(Row);
            r.size.width = px(100.0);
            r.flex_wrap = FlexWrap::Wrap;
            let root = t.new_leaf(r);
            let mut thin = Style::DEFAULT;
            thin.size.width = px(1.0);
            thin.flex_shrink = 0.0;
            for _ in 0..1_000 {
                for s in [10.0, 20.0, 30.0, 40.0, 50.0, 60.0] {
                    let mut c = Style::DEFAULT;
                    c.size = Size { width: px(s), height: px(s) };
                    c.flex_shrink = 0.0;
                    add(t, root, &c);
                    add(t, root, &thin);
                }
            }
            root
        }
        "padding_and_margin" => {
            let mut r = flex(Column);
            r.size.width = px(1000.0);
            r.padding = Rect::splat(LengthPercentage::length(20.0));
            r.gap = Size { width: LengthPercentage::length(10.0), height: LengthPercentage::length(10.0) };
            let root = t.new_leaf(r);
            let mut c = Style::DEFAULT;
            c.size.height = px(20.0);
            c.margin = Rect::splat(LengthPercentageAuto::length(2.0));
            for _ in 0..100 {
                add(t, root, &c);
            }
            root
        }
        "grid_mixed" | "grid_auto" | "grid_fixed" => {
            use cephal::style::{GridTemplateComponent, TrackSizingFunction};
            let n = 100;
            let mut s = Style::DEFAULT;
            s.display = Display::Grid;
            let g = s.grid_mut();
            let column = |i: usize| match base {
                "grid_mixed" if i % 3 == 0 => TrackSizingFunction::fr(1.0),
                "grid_fixed" => TrackSizingFunction::length(10.0),
                _ => TrackSizingFunction::AUTO,
            };
            g.template_columns = (0..n).map(|i| GridTemplateComponent::Single(column(i))).collect();
            let row = if base == "grid_fixed" { TrackSizingFunction::length(10.0) } else { TrackSizingFunction::AUTO };
            g.template_rows = (0..n).map(|_| GridTemplateComponent::Single(row)).collect();
            let root = t.new_leaf(s);
            for i in 0..n * n {
                let mut c = Style::DEFAULT;
                c.size = Size { width: px(10.0 + (i % 7) as f32), height: px(10.0) };
                add(t, root, &c);
            }
            root
        }
        "block_stack" => {
            let mut r = Style::DEFAULT;
            r.size.width = px(1000.0);
            let root = t.new_leaf(r);
            let mut c = Style::DEFAULT;
            c.size.height = px(10.0);
            for _ in 0..10_000 {
                add(t, root, &c);
            }
            root
        }
        "text_row" => {
            let mut r = flex(Row);
            r.size.width = px(1000.0);
            r.flex_wrap = FlexWrap::Wrap;
            let root = t.new_leaf(r);
            for i in 0..10_000 {
                let c = t.new_leaf_with_context(Style::DEFAULT, Text(3 + (i % 5) as u32));
                t.append_child(root, c);
            }
            root
        }
        "random_10k" => {
            let mut rng = Rng(3);
            let mut r = Style::DEFAULT;
            r.display = Display::Flex;
            let root = t.new_leaf(r);
            while t.node_count() < 10_000 {
                let sub = build_subtree(&mut rng, t, 1);
                t.append_child(root, sub);
            }
            root
        }
        _ => panic!("unknown workload {name}"),
    }
}

const WORKLOADS: &[&str] = &[
    "wide_no_wrap_simple_few",
    "wide_no_wrap_simple_many",
    "flex_expand_equal_weights",
    "flex_expand_equal_weights_min0",
    "flex_expand_weights",
    "flex_expand_weights_min0",
    "nested_vertical_stack",
    "percentage_and_ratio",
    "expand_with_max_constraint",
    "expand_with_max_constraint_min0",
    "expand_with_min_constraint",
    "fit_nesting",
    "fit_nesting_min0",
    "wide_wrapping",
    "perpendicular_expand_with_wrap",
    "padding_and_margin",
    "grid_mixed",
    "grid_auto",
    "grid_fixed",
    "block_stack",
    "text_row",
    "random_10k",
];

fn main() {
    let filter: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect();
    let secs: f64 = std::env::var("CEPHAL_BENCH_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let rounding = std::env::var_os("CEPHAL_NO_ROUNDING").is_none();
    let viewport = Size { width: AvailableSpace::MaxContent, height: AvailableSpace::MaxContent };
    for &name in WORKLOADS {
        if !filter.is_empty() && !filter.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        let mut tree: Tree<Text> = Tree::new();
        if !rounding {
            tree.disable_rounding();
        }
        let root = build(name, &mut tree);
        let nodes = tree.node_count();
        let run = |tree: &mut Tree<Text>| {
            tree.clear_layout();
            let start = Instant::now();
            tree.compute_layout_with_measure(root, viewport, measure);
            start.elapsed().as_nanos() as f64
        };
        for _ in 0..5 {
            run(&mut tree);
        }
        let mut samples = Vec::new();
        let begin = Instant::now();
        while begin.elapsed().as_secs_f64() < secs || samples.len() < 10 {
            samples.push(run(&mut tree));
        }
        samples.sort_by(f64::total_cmp);
        let median = samples[samples.len() / 2];
        println!(
            "{name:34} {nodes:>7} nodes {:>10.3} ms {:>8.1} ns/node",
            median / 1e6,
            median / nodes as f64
        );
    }
}
