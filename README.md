# cephal

`cephal` is a retained CSS layout engine for Rust UI frameworks. It updates layouts
incrementally and can process independent subtrees in parallel.

The engine supports:

- block flow, floats, and margin collapse;
- flexbox, including `flex-wrap: balance`;
- CSS grid, named lines, named areas, and auto-repeat;
- `calc()`, sizing keywords, `box-sizing`, scroll containers, and containment;
- absolute positioning and pixel snapping.

Cephal runs the 6,504 Chrome-generated layout fixtures of taffy 0.15. The ones it does not pass yet
are listed by cause in `tests/fixtures/expected_failures.txt`.

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
| Cold layout of 100,000 nested flex nodes | 16 ms | 10 ms | 39 ms |
| Cold layout of 10,000 leaves in one flex row | 0.68 ms | 0.98 ms | 2.0 ms |
| Cold layout of a 100×100 grid | 4.0 ms | 4.1 ms | 9.1 ms |
| Cold layout of 10,000 random nodes | 20 ms | 10 ms | 22 ms |
| Resize a 10,000-node random tree | 1.8 ms | 1.9 ms | 2.0 ms |

Run `cargo run --release --example scaling` to measure trees from 1,000 to 100,000 nodes.
Use `scripts/plot_bench.py` to plot the results.

`benches/suite.rs` is a comparative suite of about 60 workloads in `benches/workloads`, each
written as CSS and tagged by what it exercises: PanGui's published workloads
([PanGui's layout post](https://www.pangui.io/blog/05-layout-rework-and-benchmarks/)), flex, grid
and block shapes, mixed documents, shapes expected to favour cephal or taffy, and pairs of
workloads that take one of cephal's fast paths or miss it by one property. It times cold layouts,
incremental frames (edits such as typing, hover, `display` toggles, inserted rows, resizes) and
one-shot builds, for cephal, cephal in parallel, and taffy, and checks every engine's layout
against cephal's:

```sh
taskset -c 2 cargo bench --bench suite [-- <filter>...] [--tag grid] [--modes cold,incremental,build]
scripts/external.sh [<filter>...]          # Yoga and Clay on the same trees, same machine
scripts/instructions.sh --csv target/bench/instructions.csv
python3 scripts/plot_bench.py suite target/bench/suite.csv \
    --instructions target/bench/instructions.csv --out target/bench/plots
```

`scripts/external.sh` builds pinned releases of Yoga and Clay and lays out the trees the suite
dumps. Every property is mapped to its CSS meaning, and a workload an engine cannot express is
skipped. Where an engine's layout still differs from cephal's, for example because Yoga has no
automatic minimum size or Clay does not shrink fixed sizes, the plots mark it as not comparable.
The plots include a heatmap of every workload and engine, per-category comparisons, scaling
curves, what each fast path is worth, incremental frame times against frame budgets, measure-call
counts, instructions against time, measurement noise, and engine agreement. `report.md` holds
the same numbers as tables. The `pangui` plot mode also compares the PanGui workloads with the
numbers PanGui published on a Ryzen 7 7800X3D. `scripts/perf.md` lists the options.

A few rows, in nanoseconds per node on one pinned core, with PanGui's published numbers from a
different machine (Ryzen 7 7800X3D) for orientation:

| Workload | cephal | taffy | PanGui | Clay |
| --- | ---: | ---: | ---: | ---: |
| `wide_no_wrap_simple_few` (1,001 nodes) | 62 | 138 | 41 | 34 |
| `nested_vertical_stack` (10,001) | 67 | 204 | 65 | 36 |
| `flex_expand_equal_weights` (15,001) | 71 | 212 | 71 | 41 |
| `fit_nesting` (101,111) | 150 | 916 | 66 | 40 |
| `fit_nesting_min0` (101,111) | 76 | 530 | | |

PanGui's `Expand` has no automatic minimum size, but CSS flex items do. The `*_min0` variants add
`min-width: 0` and `min-height: 0` to match PanGui. PanGui's
`pixels_with_min_expand_constraint` has no CSS equivalent and is not ported.

## Tests

- `tests/fixtures.rs` runs 6,504 Chrome-generated fixtures from taffy 0.15 (`scripts/update_fixtures.sh`
  regenerates them), apart from the listed expected failures.
- `tests/differential.rs` compares random trees with taffy, apart from the divergences listed in
  `tests/differential_known.txt`.
- `tests/incremental.rs` compares incremental and parallel results with cold serial layouts.

Use `CEPHAL_SEEDS` and `CEPHAL_STEPS` to set the size of the random test suites. Use
`CEPHAL_FIXTURE=name` to select fixtures.

## License

Cephal is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
