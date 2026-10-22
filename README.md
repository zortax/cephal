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
position-only update without its containing block's in-flow content being laid
out again. Pixel snapping processes changed nodes and subtrees whose origin
moved.

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

Run on an Intel Core i9-13900K (one core, parallel: 16 cores), every layout
read back after each frame.

| Cold layout (ns per node) | cephal | cephal (parallel) | taffy 0.15 | Yoga 3.2.1 | Clay 0.14 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Flex column with gap (10,001 nodes) | 59 | 88 | 212 | 339 | 84 |
| Wrapping text in a flex row (10,001 nodes) | 184 | 314 | 281 | 1,034 | – |
| 100×100 grid of auto tracks with text (10,001 nodes) | 441 | 788 | 585 | – | – |
| Grid of flex cards with text (8,751 nodes) | 429 | 132 | 1,052 | – | – |
| Nested intrinsic flex rows (101,111 nodes) | 68 | 26 | 317 | 640 | – |

| One edit, then layout (about 100,000 nodes) | cephal | taffy 0.15 | Speed-up |
| --- | ---: | ---: | ---: |
| Change the text of a list item | 0.14 ms | 6.1 ms | 44× |
| Change the width of a list item | 0.15 ms | 6.1 ms | 39× |
| Move an absolutely positioned box in a dashboard | 0.13 ms | 5.5 ms | 41× |
| Resize the viewport of a text document | 0.26 ms | 4.6 ms | 18× |
| Change `align-self` in a random tree | 0.15 ms | 17.2 ms | 116× |
| Move a relatively positioned box in a random tree | 0.17 ms | 17.3 ms | 102× |

## Tests

- **Chrome fixtures**: trees whose layout Chrome rendered, taffy's set and cephal's own, must lay
  out the same in cephal.
- **Differential testing**: random trees are laid out by cephal and taffy; where they disagree,
  rendering the tree in Chrome decides which engine is right.
- **Incremental equivalence**: random edit sequences are applied to random trees, and after every
  edit the incremental and parallel layouts must equal a cold layout bit for bit.

## License

Cephal is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
