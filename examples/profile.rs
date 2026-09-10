//! Profiling driver: `cargo run --release --example profile -- random 10000 50`.

#[path = "../tests/common/mod.rs"]
mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Counts allocator calls so a layout's allocation count can be reported.
struct Counting;
static ALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;

use cephal::Tree;
use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{Dimension, Display, Style};
use cephal::tree::NodeId;
use common::*;

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

fn build_nested_block(tree: &mut Tree<Text>, b: usize, levels: usize) -> NodeId {
    let mut s = Style::DEFAULT;
    s.display = Display::Block;
    s.size.width = Dimension::length(500.0);
    let node = tree.new_leaf(s);
    if levels > 0 {
        for _ in 0..b {
            let c = build_nested_block(tree, b, levels - 1);
            tree.append_child(node, c);
        }
    }
    node
}

/// One flex row of text leaves.
fn build_flat(tree: &mut Tree<Text>, n: usize) -> NodeId {
    let root = tree.new_leaf(fixed_style());
    for i in 0..n {
        let c = tree.new_leaf_with_context(Style::DEFAULT, Text(3 + (i % 5) as u32));
        tree.append_child(root, c);
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let shape = args.get(1).map(String::as_str).unwrap_or("random");
    let n: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(10_000);
    let iters: usize = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(20);
    let viewport = Size { width: AvailableSpace::Definite(1000.0), height: AvailableSpace::Definite(800.0) };
    if shape.starts_with("cold-") {
        let mut tree: Tree<Text> = Tree::new();
        let root = match shape.strip_prefix("cold-").unwrap() {
            "nested" => build_nested(&mut tree, 10, (n as f64).log10() as usize),
            "block" => build_nested_block(&mut tree, 10, (n as f64).log10() as usize),
            "flat" => build_flat(&mut tree, n),
            "grid" => build_grid(&mut tree, (n as f64).sqrt() as usize),
            _ => build_random(&mut tree, 3, n),
        };
        if let Ok(force) = std::env::var("CEPHAL_FORCE_DISPLAY") {
            let display = match force.as_str() {
                "flex" => Display::Flex,
                "grid" => Display::Grid,
                "block" => Display::Block,
                _ => panic!("flex, grid or block"),
            };
            let mut nodes = Vec::new();
            all_nodes(&tree, root, &mut nodes);
            for n in nodes {
                if tree.child_count(n) > 0 && tree.style(n).display != Display::None {
                    tree.patch_style(n, |s| s.display = display);
                }
                if std::env::var_os("CEPHAL_NO_BASELINE").is_some() {
                    tree.patch_style(n, |s| {
                        if s.align_items == Some(cephal::style::AlignItems::BASELINE) {
                            s.align_items = None;
                        }
                    });
                }
            }
        }
        if std::env::var_os("CEPHAL_STATS").is_some() {
            let mut nodes = Vec::new();
            all_nodes(&tree, root, &mut nodes);
            let mut counts = std::collections::BTreeMap::new();
            for n in &nodes {
                let kind = if tree.child_count(*n) == 0 { "leaf".to_owned() } else { format!("{:?}", tree.style(*n).display) };
                *counts.entry(kind).or_insert(0usize) += 1;
            }
            println!("nodes by kind: {counts:?}");
        }
        let parallel = std::env::var("CEPHAL_PARALLEL").ok().and_then(|v| v.parse::<usize>().ok());
        let executor = cephal::Parallel::with_threads(
            |ctx: &Text, input: cephal::tree::MeasureInput, style: &cephal::Style| {
                let mut ctx = *ctx;
                measure(&mut ctx, input, style)
            },
            parallel.unwrap_or(1),
        );
        let start = std::time::Instant::now();
        ALLOC_CALLS.store(0, Ordering::Relaxed);
        let skip_cephal = std::env::var_os("CEPHAL_SKIP").is_some();
        for _ in 0..if skip_cephal { 0 } else { iters } {
            tree.clear_layout();
            match parallel {
                Some(_) => tree.compute_layout_parallel(root, viewport, &executor),
                None => tree.compute_layout_with_measure(root, viewport, measure),
            }
        }
        if std::env::var_os("CEPHAL_DUMP_KEYS").is_some() {
            tree.clear_layout();
            tree.compute_layout_with_measure(root, viewport, measure);
            let mut nodes = Vec::new();
            all_nodes(&tree, root, &mut nodes);
            let mut shown = 0;
            for n in nodes {
                let entries = tree.cache_entries(n);
                if tree.child_count(n) > 0 && entries.len() >= 7 && shown < 3 {
                    shown += 1;
                    println!("container depth {} with {} children, {} entries:", tree.depth(n), tree.child_count(n), entries.len());
                    for (i, o) in entries {
                        println!(
                            "  {:?} {:?} {:?} known={:?}/{:?} def={}/{} parent={:?}/{:?} avail={:?}/{:?} -> {}x{}",
                            i.run_mode, i.sizing_mode, i.axis, i.known_dimensions.width, i.known_dimensions.height,
                            i.known_dimensions_are_definite.width, i.known_dimensions_are_definite.height,
                            i.parent_size.width, i.parent_size.height, i.available_space.width, i.available_space.height, o.size.width, o.size.height
                        );
                    }
                }
            }
        }
        #[cfg(feature = "stats")]
        {
            tree.clear_layout();
            let _ = cephal::compute::stats::report();
            match parallel {
                Some(_) => tree.compute_layout_parallel(root, viewport, &executor),
                None => tree.compute_layout_with_measure(root, viewport, measure),
            }
            println!("queries: {}", cephal::compute::stats::report());
        }
        println!(
            "{shape} {n}: {:.3} ms per cold layout, {} allocations per layout",
            start.elapsed().as_secs_f64() * 1000.0 / iters as f64,
            ALLOC_CALLS.load(Ordering::Relaxed) / iters
        );
        if std::env::var_os("CEPHAL_TAFFY").is_some() {
            let (mut t, troot, map) = mirror_in_taffy(&tree, root);
            let start = std::time::Instant::now();
            ALLOC_CALLS.store(0, Ordering::Relaxed);
            for _ in 0..iters {
                for (_, tn) in &map {
                    t.mark_dirty(*tn).unwrap();
                }
                t.compute_layout_with_measure(troot, to_taffy_space(viewport), taffy_measure).unwrap();
            }
            println!(
                "taffy: {:.3} ms per cold layout, {} allocations per layout",
                start.elapsed().as_secs_f64() * 1000.0 / iters as f64,
                ALLOC_CALLS.load(Ordering::Relaxed) / iters
            );
        }
        return;
    }
    if shape == "resize" {
        // Alternating viewport widths on a random tree; serial or parallel via CEPHAL_PARALLEL.
        let mut tree: Tree<Text> = Tree::new();
        let root = build_random(&mut tree, 7, n);
        let parallel = std::env::var("CEPHAL_PARALLEL").ok().and_then(|v| v.parse::<usize>().ok());
        let executor = cephal::Parallel::with_threads(
            |ctx: &Text, input: cephal::tree::MeasureInput, style: &cephal::Style| {
                let mut ctx = *ctx;
                measure(&mut ctx, input, style)
            },
            parallel.unwrap_or(1),
        );
        let mut flip = false;
        let frame = |tree: &mut Tree<Text>, flip: bool| {
            let vp = Size { width: AvailableSpace::Definite(if flip { 1000.0 } else { 1100.0 }), height: viewport.height };
            match parallel {
                Some(_) => tree.compute_layout_parallel(root, vp, &executor),
                None => tree.compute_layout_with_measure(root, vp, measure),
            }
        };
        for _ in 0..4 {
            flip = !flip;
            frame(&mut tree, flip);
        }
        #[cfg(feature = "stats")]
        {
            let _ = cephal::compute::stats::report();
            flip = !flip;
            frame(&mut tree, flip);
            println!("queries: {}", cephal::compute::stats::report());
        }
        let start = std::time::Instant::now();
        for _ in 0..iters {
            flip = !flip;
            frame(&mut tree, flip);
        }
        println!("resize {n}: {:.3} ms per frame", start.elapsed().as_secs_f64() * 1000.0 / iters as f64);
        return;
    }
    if shape == "rerun" {
        // A flat flex row of leaves; one leaf's width flips so the row re-runs every frame.
        let mut tree: Tree<Text> = Tree::new();
        let root = tree.new_leaf(fixed_style());
        let mut leaves = Vec::new();
        for _ in 0..n {
            let c = tree.new_leaf(fixed_style());
            tree.append_child(root, c);
            leaves.push(c);
        }
        tree.compute_layout_with_measure(root, viewport, measure);
        let start = std::time::Instant::now();
        for i in 0..iters {
            let w = if i % 2 == 0 { 10.0 } else { 12.0 };
            tree.patch_style(leaves[n / 2], |s| s.size.width = Dimension::length(w));
            tree.compute_layout_with_measure(root, viewport, measure);
        }
        println!("rerun {n}: {:.3} us per frame", start.elapsed().as_secs_f64() * 1e6 / iters as f64);
        return;
    }
    if shape == "animate" {
        // A large flex root with a small absolutely positioned child that keeps moving.
        let mut tree: Tree<Text> = Tree::new();
        let root = build_random(&mut tree, 7, n);
        let mut s = Style::DEFAULT;
        s.position = cephal::style::Position::Absolute;
        s.size = Size { width: Dimension::length(50.0), height: Dimension::length(50.0) };
        s.inset.left = cephal::style::LengthPercentageAuto::length(10.0);
        s.inset.top = cephal::style::LengthPercentageAuto::length(10.0);
        let abs = tree.new_leaf(s);
        tree.append_child(root, abs);
        tree.compute_layout_with_measure(root, viewport, measure);
        let start = std::time::Instant::now();
        for i in 0..iters {
            let x = 10.0 + (i % 50) as f32;
            tree.patch_style(abs, |s| s.inset.left = cephal::style::LengthPercentageAuto::length(x));
            tree.compute_layout_with_measure(root, viewport, measure);
        }
        println!("animate: {:.3} us per frame, {} changed", start.elapsed().as_secs_f64() * 1e6 / iters as f64, tree.changed().len());
        return;
    }
    if shape == "typing" {
        let mut tree: Tree<Text> = Tree::new();
        let root = build_random(&mut tree, 7, n);
        let mut nodes = Vec::new();
        all_nodes(&tree, root, &mut nodes);
        let leaf = *nodes.iter().filter(|n| tree.child_count(**n) == 0 && tree.context(**n).is_some()).nth(nodes.len() / 3).unwrap();
        tree.compute_layout_with_measure(root, viewport, measure);
        let mut depth = 0;
        let mut p = Some(leaf);
        while let Some(x) = p { depth += 1; p = tree.parent(x); }
        println!("leaf depth {depth}, root children {}", tree.child_count(root));
        let start = std::time::Instant::now();
        for i in 0..iters {
            tree.set_context(leaf, Some(Text(5 + (i % 2) as u32)));
            tree.compute_layout_with_measure(root, viewport, measure);
        }
        println!("typing: {:.3} us per frame, {} changed", start.elapsed().as_secs_f64() * 1e6 / iters as f64, tree.changed().len());
        return;
    }
    let start = std::time::Instant::now();
    for _ in 0..iters {
        let mut tree: Tree<Text> = Tree::new();
        let root = match shape {
            "nested" => build_nested(&mut tree, 10, (n as f64).log10() as usize),
            "grid" => build_grid(&mut tree, (n as f64).sqrt() as usize),
            _ => build_random(&mut tree, 3, n),
        };
        tree.compute_layout_with_measure(root, viewport, measure);
    }
    println!("{shape} {n}: {:.3} ms per build+layout", start.elapsed().as_secs_f64() * 1000.0 / iters as f64);
}
