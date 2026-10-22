//! The comparative benchmark suite: cephal (serial and parallel) against taffy on the workloads of
//! `benches/workloads`, cold, incremental and one-shot, with every engine's layout checked against
//! cephal's.
//!
//! `cargo bench --bench suite [-- <filter>...] [--engines cephal,cephal_parallel,taffy]
//! [--modes cold,incremental,build] [--sizes 1000,10000] [--tag grid] [--quick] [--list]`
//!
//! Filters match workload names by substring, or whole names with `--exact`. Pin the process to
//! one performance core (`taskset -c 2`) on hybrid CPUs, or timings vary by ±20 %. The parallel
//! engine uses the cores the process may run on, so time it on the performance cores
//! (`taskset -c 0-15 ... --engines cephal_parallel`). Environment:
//!
//! - `CEPHAL_BENCH_SECS`: sampling time per measurement (default 1, `--quick` 0.2);
//! - `CEPHAL_BENCH_READ=1`: also read every node's layout inside the timed region;
//! - `CEPHAL_NO_ROUNDING=1`: no pixel snapping, in either engine;
//! - `CEPHAL_BENCH_CSV=path`: the CSV (default `target/bench/suite.csv`), with `meta.json` beside it;
//! - `CEPHAL_BENCH_ITERS=n`: run n cold cephal layouts per workload (each followed by the reads,
//!   under `CEPHAL_BENCH_READ`) and print nothing else, for `scripts/instructions.sh`;
//! - `CEPHAL_BENCH_DUMP=dir`: write each workload's tree, cephal's layout and the incremental edits
//!   for the external engines (`benches/external`), without timing anything.
//!
//! Cold layout is `clear_layout` (cephal) or marking every node dirty (taffy), untimed, then one
//! timed layout. Incremental frames time the edit and the layout. Build times creating the tree from
//! prepared styles plus its first layout.
#![cfg_attr(feature = "stats", allow(unused))]

#[path = "../tests/common/mod.rs"]
mod common;
mod workloads;

use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{AlignItems, Display, LengthPercentageAuto, Style};
use cephal::tree::{MeasureInput, MeasureOutput, NodeId};
use cephal::{Parallel, Tree};
use common::{Text, all_nodes, measure, mirror_in_taffy, taffy_measure, to_taffy_space};
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use workloads::{Scenario, WORKLOADS, Workload};

type Executor = Parallel<fn(&Text, MeasureInput, &Style) -> MeasureOutput>;

fn measure_shared(ctx: &Text, input: MeasureInput, style: &Style) -> MeasureOutput {
    let mut ctx = *ctx;
    measure(&mut ctx, input, style)
}

/// Measure calls of the counting runs, which are never timed.
static MEASURES: AtomicU64 = AtomicU64::new(0);

fn measure_counted(ctx: &mut Text, input: MeasureInput, style: &Style) -> MeasureOutput {
    MEASURES.fetch_add(1, Ordering::Relaxed);
    measure(ctx, input, style)
}

fn measure_shared_counted(ctx: &Text, input: MeasureInput, style: &Style) -> MeasureOutput {
    MEASURES.fetch_add(1, Ordering::Relaxed);
    measure_shared(ctx, input, style)
}

fn taffy_measure_counted(inputs: taffy::LayoutInput, node: taffy::NodeId, context: Option<&mut Text>, style: &taffy::Style) -> taffy::LayoutOutput {
    // Counts what reaches the content, as the cephal counters do: leaves with text, not empty ones.
    let counted = context.is_some() && !matches!(inputs.known_dimensions, taffy::Size { width: Some(_), height: Some(_) });
    if counted {
        MEASURES.fetch_add(1, Ordering::Relaxed);
    }
    taffy_measure(inputs, node, context, style)
}

struct Options {
    filter: Vec<String>,
    tags: Vec<String>,
    engines: Vec<String>,
    modes: Vec<String>,
    sizes: Option<Vec<usize>>,
    secs: f64,
    read: bool,
    rounding: bool,
}

struct Stats {
    median: f64,
    p10: f64,
    p90: f64,
    samples: usize,
}

/// Nanoseconds of `run` (one iteration, returning its own timed nanoseconds) over at least `secs`
/// seconds and 10 samples, after warm-up.
fn sample(secs: f64, mut run: impl FnMut() -> f64) -> Stats {
    for _ in 0..3 {
        run();
    }
    let mut v = Vec::new();
    let begin = Instant::now();
    while (begin.elapsed().as_secs_f64() < secs || v.len() < 10) && v.len() < 1_000_000 {
        v.push(run());
    }
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    Stats { median: at(0.5), p10: at(0.1), p90: at(0.9), samples: v.len() }
}

struct Csv {
    file: Option<std::fs::File>,
}

impl Csv {
    const HEADER: &'static str = "workload,family,tags,size,nodes,engine,mode,scenario,ns_per_node,median_ms,p10_ms,p90_ms,samples,agree,max_err,measure_calls";

    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, w: &Workload, size: usize, nodes: usize, engine: &str, mode: &str, scenario: &str, s: &Stats, agree: &str, max_err: f32, measures: Option<u64>) {
        let line = format!(
            "{},{},{},{size},{nodes},{engine},{mode},{scenario},{:.3},{:.6},{:.6},{:.6},{},{agree},{max_err:e},{}",
            w.name,
            w.family,
            w.tags.join("|"),
            s.median / nodes as f64,
            s.median / 1e6,
            s.p10 / 1e6,
            s.p90 / 1e6,
            s.samples,
            measures.map_or(String::new(), |m| m.to_string()),
        );
        if let Some(f) = &mut self.file {
            writeln!(f, "{line}").unwrap();
        }
    }
}

fn viewport(w: &Workload) -> Size<AvailableSpace> {
    let space = |v: Option<f32>| v.map_or(AvailableSpace::MaxContent, AvailableSpace::Definite);
    Size { width: space(w.viewport.0), height: space(w.viewport.1) }
}

/// Every node's unrounded border box in tree order.
type Boxes = Vec<[f32; 4]>;

fn cephal_boxes(t: &Tree<Text>, nodes: &[NodeId]) -> Boxes {
    nodes.iter().map(|&n| {
        let l = t.unrounded_layout(n);
        [l.location.x, l.location.y, l.size.width, l.size.height]
    }).collect()
}

fn taffy_boxes(t: &taffy::TaffyTree<Text>, nodes: &[taffy::NodeId]) -> Boxes {
    nodes.iter().map(|&n| {
        let l = t.unrounded_layout(n);
        [l.location.x, l.location.y, l.size.width, l.size.height]
    }).collect()
}

/// `exact` (same bits), `close` (within 0.01 px relative to magnitude) or `differs`, and the largest
/// absolute difference.
fn agreement(a: &Boxes, b: &Boxes) -> (&'static str, f32) {
    let mut exact = true;
    let mut close = true;
    let mut max_err = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        for (p, q) in x.iter().zip(y) {
            exact &= p.to_bits() == q.to_bits();
            let d = (p - q).abs();
            max_err = max_err.max(d);
            close &= d <= 0.01 * (1.0 + p.abs().max(q.abs()));
        }
    }
    (if exact { "exact" } else if close { "close" } else { "differs" }, max_err)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut o = Options {
        filter: Vec::new(),
        tags: Vec::new(),
        engines: vec!["cephal".into(), "cephal_parallel".into(), "taffy".into()],
        modes: vec!["cold".into(), "incremental".into()],
        sizes: None,
        secs: std::env::var("CEPHAL_BENCH_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0),
        read: std::env::var_os("CEPHAL_BENCH_READ").is_some(),
        rounding: std::env::var_os("CEPHAL_NO_ROUNDING").is_none(),
    };
    let mut list = false;
    let mut exact = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().unwrap_or_default();
        match a.as_str() {
            "--engines" => o.engines = value().split(',').map(str::to_owned).collect(),
            "--modes" => o.modes = value().split(',').map(str::to_owned).collect(),
            "--sizes" => o.sizes = Some(value().split(',').filter_map(|s| s.parse().ok()).collect()),
            "--tag" => o.tags.push(value()),
            "--quick" => o.secs = o.secs.min(0.2),
            "--exact" => exact = true,
            "--list" => list = true,
            "--bench" | "--taffy" => {}
            s if s.starts_with("--") => panic!("unknown option {s}"),
            s => o.filter.push(s.to_owned()),
        }
    }
    let selected: Vec<&Workload> = WORKLOADS
        .iter()
        .filter(|w| o.filter.is_empty() || o.filter.iter().any(|f| if exact { w.name == f } else { w.name.contains(f.as_str()) }))
        .filter(|w| o.tags.iter().all(|t| w.tags.iter().any(|wt| wt == t || wt.starts_with(&format!("{t}:")))))
        .collect();
    if list {
        for w in &selected {
            println!("{:34} {:28} sizes {:?} scenarios {:?}", w.name, w.tags.join(" "), w.sizes, w.scenarios.iter().map(|s| s.name()).collect::<Vec<_>>());
        }
        return;
    }
    let sizes_of = |w: &Workload| o.sizes.clone().unwrap_or_else(|| w.sizes.to_vec());

    if let Ok(dir) = std::env::var("CEPHAL_BENCH_DUMP") {
        std::fs::create_dir_all(&dir).unwrap();
        for w in &selected {
            for size in sizes_of(w) {
                dump(w, size, std::path::Path::new(&dir));
            }
        }
        return;
    }
    if let Ok(iters) = std::env::var("CEPHAL_BENCH_ITERS") {
        let iters: usize = iters.parse().unwrap();
        for w in &selected {
            for size in sizes_of(w) {
                let mut tree: Tree<Text> = Tree::new();
                if !o.rounding {
                    tree.disable_rounding();
                }
                tree.set_measure_ignores_available_height(true);
                let root = (w.build)(&mut tree, size);
                let vp = viewport(w);
                // Under `CEPHAL_BENCH_READ`, every node's layout is read after each layout too.
                let mut order = Vec::new();
                if o.read {
                    all_nodes(&tree, root, &mut order);
                }
                for _ in 0..iters {
                    tree.clear_layout();
                    tree.compute_layout_with_measure(root, vp, measure);
                    let mut sum = 0.0;
                    for &n in &order {
                        sum += tree.layout(n).location.x;
                    }
                    std::hint::black_box(sum);
                }
                println!("{}@{size} {}", w.name, tree.node_count());
            }
        }
        return;
    }

    let csv_path = std::env::var("CEPHAL_BENCH_CSV").unwrap_or_else(|_| "target/bench/suite.csv".into());
    let csv_path = std::path::PathBuf::from(csv_path);
    if let Some(dir) = csv_path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).unwrap();
    }
    let mut file = std::fs::File::create(&csv_path).unwrap();
    writeln!(file, "{}", Csv::HEADER).unwrap();
    let mut csv = Csv { file: Some(file) };
    let executor: Executor = Parallel::new(measure_shared);
    write_meta(&csv_path.with_file_name(format!("{}.meta.json", csv_path.file_stem().unwrap().to_string_lossy())), &o, executor.threads());

    let counting: Executor = Parallel::new(measure_shared_counted);
    for w in &selected {
        for size in sizes_of(w) {
            if o.modes.iter().any(|m| m == "cold") {
                cold(w, size, &o, &executor, &counting, &mut csv);
            }
            if o.modes.iter().any(|m| m == "build") {
                build(w, size, &o, &mut csv);
            }
            if o.modes.iter().any(|m| m == "incremental") {
                for &scenario in w.scenarios {
                    incremental(w, size, scenario, &o, &executor, &mut csv);
                }
            }
        }
    }
    eprintln!("wrote {}", csv_path.display());
}

fn new_tree(w: &Workload, size: usize, rounding: bool) -> (Tree<Text>, NodeId) {
    let mut tree: Tree<Text> = Tree::new();
    if !rounding {
        tree.disable_rounding();
    }
    // The text measurement wraps at the available width and never reads the height.
    tree.set_measure_ignores_available_height(true);
    let root = (w.build)(&mut tree, size);
    (tree, root)
}

fn print(w: &Workload, size: usize, nodes: usize, engine: &str, what: &str, s: &Stats, note: &str) {
    println!(
        "{:34} {:>7} {:>7} {:16} {:15} {:>10.4} ms {:>9.1} ns/node  [{:.4}..{:.4}] {note}",
        w.name,
        size,
        nodes,
        engine,
        what,
        s.median / 1e6,
        s.median / nodes as f64,
        s.p10 / 1e6,
        s.p90 / 1e6
    );
}

fn cold(w: &Workload, size: usize, o: &Options, executor: &Executor, counting: &Executor, csv: &mut Csv) {
    let (mut tree, root) = new_tree(w, size, o.rounding);
    let nodes = tree.node_count();
    let vp = viewport(w);
    let mut order = Vec::new();
    all_nodes(&tree, root, &mut order);

    #[cfg(feature = "stats")]
    {
        tree.clear_layout();
        let _ = cephal::compute::stats::report();
        tree.compute_layout_with_measure(root, vp, measure);
        let report = cephal::compute::stats::report();
        println!("{:34} {size:>7} {report}", w.name);
        // `CEPHAL_BENCH_STATS` (default target/bench/stats.csv): one row of counters per workload.
        let path = std::env::var("CEPHAL_BENCH_STATS").unwrap_or_else(|_| "target/bench/stats.csv".into());
        let pairs: Vec<(&str, &str)> = report.split(' ').filter_map(|kv| kv.split_once('=')).collect();
        let fresh = !std::path::Path::new(&path).exists();
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path).unwrap();
        if fresh {
            writeln!(f, "workload,size,nodes,{}", pairs.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(",")).unwrap();
        }
        writeln!(f, "{},{size},{nodes},{}", w.name, pairs.iter().map(|(_, v)| *v).collect::<Vec<_>>().join(",")).unwrap();
        return;
    }

    let read = |tree: &Tree<Text>| {
        let mut sum = 0.0;
        for &n in &order {
            sum += tree.layout(n).location.x;
        }
        std::hint::black_box(sum);
    };
    tree.compute_layout_with_measure(root, vp, measure);
    let reference = cephal_boxes(&tree, &order);
    for engine in &o.engines {
        match engine.as_str() {
            "cephal" => {
                let s = sample(o.secs, || {
                    tree.clear_layout();
                    let start = Instant::now();
                    tree.compute_layout_with_measure(root, vp, measure);
                    if o.read {
                        read(&tree);
                    }
                    start.elapsed().as_nanos() as f64
                });
                tree.clear_layout();
                MEASURES.store(0, Ordering::Relaxed);
                tree.compute_layout_with_measure(root, vp, measure_counted);
                let m = MEASURES.load(Ordering::Relaxed);
                print(w, size, nodes, engine, "cold", &s, &format!("{m} measures"));
                csv.row(w, size, nodes, engine, "cold", "-", &s, "exact", 0.0, Some(m));
            }
            "cephal_parallel" => {
                let s = sample(o.secs, || {
                    tree.clear_layout();
                    let start = Instant::now();
                    tree.compute_layout_parallel(root, vp, executor);
                    if o.read {
                        read(&tree);
                    }
                    start.elapsed().as_nanos() as f64
                });
                let (agree, err) = agreement(&reference, &cephal_boxes(&tree, &order));
                tree.clear_layout();
                MEASURES.store(0, Ordering::Relaxed);
                tree.compute_layout_parallel(root, vp, counting);
                let m = MEASURES.load(Ordering::Relaxed);
                print(w, size, nodes, engine, "cold", &s, &format!("{m} measures, {agree}"));
                csv.row(w, size, nodes, engine, "cold", "-", &s, agree, err, Some(m));
            }
            "taffy" => {
                let (mut t, troot, map) = mirror_in_taffy(&tree, root);
                if !o.rounding {
                    t.disable_rounding();
                }
                let tnodes: Vec<taffy::NodeId> = map.iter().map(|&(_, n)| n).collect();
                let space = to_taffy_space(vp);
                let s = sample(o.secs, || {
                    // taffy has no `clear_layout`: marking every node dirty (untimed) empties every cache.
                    for &n in &tnodes {
                        t.mark_dirty(n).unwrap();
                    }
                    let start = Instant::now();
                    t.compute_layout_with_measure(troot, space, taffy_measure).unwrap();
                    if o.read {
                        let mut sum = 0.0;
                        for &n in &tnodes {
                            sum += t.layout(n).unwrap().location.x;
                        }
                        std::hint::black_box(sum);
                    }
                    start.elapsed().as_nanos() as f64
                });
                let (agree, err) = agreement(&reference, &taffy_boxes(&t, &tnodes));
                for &n in &tnodes {
                    t.mark_dirty(n).unwrap();
                }
                MEASURES.store(0, Ordering::Relaxed);
                t.compute_layout_with_measure(troot, space, taffy_measure_counted).unwrap();
                let m = MEASURES.load(Ordering::Relaxed);
                print(w, size, nodes, engine, "cold", &s, &format!("{m} measures, {agree} (max {err:.3})"));
                csv.row(w, size, nodes, engine, "cold", "-", &s, agree, err, Some(m));
            }
            other => panic!("unknown engine {other}"),
        }
    }
}

/// One node of a tree prepared for building: parent index in pre-order, style, text.
type Blueprint = Vec<(Option<usize>, Style, Option<Text>)>;

fn blueprint(tree: &Tree<Text>, root: NodeId) -> Blueprint {
    let mut order = Vec::new();
    all_nodes(tree, root, &mut order);
    let index: HashMap<NodeId, usize> = order.iter().enumerate().map(|(i, &n)| (n, i)).collect();
    order.iter().map(|&n| (tree.parent(n).map(|p| index[&p]), tree.style(n).clone(), tree.context(n).copied())).collect()
}

fn build(w: &Workload, size: usize, o: &Options, csv: &mut Csv) {
    let (tree, root) = new_tree(w, size, o.rounding);
    let nodes = tree.node_count();
    let vp = viewport(w);
    let bp = blueprint(&tree, root);
    let names = |i| tree.ident_name(i).to_owned();
    let tbp: Vec<(Option<usize>, taffy::Style, Option<Text>)> =
        bp.iter().map(|(p, s, c)| (*p, cephal::compat::taffy::to_taffy(s, &names), *c)).collect();
    for engine in &o.engines {
        let s = match engine.as_str() {
            "cephal" => sample(o.secs, || {
                // Grid line names are idents of the source tree's interner; no workload names lines,
                // so a fresh tree reads the styles alike.
                let mut styles: Vec<Style> = bp.iter().map(|(_, s, _)| s.clone()).collect();
                let start = Instant::now();
                let mut t: Tree<Text> = Tree::with_capacity(bp.len());
                if !o.rounding {
                    t.disable_rounding();
                }
                t.set_measure_ignores_available_height(true);
                let mut ids = Vec::with_capacity(bp.len());
                for (i, (parent, _, ctx)) in bp.iter().enumerate() {
                    let style = std::mem::take(&mut styles[i]);
                    let n = match ctx {
                        Some(c) => t.new_leaf_with_context(style, *c),
                        None => t.new_leaf(style),
                    };
                    if let Some(p) = parent {
                        t.append_child(ids[*p], n);
                    }
                    ids.push(n);
                }
                t.compute_layout_with_measure(ids[0], vp, measure);
                let ns = start.elapsed().as_nanos() as f64;
                drop(t);
                ns
            }),
            "taffy" => sample(o.secs, || {
                let mut styles: Vec<taffy::Style> = tbp.iter().map(|(_, s, _)| s.clone()).collect();
                let start = Instant::now();
                let mut t: taffy::TaffyTree<Text> = taffy::TaffyTree::with_capacity(tbp.len());
                if !o.rounding {
                    t.disable_rounding();
                }
                let mut ids = Vec::with_capacity(tbp.len());
                for (i, (parent, _, ctx)) in tbp.iter().enumerate() {
                    let style = std::mem::take(&mut styles[i]);
                    let n = match ctx {
                        Some(c) => t.new_leaf_with_context(style, *c).unwrap(),
                        None => t.new_leaf(style).unwrap(),
                    };
                    if let Some(p) = parent {
                        t.add_child(ids[*p], n).unwrap();
                    }
                    ids.push(n);
                }
                t.compute_layout_with_measure(ids[0], to_taffy_space(vp), taffy_measure).unwrap();
                let ns = start.elapsed().as_nanos() as f64;
                drop(t);
                ns
            }),
            _ => continue,
        };
        print(w, size, nodes, engine, "build", &s, "");
        csv.row(w, size, nodes, engine, "build", "-", &s, "-", 0.0, None);
    }
}

/// The edit sites of a scenario: nodes of the cephal tree (an added absolute box for
/// `MoveAbsolute`), chosen before any engine mirrors the tree.
fn sites(tree: &mut Tree<Text>, root: NodeId, scenario: Scenario) -> Vec<NodeId> {
    const SITES: usize = 8;
    match scenario {
        Scenario::EditText | Scenario::HoverWidth | Scenario::AlignSelf => workloads::spread(&workloads::text_leaves(tree, root), SITES),
        // Insets move only relatively positioned boxes; a static site would make the edit a no-op.
        Scenario::RelativeInset => {
            let sites = workloads::spread(&workloads::text_leaves(tree, root), SITES);
            for &n in &sites {
                tree.patch_style(n, |s| s.position = cephal::style::Position::Relative);
            }
            sites
        }
        Scenario::ToggleDisplay | Scenario::InsertRemove => workloads::containers(tree, root, SITES),
        Scenario::MoveAbsolute => {
            let s = workloads::css(tree, "position: absolute; width: 50px; height: 50px; left: 10px; top: 10px");
            let n = tree.new_leaf(s);
            tree.append_child(root, n);
            vec![n]
        }
        Scenario::ResizeRoot | Scenario::Idle => vec![root],
    }
}

/// Applies state `on` of `scenario` at site `n` of the cephal tree; `added` holds inserted rows.
#[allow(clippy::too_many_arguments)]
fn edit_cephal(t: &mut Tree<Text>, scenario: Scenario, n: NodeId, on: bool, original: &Style, base_text: u32, added: &mut HashMap<NodeId, NodeId>) {
    match scenario {
        Scenario::EditText => t.set_context(n, Some(Text(if on { base_text + 4 } else { base_text }))),
        Scenario::HoverWidth => t.patch_style(n, |s| s.size.width = if on { cephal::style::Dimension::length(41.0) } else { original.size.width }),
        Scenario::ToggleDisplay => t.patch_style(n, |s| s.display = if on { Display::None } else { original.display }),
        Scenario::AlignSelf => t.patch_style(n, |s| s.align_self = if on { Some(AlignItems::CENTER) } else { original.align_self }),
        Scenario::RelativeInset => t.patch_style(n, |s| s.inset.left = if on { LengthPercentageAuto::length(5.0) } else { original.inset.left }),
        Scenario::MoveAbsolute => t.patch_style(n, |s| s.inset.left = LengthPercentageAuto::length(if on { 20.0 } else { 10.0 })),
        Scenario::InsertRemove => {
            if on {
                let c = t.new_leaf_with_context(Style::DEFAULT, Text(5));
                t.append_child(n, c);
                added.insert(n, c);
            } else if let Some(c) = added.remove(&n) {
                t.remove(c);
            }
        }
        Scenario::ResizeRoot | Scenario::Idle => {}
    }
}

fn edit_taffy(t: &mut taffy::TaffyTree<Text>, scenario: Scenario, n: taffy::NodeId, on: bool, original: &taffy::Style, base_text: u32, added: &mut HashMap<taffy::NodeId, taffy::NodeId>) {
    let mut set = |f: &dyn Fn(&mut taffy::Style)| {
        let mut s = t.style(n).unwrap().clone();
        f(&mut s);
        t.set_style(n, s).unwrap();
    };
    match scenario {
        Scenario::EditText => t.set_node_context(n, Some(Text(if on { base_text + 4 } else { base_text }))).unwrap(),
        Scenario::HoverWidth => set(&|s| s.size.width = if on { taffy::Dimension::length(41.0) } else { original.size.width }),
        Scenario::ToggleDisplay => set(&|s| s.display = if on { taffy::Display::None } else { original.display }),
        Scenario::AlignSelf => set(&|s| s.align_self = if on { Some(taffy::AlignItems::CENTER) } else { original.align_self }),
        Scenario::RelativeInset => set(&|s| s.inset.left = if on { taffy::LengthPercentageAuto::length(5.0) } else { original.inset.left }),
        Scenario::MoveAbsolute => set(&|s| s.inset.left = taffy::LengthPercentageAuto::length(if on { 20.0 } else { 10.0 })),
        Scenario::InsertRemove => {
            if on {
                let c = t.new_leaf_with_context(taffy::Style::DEFAULT, Text(5)).unwrap();
                let mut s = t.style(c).unwrap().clone();
                s.position = taffy::Position::Relative;
                t.set_style(c, s).unwrap();
                t.add_child(n, c).unwrap();
                added.insert(n, c);
            } else if let Some(c) = added.remove(&n) {
                t.remove(c).unwrap();
            }
        }
        Scenario::ResizeRoot | Scenario::Idle => {}
    }
}

fn incremental(w: &Workload, size: usize, scenario: Scenario, o: &Options, executor: &Executor, csv: &mut Csv) {
    for engine in &o.engines {
        let (mut tree, root) = new_tree(w, size, o.rounding);
        let sites = sites(&mut tree, root, scenario);
        if sites.is_empty() {
            continue;
        }
        let nodes = tree.node_count();
        let base = viewport(w);
        let vp = move |on: bool| match scenario {
            Scenario::ResizeRoot => Size {
                width: AvailableSpace::Definite(w.viewport.0.unwrap_or(1000.0) + if on { 100.0 } else { 0.0 }),
                height: base.height,
            },
            _ => base,
        };
        let mut frame = 0usize;
        // A site is toggled on and back off before moving to the next one.
        let count = sites.len();
        let mut step = move || {
            frame += 1;
            (frame % 2 == 1, ((frame - 1) / 2) % count)
        };
        let sites_c = sites;
        let s = match engine.as_str() {
            "cephal" | "cephal_parallel" => {
                let parallel = engine == "cephal_parallel";
                let originals: Vec<Style> = sites_c.iter().map(|&n| tree.style(n).clone()).collect();
                let texts: Vec<u32> = sites_c.iter().map(|&n| tree.context(n).map_or(0, |c| c.0)).collect();
                let mut added = HashMap::new();
                let layout = |tree: &mut Tree<Text>, on: bool| {
                    if parallel {
                        tree.compute_layout_parallel(root, vp(on), executor);
                    } else {
                        tree.compute_layout_with_measure(root, vp(on), measure);
                    }
                };
                layout(&mut tree, false);
                let mut order = Vec::new();
                all_nodes(&tree, root, &mut order);
                sample(o.secs, || {
                    let (on, k) = step();
                    let start = Instant::now();
                    edit_cephal(&mut tree, scenario, sites_c[k], on, &originals[k], texts[k], &mut added);
                    layout(&mut tree, on);
                    if o.read {
                        let mut sum = 0.0;
                        for &n in &order {
                            if !added.values().any(|&a| a == n) {
                                sum += tree.layout(n).location.x;
                            }
                        }
                        std::hint::black_box(sum);
                    }
                    start.elapsed().as_nanos() as f64
                })
            }
            "taffy" => {
                let (mut t, troot, map) = mirror_in_taffy(&tree, root);
                if !o.rounding {
                    t.disable_rounding();
                }
                let index: HashMap<NodeId, taffy::NodeId> = map.iter().copied().collect();
                let tsites: Vec<taffy::NodeId> = sites_c.iter().map(|n| index[n]).collect();
                let originals: Vec<taffy::Style> = tsites.iter().map(|&n| t.style(n).unwrap().clone()).collect();
                let texts: Vec<u32> = sites_c.iter().map(|&n| tree.context(n).map_or(0, |c| c.0)).collect();
                let tnodes: Vec<taffy::NodeId> = map.iter().map(|&(_, n)| n).collect();
                let mut added = HashMap::new();
                t.compute_layout_with_measure(troot, to_taffy_space(vp(false)), taffy_measure).unwrap();
                sample(o.secs, || {
                    let (on, k) = step();
                    let start = Instant::now();
                    edit_taffy(&mut t, scenario, tsites[k], on, &originals[k], texts[k], &mut added);
                    t.compute_layout_with_measure(troot, to_taffy_space(vp(on)), taffy_measure).unwrap();
                    if o.read {
                        let mut sum = 0.0;
                        for &n in &tnodes {
                            sum += t.layout(n).unwrap().location.x;
                        }
                        std::hint::black_box(sum);
                    }
                    start.elapsed().as_nanos() as f64
                })
            }
            _ => continue,
        };
        print(w, size, nodes, engine, scenario.name(), &s, "");
        csv.row(w, size, nodes, engine, "incremental", scenario.name(), &s, "-", 0.0, None);
    }
}

/// Writes `<workload>@<size>.tree` for the external engines: the viewport, one line per node in tree
/// order with every property the external harness maps, cephal's unrounded box, and the edit sites
/// of each scenario.
fn dump(w: &Workload, size: usize, dir: &std::path::Path) {
    let (mut tree, root) = new_tree(w, size, false);
    let mut scenario_sites = Vec::new();
    for &s in w.scenarios {
        if matches!(s, Scenario::MoveAbsolute | Scenario::InsertRemove) {
            continue;
        }
        let at = sites(&mut tree, root, s);
        scenario_sites.push((s, at));
    }
    let vp = viewport(w);
    tree.compute_layout_with_measure(root, vp, measure);
    let mut order = Vec::new();
    all_nodes(&tree, root, &mut order);
    let index: HashMap<NodeId, usize> = order.iter().enumerate().map(|(i, &n)| (n, i)).collect();
    let mut out = String::new();
    let space = |v: Option<f32>| v.map_or("max-content".to_owned(), |v| format!("{v}"));
    out += &format!("cephal-tree 1\nworkload {} {size} {}\ntags {}\nviewport {} {}\n", w.name, w.family, w.tags.join("|"), space(w.viewport.0), space(w.viewport.1));
    for &n in &order {
        let s = tree.style(n);
        let l = tree.unrounded_layout(n);
        out += &format!(
            "node {} {} text={} {} box={},{},{},{}\n",
            index[&n],
            tree.parent(n).map_or(-1, |p| index[&p] as i64),
            tree.context(n).map_or(0, |c| c.0),
            style_fields(s),
            l.location.x,
            l.location.y,
            l.size.width,
            l.size.height
        );
    }
    for (s, at) in scenario_sites {
        let ids: Vec<String> = at.iter().map(|n| index[n].to_string()).collect();
        out += &format!("scenario {} {}\n", s.name(), ids.join(","));
    }
    std::fs::write(dir.join(format!("{}@{size}.tree", w.name)), out).unwrap();
}

/// `key=value` pairs of every property external engines may support; anything they cannot express
/// is written too, so the harness can refuse the workload rather than lay out something else.
fn style_fields(s: &Style) -> String {
    use cephal::style::LengthKind;
    let len = |k: LengthKind| match k {
        LengthKind::Auto => "auto".to_owned(),
        LengthKind::Length(v) => format!("{v}px"),
        LengthKind::Percent(f) => format!("{}%", f * 100.0),
        other => format!("unsupported:{other:?}").replace(' ', ""),
    };
    let align_items = |a: Option<AlignItems>| a.map_or("normal".to_owned(), |a| format!("{:?}", a.keyword).to_lowercase());
    let align_content = |a: Option<cephal::style::AlignContent>| a.map_or("normal".to_owned(), |a| format!("{:?}", a.keyword).to_lowercase());
    let mut f = vec![
        format!("display={}", format!("{:?}", s.display).to_lowercase()),
        format!("position={}", if s.position.is_out_of_flow() { "absolute" } else { "relative" }),
        format!("box-sizing={}", format!("{:?}", s.box_sizing).to_lowercase()),
        format!("direction={}", format!("{:?}", s.direction).to_lowercase()),
        format!("overflow={}", format!("{:?}", s.overflow.x).to_lowercase()),
        format!("width={}", len(s.size.width.0.kind())),
        format!("height={}", len(s.size.height.0.kind())),
        format!("min-width={}", len(s.min_size.width.0.kind())),
        format!("min-height={}", len(s.min_size.height.0.kind())),
        format!("max-width={}", len(s.max_size.width.0.kind())),
        format!("max-height={}", len(s.max_size.height.0.kind())),
        format!("flex-direction={}", format!("{:?}", s.flex_direction).to_lowercase()),
        format!("flex-wrap={}", format!("{:?}", s.flex_wrap).to_lowercase()),
        format!("flex-grow={}", s.flex_grow),
        format!("flex-shrink={}", s.flex_shrink),
        format!("flex-basis={}", len(s.flex_basis.0.kind())),
        format!("align-items={}", align_items(s.align_items)),
        format!("align-self={}", s.align_self.map_or("auto".to_owned(), |a| format!("{:?}", a.keyword).to_lowercase())),
        format!("justify-content={}", align_content(s.justify_content)),
        format!("align-content={}", align_content(s.align_content)),
        format!("aspect-ratio={}", s.aspect_ratio.map_or("auto".to_owned(), |a| a.to_string())),
        format!("gap={},{}", len(s.gap.width.0.kind()), len(s.gap.height.0.kind())),
        format!("float={}", format!("{:?}", s.float).to_lowercase()),
    ];
    for (name, r) in [("margin", s.margin.map(|v| v.0)), ("inset", s.inset.map(|v| v.0))] {
        f.push(format!("{name}={},{},{},{}", len(r.top.kind()), len(r.right.kind()), len(r.bottom.kind()), len(r.left.kind())));
    }
    for (name, r) in [("padding", s.padding.map(|v| v.0)), ("border", s.border.map(|v| v.0))] {
        f.push(format!("{name}={},{},{},{}", len(r.top.kind()), len(r.right.kind()), len(r.bottom.kind()), len(r.left.kind())));
    }
    if s.grid.is_some() || s.grid_row.start != cephal::style::GridPlacement::Auto || s.grid_column.start != cephal::style::GridPlacement::Auto {
        f.push("grid=unsupported".into());
    }
    f.join(" ")
}

fn write_meta(path: &std::path::Path, o: &Options, threads: usize) {
    let run = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd).args(args).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default()
    };
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|c| c.lines().find(|l| l.starts_with("model name")).map(|l| l.split(':').nth(1).unwrap_or("").trim().to_owned()))
        .unwrap_or_default();
    let governor = std::fs::read_to_string("/sys/devices/system/cpu/cpu2/cpufreq/scaling_governor").unwrap_or_default();
    let affinity = run("taskset", &["-cp", &std::process::id().to_string()]);
    let lock = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    let taffy = lock.split("[[package]]").find(|p| p.contains("name = \"taffy\"")).and_then(|p| p.lines().find(|l| l.starts_with("version")).map(|l| l.trim_start_matches("version = ").trim_matches('"').to_owned())).unwrap_or_default();
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ");
    let json = format!(
        "{{\n  \"cpu\": \"{}\",\n  \"governor\": \"{}\",\n  \"affinity\": \"{}\",\n  \"kernel\": \"{}\",\n  \"rustc\": \"{}\",\n  \"cephal\": \"{}\",\n  \"cephal_dirty\": {},\n  \"taffy\": \"{}\",\n  \"profile\": \"bench\",\n  \"features\": \"{}\",\n  \"secs\": {},\n  \"read\": {},\n  \"rounding\": {},\n  \"threads\": {},\n  \"date\": \"{}\"\n}}\n",
        esc(&cpu),
        esc(governor.trim()),
        esc(&affinity),
        esc(&run("uname", &["-r"])),
        esc(&run("rustc", &["--version"])),
        esc(&run("git", &["rev-parse", "HEAD"])),
        !run("git", &["status", "--porcelain", "--untracked-files=no"]).is_empty(),
        esc(&taffy),
        if cfg!(feature = "stats") { "stats" } else { "" },
        o.secs,
        o.read,
        o.rounding,
        threads,
        esc(&run("date", &["-Iseconds"])),
    );
    std::fs::write(path, json).unwrap();
}
