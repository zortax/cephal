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

## A measured leaf of known size measured at its border-box size

Census category `LeafInsets`. Fixture `leaf_known_width_is_border_box`.

A measure function lays out a leaf's content (text, here) in its content box; when the leaf's
width is known, the content gets that width less the padding, border and scrollbar (CSS 2.2
§10.1, css-overflow-3 §3 for the scrollbar gutter). Taffy hands the measure function the known
border-box size, so text in a 25px grid column with a 15px scrollbar is laid out 25px wide
instead of 10px, and wraps less.

## Percentage margins in a block's intrinsic width

Census category `PercentMargins`. Fixture `block_intrinsic_width_percentage_margins`.

A percentage margin refers to the width of the containing block, which the intrinsic width of a
block container is being found for: in its children's intrinsic contributions such margins
count as zero (css-sizing-3 §5.2.1), and resolve against the width found afterwards. Taffy
resolves them against the available space, so the shrink-to-fit width of an absolutely
positioned block grows by 3% of its containing block for a child's `margin-left: 3%`.

## Percentage heights in a row flex item that is not stretched

Census category `FlexCrossAlignment`. Fixture `flex_item_unstretched_cross_size_indefinite`.

Only a stretched flex item's cross size is definite (css-flexbox-1 §9.8); one aligned otherwise,
or with an `auto` cross margin, is laid out at its content height, which percentages of its
contents cannot refer to: when it is a flex container, its items' percentage heights and minimum
heights and its percentage row gap behave as `auto` (CSS 2.2 §10.5). Taffy resolves them
against the height the item ends up with. (A width is always definite for percentages, so
column flex items are not affected.)

## An absolutely positioned box's content height before its width

Census category `Absolute` (with others). Fixtures `abspos_width_before_height`,
`abspos_content_sized_height_after_width`.

An absolutely positioned box of automatic (or content-based keyword) height gets its width
first and then its height at that width (CSS 2.2 §10.3.7 before §10.6.4), so percentages inside
resolve against the width it ends up with. Taffy finds a content-sized height (`fit-content`,
`max-content`, `min-content`) before the width, and lays out an absolutely positioned root
measuring both axes at once; the percentage width of a column flex item, and margins inside
it, then resolve against nothing.

## A percentage row gap against an indefinite height

Census category `PercentGaps`. Fixture `flex_column_percentage_row_gap_indefinite`.

A percentage gap refers to the container's content box (css-align-3 §8.1); Chrome takes a
percentage row gap of a flex container whose height is indefinite as zero, also once the height
is found from the content. Taffy resolves it against that height, which then shrinks the items.

## Lines of a multi-line flex container whose main size is not definite

Census category `FlexWrap`. Fixture `flex_column_wrap_item_breaks_at_its_size`.

A multi-line flex container breaks its lines at its inner main size (css-flexbox-1 §9.3), the
size it is laid out at, whether or not that size counts as definite for percentages (the
height of a column flex item in a container of auto height, held by a max-height). Taffy puts
all items of such a container on one line.

## A column flex container's content height from items' heights

No census category of its own (seeds 484, 491). Fixture
`flex_column_intrinsic_height_from_flex_basis`.

The content height of a column flex container is the sum of its items' hypothetical main sizes,
their flex base sizes clamped (Chrome; css-flexbox-1 §9.9.1 describes contributions instead): a
flex basis overrides the item's `height`. Taffy lets the `height` raise the item's contribution
past its flex base size, and the container grows by it.

## Percentages inside an absolutely positioned box of content height

Census category `Absolute` (with others). Fixture `abspos_content_height_indefinite`.

The height of an absolutely positioned box found from its content is not definite (CSS 2.2
§10.5, css-sizing-3 §2.1): percentage heights inside behave as `auto`, percentage maximum heights
as `none`, and a percentage row gap is zero. A height between two insets, or transferred from
the width through `aspect-ratio`, is definite. Taffy resolves them against the height found.

## The root (both engines, by design)

Not a divergence between the engines, but where both differ from a browser's root element,
which is a block-level box in the initial containing block: an in-flow root's `auto` margins
count as zero, and an in-flow flex or grid root of `auto` width takes the width its layout
algorithm gives it instead of filling the viewport. When the viewport height is indefinite, the
initial containing block is the root's padding box vertically, and an out-of-flow root is laid
out in flow. The Chrome fixtures `to_html` writes pin these (or report them as not expressible).
