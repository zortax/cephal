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

Measured with `benches/suite.rs` on an Intel Core i9-13900K, pinned to one core, against taffy 0.15,
Yoga 3.2.1 and Clay 0.14 on the same trees, including reading every layout back. Cold layout, in
nanoseconds per node (an empty cell is a tree the engine cannot express):

| Workload (nodes) | cephal | taffy | Yoga | Clay |
| --- | ---: | ---: | ---: | ---: |
| 10,000 items in a flex column with gap | 59 | 212 | 339 | 84 |
| 10,000 wrapping text items in a flex row | 184 | 281 | 1034 | |
| 100×100 grid of auto tracks holding text | 441 | 585 | | |
| a grid of flex cards of text (8,751) | 429 | 1052 | | |
| nested intrinsic flex rows (101,111, `min-width: 0`) | 68 | 317 | 640 | |

Laying out in parallel on 16 cores brings the last two rows to 132 and 26 ns per node. On trees
of about 100,000 nodes (a list, a dashboard, a document), a frame with a single edit (text, a width,
moving a positioned box) takes 0.12–0.29 ms with cephal and 4.6–6.1 ms with taffy. Cephal is faster
in every incremental scenario of the suite.

```sh
taskset -c 2 cargo bench --bench suite [-- <filter>...]   # cephal and taffy
scripts/external.sh [<filter>...]                         # Yoga and Clay
python3 scripts/plot_bench.py suite target/bench/suite.csv --out target/bench/plots
```

`scripts/perf.md` describes the workloads, options and plots.

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
