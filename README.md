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
| Contained edit in a 100,000-node list | 2 µs | 2.5 µs | 5 ms |
| Move an absolute box in a 100,000-node tree | 2.4 µs | | 21 ms |
| Leaf-width edit in a 100,000-node random tree | 8.4 ms | 10.0 ms | 18 ms |
| Cold layout of 100,000 nested flex nodes | 34 ms | 12 ms | 39 ms |
| Cold layout of 10,000 leaves in one flex row | 1.7 ms | 1.9 ms | 2.0 ms |
| Cold layout of a 100×100 grid | 8.4 ms | 8.6 ms | 9.2 ms |
| Cold layout of 10,000 random nodes | 25 ms | 12 ms | 22 ms |
| Resize a 10,000-node random tree | 2.0 ms | 2.1 ms | 2.0 ms |

Run `cargo run --release --example scaling` to measure trees from 1,000 to 100,000 nodes.
Use `scripts/plot_bench.py` to plot the results.

## Tests

- `tests/fixtures.rs` runs 6,084 Chrome-generated fixtures from taffy.
- `tests/differential.rs` compares random trees with taffy.
- `tests/incremental.rs` compares incremental and parallel results with cold serial layouts.

Use `CEPHAL_SEEDS` and `CEPHAL_STEPS` to set the size of the random test suites. Use
`CEPHAL_FIXTURE=name` to select fixtures.

## License

Cephal is licensed under the Apache License 2.0. See [LICENSE](LICENSE).
