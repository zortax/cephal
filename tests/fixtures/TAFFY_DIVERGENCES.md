# Where taffy 0.15 is wrong and cephal follows Chrome

`tests/differential.rs` compares cephal with taffy 0.15 on random trees. Seeds where they still
differ are listed in `tests/differential_known.txt`, each with the census category that explains
it (the `Strip`s whose removal makes the engines agree, see `census`). Chrome adjudicates such seeds
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

## A block's flex or grid child below its padding and border

Census category `unexplained`. Fixture `block_child_size_floored_by_padding_border`.

The same floor (CSS 2.2 §10.2) for a block container's flex and grid children whose max-width or
max-height clamps their size below their padding and border: taffy gives the flex container the
clamped size.

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

## Percentage padding in a content-based flex base size

Census category `PercentPadding`. Fixture `flex_basis_content_percentage_padding`.

Percentage padding refers to the containing block's width (CSS 2.2 §8.4), the flex container's
width for a flex item, also while its content-based flex base size is measured. Taffy measures
a row item's base size without the container's width, so its percentage padding counts as zero
there and the item gets narrower than its content.

## `align-content` of a block container whose content overflows

Census category `BlockAlignContent`. Fixture `block_align_content_overflow_safe`.

A block container's `align-content` (css-align-3 §5.4) without an overflow keyword keeps content
that overflows the container at its start in Chrome, as `safe` would; `unsafe center` centers it.
Taffy centers (or end-aligns) it, moving it above the container.

## `space-between` of overflowing items in a reversed flex container

Census category `SpaceBetween`. Fixture `flex_space_between_overflow_reversed`.

`space-between` falls back to `flex-start` when there is no free space (css-align-3 §4.3); in a
`row-reverse` or `column-reverse` container that is the end edge, so overflowing items stick out
past the start edge (Chrome). Taffy falls back to `start` and lets them overflow at the end.

## Sizing keywords on a block root

Census category `RootSizingKeywords`. Fixtures `root_width_fit_content`, `root_height_stretch`.

A block root's `width: fit-content` (or `min-content`, `max-content`) shrinks it to its content,
and `stretch` fills the viewport less the margins (css-sizing-3 §3.1), as for any block-level
box. Taffy takes them as `auto`: the root fills the viewport's width and takes its content
height.

## An overflowing line of a `wrap-reverse` flex container

Census category `WrapReverse`. Fixture `flex_wrap_reverse_overflowing_line`.

`stretch` (and `normal`) distribute nothing once the lines are stretched; space left over, or
overflow, is placed as for `flex-start` (css-align-3 §4.3), which in a `wrap-reverse` container
is the bottom: a line taller than the container overflows past its top (Chrome). Taffy keeps it
at the top.

## Percentages inside a column flex container of content-based width

Census categories `PercentPadding`, `PercentMargins` or `unexplained` (other percentages).
Fixture `flex_column_width_before_items`.

A column flex container whose width comes from its content (here a flex item that is not
stretched) has its width found first and its items laid out at it (Chrome), so their percentage
padding, margins and widths resolve against that width. Taffy measures the container's height
with its items' percentages resolved against nothing, and keeps that height.

## A grid item's content-sized height below its min width

Census category `unexplained` (seeds 636, 694, 700, 720, 981). Fixture
`grid_item_content_height_at_min_width`.

A grid item's `fit-content` (or other content-based) height is its content's height at the
item's used width, after `min-width` and `max-width` (css-sizing-3 §5.2). Taffy measures it at
the preferred width, here `0%`, so a percentage margin inside resolves against zero.

## Percentage tracks of a grid with a min size and a content-based size

Census category `PercentTracks`. Fixture `grid_percentage_track_intrinsic_with_min_width`.

While a grid container's size is found from its tracks, its percentage tracks behave as `auto`,
and resolve against the size found (css-grid-1 §7.2.1). Taffy resolves them against the
container's min size while sizing it, so a grid whose content is wider than its min-width is
sized at the min-width.

## A block root's min and max width

Census category `RootMinMaxWidth`. Fixture `root_max_width`.

A block root fills the viewport's width held between its min and max widths (CSS 2.2 §10.4).
Taffy fills the viewport whatever its `max-width`.

## Intrinsic and shrink-to-fit widths of row flex containers

Mostly census category `unexplained` (no feature to strip). Fixtures `flex_row_shrink_to_fit`,
`flex_intrinsic_size_inflexible_items`, `flex_intrinsic_size_keywords_and_scroll_items`,
`root_flex_width_max_content`.

- A single-line row flex container of `auto` width in definite space (absolutely positioned, an
  unstretched item) is shrink-to-fit (CSS 2.2 §10.3.5): no wider than the space unless its
  min-content width is. Taffy takes its items' added hypothetical sizes, whatever the space.
- Its intrinsic contributions (css-flexbox-1 §9.9.1) keep an item that cannot shrink (grow) at
  no less (more) than its flex base size, count an item's `width: max-content` (or another
  content keyword) at that size, and a scroll container item at its preferred width; taffy
  counts the content, the min-content size, and the flex base size there.
- A flex or grid root's sizing keyword is resolved as a block root's is; taffy ignores it.

Chrome itself does not use the flex base size in a shrink-to-fit width (an empty item of
`flex-basis: 197px` in an absolutely positioned row container gives it width 0, while
`width: max-content` gives it 197px); cephal follows the latter (§9.9.1) in both.

## Percentages inside a grid item of content-keyword height

Census categories `PercentGaps` or `unexplained`. Fixture
`grid_item_content_keyword_height_indefinite`.

A grid item's `height: fit-content` (or `min-content`, `max-content`) comes from its content, so
it is not definite for the percentages inside (CSS 2.2 §10.5): percentage heights behave as
`auto`, a percentage row gap is zero. Taffy resolves them against the height measured.

## Shrink-to-fit width of a grid container

Census category `unexplained`. Fixture `grid_shrink_to_fit`.

A grid container of automatic width in shrink-to-fit space (absolutely positioned, a
`fit-content` parent) is no wider than the space unless its min-content width is (CSS 2.2
§10.3.5); taffy gives it its max-content width.

## A flex item's cross size from its aspect ratio

Census category `unexplained`. Fixture `flex_item_cross_size_from_aspect_ratio`.

A flex item with an aspect ratio and an automatic cross size takes its cross size from its used
main size through the ratio, or from its content where that is larger (its automatic minimum,
css-sizing-4 §5.2.1). Taffy measures the content only, so an empty item of `aspect-ratio: 0.5`
grown to 26px is 0px tall instead of 52px.

## Content taller than a block's aspect-ratio height

Census category `unexplained`. Fixture `block_aspect_ratio_automatic_minimum`.

The automatic minimum height of a box whose height comes from its width through `aspect-ratio`
is its content height, capped by its max-height, unless it is a scroll container or has a
min-height (css-sizing-4 §5.2.1). Min and max sizes transfer through the ratio only to the axis
the ratio sizes, so a definite height stays when max-width narrows the box. Taffy keeps the
ratio's height in both cases.

## The root (both engines, by design)

Not a divergence between the engines, but where both differ from a browser's root element,
which is a block-level box in the initial containing block: an in-flow root's `auto` margins
count as zero, and an in-flow flex or grid root of `auto` width takes the width its layout
algorithm gives it instead of filling the viewport. When the viewport height is indefinite, the
initial containing block is the root's padding box vertically, and an out-of-flow root is laid
out in flow. The Chrome fixtures `to_html` writes pin these (or report them as not expressible).
