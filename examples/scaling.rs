//! Scaling benchmark: cold and incremental frames across tree sizes, for cephal (serial and
//! parallel) and taffy. Writes a CSV for `scripts/plot_bench.py`.
//!
//! `cargo run --release --example scaling -- target/bench/scaling.csv`
//! `CEPHAL_SIZES=1000,10000` restricts the tree sizes.

#[path = "../tests/common/mod.rs"]
mod common;

use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{Dimension, Display, FlexDirection, LengthPercentageAuto, Overflow, Position, Style};
use cephal::tree::{MeasureInput, MeasureOutput, NodeId};
use cephal::{Parallel, Tree};
use common::*;
use std::io::Write;
use std::time::{Duration, Instant};

const VIEWPORT: Size<AvailableSpace> = Size { width: AvailableSpace::Definite(1000.0), height: AvailableSpace::Definite(800.0) };

fn measure_shared(ctx: &Text, input: MeasureInput, style: &Style) -> MeasureOutput {
    let mut ctx = *ctx;
    measure(&mut ctx, input, style)
}

fn fixed_style() -> Style {
    let mut s = Style::DEFAULT;
    s.display = Display::Flex;
    s.size = Size { width: Dimension::length(10.0), height: Dimension::length(10.0) };
    s.flex_grow = 1.0;
    s
}

fn build_nested(tree: &mut Tree<Text>, b: usize, levels: usize) -> NodeId {
    let node = tree.new_leaf(fixed_style());
    if levels > 0 {
        for _ in 0..b {
            let c = build_nested(tree, b, levels - 1);
            tree.append_child(node, c);
        }
    }
    node
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

/// Mean seconds per iteration of `f`, after warm-up, over enough iterations to fill ~0.3 s.
fn time(mut f: impl FnMut()) -> f64 {
    f();
    f();
    let probe = Instant::now();
    f();
    let one = probe.elapsed().max(Duration::from_nanos(1));
    let iters = (Duration::from_millis(300).as_secs_f64() / one.as_secs_f64()).clamp(5.0, 5000.0) as usize;
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    start.elapsed().as_secs_f64() / iters as f64
}

struct Row {
    shape: &'static str,
    nodes: usize,
    scenario: &'static str,
    engine: &'static str,
    seconds: f64,
}

fn count(tree: &Tree<Text>, root: NodeId) -> usize {
    let mut n = Vec::new();
    all_nodes(tree, root, &mut n);
    n.len()
}

fn cold(rows: &mut Vec<Row>, shape: &'static str, build: impl Fn(&mut Tree<Text>) -> NodeId, executor: &Parallel<fn(&Text, MeasureInput, &Style) -> MeasureOutput>) {
    let mut tree: Tree<Text> = Tree::new();
    let root = build(&mut tree);
    let nodes = count(&tree, root);
    let serial = time(|| {
        tree.clear_layout();
        tree.compute_layout_with_measure(root, VIEWPORT, measure);
    });
    let parallel = time(|| {
        tree.clear_layout();
        tree.compute_layout_parallel(root, VIEWPORT, executor);
    });
    let (mut t, troot, map) = mirror_in_taffy(&tree, root);
    let taffy = time(|| {
        // `mark_dirty` only clears a node and its ancestors; a cold frame needs every node cleared.
        for (_, tn) in &map {
            t.mark_dirty(*tn).unwrap();
        }
        t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap();
    });
    for (engine, seconds) in [("cephal", serial), ("cephal_parallel", parallel), ("taffy", taffy)] {
        rows.push(Row { shape, nodes, scenario: "cold", engine, seconds });
    }
    eprintln!("{shape} {nodes} cold: cephal {:.3} ms, parallel {:.3} ms, taffy {:.3} ms", serial * 1e3, parallel * 1e3, taffy * 1e3);
}

fn incremental(rows: &mut Vec<Row>, target: usize, executor: &Parallel<fn(&Text, MeasureInput, &Style) -> MeasureOutput>) {
    let mut tree: Tree<Text> = Tree::new();
    let root = build_random(&mut tree, 7, target);
    let mut nodes = Vec::new();
    all_nodes(&tree, root, &mut nodes);
    // Several edit sites spread through the tree; frames rotate through them so a point averages
    // over how far each edit happens to propagate.
    const SITES: usize = 8;
    let leaves: Vec<NodeId> = nodes.iter().copied().filter(|n| tree.child_count(*n) == 0 && tree.context(*n).is_some()).collect();
    let leaves: Vec<NodeId> = (1..=SITES).map(|k| leaves[k * leaves.len() / (SITES + 1)]).collect();
    let containers: Vec<NodeId> = nodes.iter().copied().filter(|n| tree.child_count(*n) > 2 && tree.depth(*n) == 2).collect();
    let containers: Vec<NodeId> = (1..=SITES).map(|k| containers[k * containers.len() / (SITES + 1)]).collect();
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
    let n = nodes.len() + 1;
    tree.compute_layout_with_measure(root, VIEWPORT, measure);
    let (mut t, troot, map) = mirror_in_taffy(&tree, root);
    let tmap = |n: NodeId| map.iter().find(|(a, _)| *a == n).unwrap().1;
    let tleaves: Vec<taffy::NodeId> = leaves.iter().map(|n| tmap(*n)).collect();
    let tcontainers: Vec<taffy::NodeId> = containers.iter().map(|n| tmap(*n)).collect();
    let tabs = tmap(abs);
    t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap();

    let mut flip = false;
    let mut frame = 0usize;
    // Each scenario: (name, edit on cephal, edit on taffy). Frames alternate between two states
    // and rotate through the edit sites.
    type Edit<'a> = Box<dyn FnMut(&mut Tree<Text>, bool, usize) + 'a>;
    type TEdit<'a> = Box<dyn FnMut(&mut taffy::TaffyTree<Text>, bool, usize) + 'a>;
    let (leaves, tleaves, containers, tcontainers) = (&leaves, &tleaves, &containers, &tcontainers);
    let mut scenarios: Vec<(&'static str, Edit, TEdit)> = vec![
        (
            "hover_leaf_width",
            Box::new(move |tree, flip, k| tree.patch_style(leaves[k], |s| s.size.width = Dimension::length(if flip { 37.0 } else { 41.0 }))),
            Box::new(move |t, flip, k| {
                let mut s = t.style(tleaves[k]).unwrap().clone();
                s.size.width = taffy::Dimension::length(if flip { 37.0 } else { 41.0 });
                t.set_style(tleaves[k], s).unwrap();
            }),
        ),
        (
            "typing_leaf_text",
            Box::new(move |tree, flip, k| tree.set_context(leaves[k], Some(Text(if flip { 5 } else { 9 })))),
            Box::new(move |t, flip, k| t.set_node_context(tleaves[k], Some(Text(if flip { 5 } else { 9 }))).unwrap()),
        ),
        (
            "animate_absolute",
            Box::new(move |tree, flip, _| tree.patch_style(abs, |s| s.inset.left = LengthPercentageAuto::length(if flip { 10.0 } else { 20.0 }))),
            Box::new(move |t, flip, _| {
                let mut s = t.style(tabs).unwrap().clone();
                s.inset.left = taffy::LengthPercentageAuto::length(if flip { 10.0 } else { 20.0 });
                t.set_style(tabs, s).unwrap();
            }),
        ),
        (
            "toggle_display",
            Box::new(move |tree, flip, k| tree.patch_style(containers[k], |s| s.display = if flip { Display::None } else { Display::Flex })),
            Box::new(move |t, flip, k| {
                let mut s = t.style(tcontainers[k]).unwrap().clone();
                s.display = if flip { taffy::Display::None } else { taffy::Display::Flex };
                t.set_style(tcontainers[k], s).unwrap();
            }),
        ),
        ("idle_frame", Box::new(|_, _, _| {}), Box::new(|_, _, _| {})),
    ];
    for (name, edit, tedit) in scenarios.iter_mut() {
        // A site is toggled on and back off before moving to the next one.
        let step = |frame: &mut usize| {
            *frame += 1;
            (*frame % 2 == 1, (*frame / 2) % SITES)
        };
        let serial = time(|| {
            let (on, k) = step(&mut frame);
            edit(&mut tree, on, k);
            tree.compute_layout_with_measure(root, VIEWPORT, measure);
        });
        let parallel = time(|| {
            let (on, k) = step(&mut frame);
            edit(&mut tree, on, k);
            tree.compute_layout_parallel(root, VIEWPORT, executor);
        });
        let taffy = time(|| {
            let (on, k) = step(&mut frame);
            tedit(&mut t, on, k);
            t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap();
        });
        for (engine, seconds) in [("cephal", serial), ("cephal_parallel", parallel), ("taffy", taffy)] {
            rows.push(Row { shape: "random", nodes: n, scenario: name, engine, seconds });
        }
        eprintln!("random {n} {name}: cephal {:.4} ms, parallel {:.4} ms, taffy {:.4} ms", serial * 1e3, parallel * 1e3, taffy * 1e3);
    }
    // Viewport resize: the whole tree re-runs, with cached answers where nothing changed.
    let vp = |flip: bool| Size { width: AvailableSpace::Definite(if flip { 1000.0 } else { 1100.0 }), height: VIEWPORT.height };
    let serial = time(|| {
        flip = !flip;
        tree.compute_layout_with_measure(root, vp(flip), measure);
    });
    let parallel = time(|| {
        flip = !flip;
        tree.compute_layout_parallel(root, vp(flip), executor);
    });
    let taffy = time(|| {
        flip = !flip;
        t.compute_layout_with_measure(troot, to_taffy_space(vp(flip)), taffy_measure).unwrap();
    });
    for (engine, seconds) in [("cephal", serial), ("cephal_parallel", parallel), ("taffy", taffy)] {
        rows.push(Row { shape: "random", nodes: n, scenario: "resize", engine, seconds });
    }
    eprintln!("random {n} resize: cephal {:.4} ms, parallel {:.4} ms, taffy {:.4} ms", serial * 1e3, parallel * 1e3, taffy * 1e3);
}

/// A column of fixed-height rows of fixed-width text cells, rows and cells `overflow: hidden`:
/// a table, list or chat log. Edits to a cell are contained by its row, whose answer never changes.
fn build_list(tree: &mut Tree<Text>, rows: usize, cols: usize) -> (NodeId, Vec<NodeId>) {
    let clip = cephal::geometry::Point { x: Overflow::Hidden, y: Overflow::Hidden };
    let mut column = Style::DEFAULT;
    column.display = Display::Flex;
    column.flex_direction = FlexDirection::Column;
    column.size.width = Dimension::length(1000.0);
    let mut row = Style::DEFAULT;
    row.display = Display::Flex;
    row.size = Size { width: Dimension::percent(1.0), height: Dimension::length(20.0) };
    row.flex_shrink = 0.0;
    row.overflow = clip;
    let mut cell = Style::DEFAULT;
    cell.size.width = Dimension::length(40.0);
    cell.overflow = clip;
    let root = tree.new_leaf(column);
    let mut first_cells = Vec::with_capacity(rows);
    for r in 0..rows {
        let row_node = tree.new_leaf(row.clone());
        for c in 0..cols {
            let leaf = tree.new_leaf_with_context(cell.clone(), Text(3 + ((r + c) % 4) as u32));
            tree.append_child(row_node, leaf);
            if c == 0 {
                first_cells.push(leaf);
            }
        }
        tree.append_child(root, row_node);
    }
    (root, first_cells)
}

fn list(rows_out: &mut Vec<Row>, target: usize, executor: &Parallel<fn(&Text, MeasureInput, &Style) -> MeasureOutput>) {
    const COLS: usize = 9;
    const SITES: usize = 8;
    let mut tree: Tree<Text> = Tree::new();
    let (root, cells) = build_list(&mut tree, target / (COLS + 1), COLS);
    let n = count(&tree, root);
    let sites: Vec<NodeId> = (1..=SITES).map(|k| cells[k * cells.len() / (SITES + 1)]).collect();
    tree.compute_layout_with_measure(root, VIEWPORT, measure);
    let (mut t, troot, map) = mirror_in_taffy(&tree, root);
    let tsites: Vec<taffy::NodeId> = sites.iter().map(|n| map.iter().find(|(a, _)| a == n).unwrap().1).collect();
    t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap();
    let mut frame = 0usize;
    let step = |frame: &mut usize| {
        *frame += 1;
        (*frame % 2 == 1, (*frame / 2) % SITES)
    };
    type Edit<'a> = Box<dyn FnMut(&mut Tree<Text>, bool, usize) + 'a>;
    type TEdit<'a> = Box<dyn FnMut(&mut taffy::TaffyTree<Text>, bool, usize) + 'a>;
    let (sites, tsites) = (&sites, &tsites);
    let mut scenarios: Vec<(&'static str, Edit, TEdit)> = vec![
        (
            "hover_list_cell",
            Box::new(move |tree, on, k| tree.patch_style(sites[k], |s| s.size.width = Dimension::length(if on { 44.0 } else { 40.0 }))),
            Box::new(move |t, on, k| {
                let mut s = t.style(tsites[k]).unwrap().clone();
                s.size.width = taffy::Dimension::length(if on { 44.0 } else { 40.0 });
                t.set_style(tsites[k], s).unwrap();
            }),
        ),
        (
            "typing_list_cell",
            Box::new(move |tree, on, k| tree.set_context(sites[k], Some(Text(if on { 9 } else { 5 })))),
            Box::new(move |t, on, k| t.set_node_context(tsites[k], Some(Text(if on { 9 } else { 5 }))).unwrap()),
        ),
    ];
    for (name, edit, tedit) in scenarios.iter_mut() {
        let serial = time(|| {
            let (on, k) = step(&mut frame);
            edit(&mut tree, on, k);
            tree.compute_layout_with_measure(root, VIEWPORT, measure);
        });
        let parallel = time(|| {
            let (on, k) = step(&mut frame);
            edit(&mut tree, on, k);
            tree.compute_layout_parallel(root, VIEWPORT, executor);
        });
        let taffy = time(|| {
            let (on, k) = step(&mut frame);
            tedit(&mut t, on, k);
            t.compute_layout_with_measure(troot, to_taffy_space(VIEWPORT), taffy_measure).unwrap();
        });
        for (engine, seconds) in [("cephal", serial), ("cephal_parallel", parallel), ("taffy", taffy)] {
            rows_out.push(Row { shape: "list", nodes: n, scenario: name, engine, seconds });
        }
        eprintln!("list {n} {name}: cephal {:.4} ms, parallel {:.4} ms, taffy {:.4} ms", serial * 1e3, parallel * 1e3, taffy * 1e3);
    }
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "target/bench/scaling.csv".to_owned());
    let sizes: Vec<usize> = std::env::var("CEPHAL_SIZES")
        .ok()
        .map(|v| v.split(',').filter_map(|s| s.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![1_000, 3_000, 10_000, 30_000, 100_000]);
    let executor: Parallel<fn(&Text, MeasureInput, &Style) -> MeasureOutput> = Parallel::new(measure_shared);
    let mut rows = Vec::new();
    let only = std::env::var("CEPHAL_ONLY").ok();
    for &size in &sizes {
        if only.as_deref() == Some("list") {
            list(&mut rows, size, &executor);
            continue;
        }
        cold(&mut rows, "random", |t| build_random(t, 3, size), &executor);
        // Nested trees have 10^k + ... nodes; only powers of ten map to a distinct depth.
        let levels = (size as f64).log10();
        if (levels - levels.round()).abs() < 1e-9 {
            cold(&mut rows, "nested", |t| build_nested(t, 10, levels as usize), &executor);
        }
        incremental(&mut rows, size, &executor);
        list(&mut rows, size, &executor);
    }
    if let Some(dir) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    let mut f = std::fs::File::create(&out).unwrap();
    writeln!(f, "shape,nodes,scenario,engine,seconds").unwrap();
    for r in &rows {
        writeln!(f, "{},{},{},{},{:e}", r.shape, r.nodes, r.scenario, r.engine, r.seconds).unwrap();
    }
    eprintln!("wrote {out} ({} rows, {} threads)", rows.len(), executor.threads());
}
