//! Heap bytes per node for cephal and taffy on the same 10k-node tree.

#[path = "../tests/common/mod.rs"]
mod common;

use cephal::Tree;
use cephal::geometry::{AvailableSpace, Size};
use common::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

fn main() {
    let n = 10_000;
    let viewport = Size { width: AvailableSpace::Definite(1000.0), height: AvailableSpace::Definite(800.0) };
    println!("size_of: Style={} Layout={} NodeCache={} taffy::Style={} taffy::Layout={}",
        size_of::<cephal::Style>(), size_of::<cephal::Layout>(), size_of::<cephal::NodeCache>(),
        size_of::<taffy::Style>(), size_of::<taffy::Layout>());

    let before = live();
    let mut tree: Tree<Text> = Tree::new();
    let mut rng = Rng(7 | 1);
    let root = tree.new_leaf(random_style(&mut rng, true));
    let mut count = 1;
    while count < n {
        let sub = build_subtree(&mut rng, &mut tree, 1);
        let mut v = Vec::new();
        all_nodes(&tree, sub, &mut v);
        count += v.len();
        tree.append_child(root, sub);
    }
    let built = live() - before;
    tree.compute_layout_with_measure(root, viewport, measure);
    let laid_out = live() - before;
    println!("cephal: {count} nodes, {} B/node built, {} B/node after layout", built / count, laid_out / count);

    let before = live();
    let (mut t, troot, _) = mirror_in_taffy(&tree, root);
    let built = live() - before;
    t.compute_layout_with_measure(troot, to_taffy_space(viewport), taffy_measure).unwrap();
    let laid_out = live() - before;
    println!("taffy:  {count} nodes, {} B/node built, {} B/node after layout", built / count, laid_out / count);
}
