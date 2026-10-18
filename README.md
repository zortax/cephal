# cephal

`cephal` is a retained CSS layout engine for Rust UI frameworks. It updates layouts
incrementally and can process independent subtrees in parallel.

The engine supports:

- block flow, floats, and margin collapse;
- flexbox, including `flex-wrap: balance`;
- CSS grid, named lines, named areas, and auto-repeat;
- `calc()`, sizing keywords, `box-sizing`, scroll containers, and containment;
- absolute positioning and pixel snapping.

Cephal passes 6,084 Chrome-generated layout fixtures vendored from taffy.

## Incremental layout

Cephal caches each answer that a node gives to its parent. A style change adds the
affected nodes to a dirty queue. The scheduler processes the deepest dirty nodes first.
Propagation stops when a recomputed answer stays unchanged.

Clipped overflow changes stay inside the clipping node. An absolute box can move after a
position-only update. Pixel snapping processes changed nodes and subtrees whose origin moved.

This model follows *Spineless Traversal for Layout Invalidation* by Kirisame, Wang, and
Panchekha (PLDI 2025). The test suite checks that incremental and parallel results equal a
cold serial layout bit for bit.

## Example

```rust
use cephal::style::{Dimension, Display};
use cephal::{AvailableSpace, Size, Style, Tree};

let mut tree: Tree<()> = Tree::new();

let mut row = Style::DEFAULT;
row.display = Display::Flex;
row.size.width = Dimension::length(300.0);

let mut item = Style::DEFAULT;
item.flex_grow = 1.0;
item.size.height = Dimension::length(40.0);

let a = tree.new_leaf(item.clone());
let b = tree.new_leaf(item);
let root = tree.new_with_children(row, &[a, b]);
let viewport = Size {
    width: AvailableSpace::Definite(300.0),
    height: AvailableSpace::MaxContent,
};

tree.compute_layout(root, viewport);
assert_eq!(tree.layout(b).location.x, 150.0);

tree.patch_style(a, |style| style.flex_grow = 3.0);
tree.compute_layout(root, viewport);
assert_eq!(tree.layout(b).location.x, 225.0);
```

Use `Tree::new_leaf_with_context` and `compute_layout_with_measure` for text, images, and
other measured content. `Tree::changed()` returns the nodes that moved during the last
layout call.

`Parallel` owns a persistent worker pool. It sends disjoint child subtrees to the pool.
Contexts and the measure function must implement `Send + Sync`.

Frameworks that own their tree can implement `LayoutTree`, `CacheAccess`, and
`Incremental`. They can then call the layout algorithms directly.

## Performance

These results use an Intel Core i9-13900K and 24 threads. Measurements come from
`cargo bench` and `cargo run --release --example scaling`.

| Scenario | Serial | Parallel | taffy |
| --- | ---: | ---: | ---: |
| Contained edit in a 100,000-node list | 1.2 µs | 1.9 µs | 5 ms |
| Move an absolute box in a 100,000-node tree | 2.4 µs | | 20 ms |
| Leaf-width edit in a 100,000-node random tree | 7.8 ms | 8.3 ms | 19 ms |
| Cold layout of 100,000 nested flex nodes | 21 ms | 11 ms | 40 ms |
| Cold layout of 10,000 leaves in one flex row | 1.05 ms | 1.2 ms | 2.0 ms |
| Cold layout of a 100×100 grid | 4.0 ms | 4.1 ms | 9.1 ms |
| Cold layout of 10,000 random nodes | 20 ms | 10 ms | 22 ms |
| Resize a 10,000-node random tree | 1.8 ms | 1.9 ms | 2.0 ms |

Run `cargo run --release --example scaling` to measure trees from 1,000 to 100,000 nodes.
Use `scripts/plot_bench.py` to plot the results.

`benches/pangui.rs` ports the cold-layout benchmarks from
[PanGui's layout post](https://www.pangui.io/blog/05-layout-rework-and-benchmarks/) to CSS and
adds grid, block, text, and random workloads. It prints the median time per node:

```sh
taskset -c 2 cargo bench --bench pangui [-- <filter>]
CEPHAL_BENCH_TAFFY=1 CEPHAL_BENCH_CSV=target/bench/pangui.csv taskset -c 2 cargo bench --bench pangui
python3 scripts/plot_bench.py pangui target/bench/pangui.csv --out target/bench/plots
```

`CEPHAL_BENCH_TAFFY=1` also times taffy on the same tree. `CEPHAL_BENCH_CSV` writes one row per
workload and engine. The `pangui` plot mode compares these with the numbers PanGui published for
PanGui, Clay, Taffy, and Yoga on a Ryzen 7 7800X3D. It also plots each engine's speed-up over
taffy on the same machine. `--baseline other.csv` adds a before-and-after chart.
`CEPHAL_NO_ROUNDING=1` disables pixel snapping in both engines, and `CEPHAL_BENCH_SECS` sets the
sampling time.

A few rows, in nanoseconds per node on one pinned core, with PanGui's published numbers from a
different machine (Ryzen 7 7800X3D) for orientation:

| Workload | cephal | taffy | PanGui | Clay |
| --- | ---: | ---: | ---: | ---: |
| `wide_no_wrap_simple_few` (1,001 nodes) | 88 | 151 | 41 | 34 |
| `nested_vertical_stack` (10,001) | 94 | 208 | 65 | 36 |
| `flex_expand_equal_weights` (15,001) | 113 | 227 | 71 | 41 |
| `fit_nesting` (101,111) | 248 | 949 | 66 | 40 |
| `fit_nesting_min0` (101,111) | 118 | 546 | | |

PanGui's `Expand` has no automatic minimum size, but CSS flex items do. The `*_min0` variants add
`min-width: 0` and `min-height: 0` to match PanGui. PanGui's
`pixels_with_min_expand_constraint` has no CSS equivalent and is not ported.

## Tests

- `tests/fixtures.rs` runs 6,084 Chrome-generated fixtures from taffy.
- `tests/differential.rs` compares random trees with taffy.
- `tests/incremental.rs` compares incremental and parallel results with cold serial layouts.

Use `CEPHAL_SEEDS` and `CEPHAL_STEPS` to set the size of the random test suites. Use
`CEPHAL_FIXTURE=name` to select fixtures.

## License

Cephal is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
