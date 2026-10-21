# cephal

`cephal` is a retained CSS layout engine for Rust UI frameworks. It updates layouts
incrementally and can process independent subtrees in parallel.

The engine supports:

- block flow, floats, and margin collapse;
- flexbox, including `flex-wrap: balance`;
- CSS grid, named lines, named areas, and auto-repeat;
- `calc()`, sizing keywords, `box-sizing`, scroll containers, and containment;
- absolute and fixed positioning against the containing block (out-of-flow boxes are laid out by
  their nearest positioned ancestor or the viewport, with static positions and self-alignment);
- pixel snapping.

Cephal passes the 6,504 Chrome-generated layout fixtures of taffy 0.15 and 188 of its own. Where
cephal and taffy disagree, Chrome decides: `tests/fixtures/TAFFY_DIVERGENCES.md` lists the cases
where taffy differs from the CSS specification and Chrome, with the fixtures that pin them.

## Incremental layout

Cephal caches each answer that a node gives to its parent. A style change adds the
affected nodes to a dirty queue. The scheduler processes the deepest dirty nodes first.
Propagation stops when a recomputed answer stays unchanged.

Clipped overflow changes stay inside the clipping node. An absolute box can move after a
position-only update without its containing block's in-flow content being laid out again. Pixel snapping processes changed nodes and subtrees whose origin moved.

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

These results come from `benches/suite.rs` on an Intel Core i9-13900K, pinned to one performance
core (the parallel column uses the 16 performance cores), against taffy 0.15, Yoga 3.2.1 and
Clay 0.14 on the same trees. Every timing includes reading every node's layout afterwards. `≠`
marks an engine whose layout differs from cephal's, so its number is not a comparison; an empty
cell is a workload the engine cannot express.

Cold layout, nanoseconds per node:

| Workload (nodes) | cephal | parallel | taffy | Yoga | Clay |
| --- | ---: | ---: | ---: | ---: | ---: |
| 10,000 fixed items in a flex row | 59 | 90 | 182 | 312 | 82 ≠ |
| 10,000 items in a flex column with gap | 60 | 88 | 216 | 349 | 83 |
| 10,000 wrapping text items in a flex row | 187 | 317 | 289 | 1039 | |
| 10,000 paragraphs in block flow | 92 | 99 | 165 | | |
| 100×100 grid of fixed tracks | 125 | 133 | 166 | | |
| 100×100 grid of auto tracks holding text | 449 | 798 | 604 | | |
| dashboard: a grid of flex cards of text (8,751) | 433 | 132 | 1044 | | |
| nested intrinsic flex rows (101,111, `min-width: 0`) | 71 | 27 | 323 | 638 | |
| random trees of block, flex and grid (10,004) | 1458 | 734 | 1485 ≠ | | |

Incremental frames on trees of about 100,000 nodes, one edit and a layout. Reading the layouts
afterwards costs about 135 µs, which is the whole idle frame:

| Edit | cephal | taffy |
| --- | ---: | ---: |
| Text of one cell in a list | 156 µs | 6.4 ms |
| Text of one card in the dashboard | 135 µs | 5.7 ms |
| Width of one element in the dashboard | 266 µs | 5.7 ms |
| Move an absolutely positioned box | 135 µs | 5.6 ms |
| Move a relatively positioned box | 176 µs | 5.7 ms |
| Resize the viewport of a document of paragraphs | 260 µs | 4.8 ms |
| Resize the viewport of the dashboard | 3.8 ms | 5.6 ms |
| Width of one element in a random tree | 4.6 ms | 17.4 ms |
| `display: none` on a container of a random tree | 30 ms | 19 ms |
| Nothing | 136 µs | 3.9 ms |

The last row but one is slower than taffy: random trees hold thousands of absolutely positioned
boxes, and re-running the root lays out every one the viewport contains again.

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

PanGui's workloads, in nanoseconds per node, with PanGui's published numbers from a different
machine (Ryzen 7 7800X3D) for orientation:

| Workload | cephal | taffy | Clay | PanGui (published) | Clay (published) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `wide_no_wrap_simple_few` (1,001 nodes) | 57 | 152 | 76 ≠ | 41 | 34 |
| `nested_vertical_stack` (10,001) | 62 | 212 | 83 | 65 | 36 |
| `flex_expand_equal_weights` (15,001) | 65 | 238 | 91 | 71 | 41 |
| `fit_nesting` (101,111) | 157 | 667 | 119 | 66 | 40 |
| `fit_nesting_min0` (101,111) | 71 | 323 | | | |

Clay is timed declaring its elements and laying them out, which is its whole frame, and reads its
render commands.
PanGui's `Expand` has no automatic minimum size, but CSS flex items do. The `*_min0` variants add
`min-width: 0` and `min-height: 0` to match PanGui. PanGui's
`pixels_with_min_expand_constraint` has no CSS equivalent and is not ported.

## Tests

- `tests/fixtures.rs` runs 6,504 Chrome-generated fixtures from taffy 0.15
  (`scripts/update_fixtures.sh` regenerates them) and cephal's own, written as HTML in
  `tests/fixtures/html` and rendered in Chrome by `scripts/gen_fixtures.sh`.
- `tests/differential.rs` compares random trees with taffy, apart from the divergences listed in
  `tests/differential_known.txt`. `scripts/chrome_seeds.sh` renders those trees in Chrome to tell
  which engine is right.
- `tests/incremental.rs` compares incremental and parallel results with cold serial layouts.

Use `CEPHAL_SEEDS` and `CEPHAL_STEPS` to set the size of the random test suites. Use
`CEPHAL_FIXTURE=name` to select fixtures.

## License

Cephal is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
