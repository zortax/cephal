# Where taffy 0.15 is wrong and cephal follows Chrome

`tests/differential.rs` compares cephal with taffy 0.15 on random trees. Seeds where they still
differ are listed in `tests/differential_known.txt`, each with the census category that explains
it (the `Strip` whose removal makes the engines agree). Chrome adjudicates such seeds
(`scripts/chrome_seeds.sh <seeds>`, see `tests/differential.rs`); the behaviours below are those
where Chrome sides with cephal. Each has a small fixture in `tests/fixtures/html`, rendered into
`tests/fixtures/cephal.xml` by `scripts/gen_fixtures.sh`; `CEPHAL_FIXTURE_TAFFY=1 cargo test
--release --test fixtures cephal -- --nocapture` shows taffy's layout of them.

## A grid item's minimum contribution below its padding and border

Census category `GridItemPaddingBorder`. Fixture `grid_item_size_floored_by_padding_border`.

A box's border box is never smaller than its padding and border (CSS 2.2 §10.2, css-sizing-3
§5.2: `box-sizing: border-box` floors the content box at zero). The minimum contribution of a
grid item (css-grid-1 §6.6) is the outer size the item gets from its definite preferred size, or
its minimum size, so it is floored the same way. Taffy takes a definite preferred size such as
`width: 2px` with `padding-left: 10px` as a minimum contribution of 2px, and so sizes an
`auto`-minimum track smaller than the item.

## The root (both engines, by design)

Not a divergence between the engines, but where both differ from a browser's root element,
which is a block-level box in the initial containing block: an in-flow root's `auto` margins
count as zero, and an in-flow flex or grid root of `auto` width takes the width its layout
algorithm gives it instead of filling the viewport. When the viewport height is indefinite, the
initial containing block is the root's padding box vertically, and an out-of-flow root is laid
out in flow. The Chrome fixtures `to_html` writes pin these (or report them as not expressible).
