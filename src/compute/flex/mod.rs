//! Flexbox layout (css-flexbox-1 §9, plus `flex-wrap: balance`).

mod align;
mod axis;
mod balance;
mod base_size;
mod cross;
mod final_pass;
mod flexible;
mod fused;
mod lines;
mod main_size;

#[doc(hidden)]
pub use fused::set_fused_flex;
use axis::{Column, FlexAxisRect, FlexAxisSize, FlexAxisSum, FlexDir, Row};

use crate::compute::LayoutTreeExt;
use crate::compute::scratch::Scratch;
use crate::geometry::{AbsoluteAxis, AvailableSpace, Line, MaybeMath, Point, Rect, Size};
use crate::style::{
    AlignContent, AlignItems, AlignSelf, BoxSizing, Contain, Dimension, Direction, JustifyContent,
    Overflow,
};
use crate::tree::{
    Baselines, CacheAccess, Layout, LayoutInput, LayoutOutput, LayoutTree, NodeId, RequestedAxis, RunMode, SizingMode,
};

pub(super) struct FlexItem {
    node: NodeId,
    order: u32,
    size: Size<Option<f32>>,
    size_style: Size<Dimension>,
    min_size: Size<Option<f32>>,
    max_size: Size<Option<f32>>,
    aspect_ratio: Option<f32>,
    align_self: AlignSelf,
    overflow: Point<Overflow>,
    contain: Contain,
    scrollbar_width: f32,
    flex_shrink: f32,
    flex_grow: f32,
    flex_basis_is_definite: bool,
    resolved_minimum_main_size: f32,
    margin: Rect<f32>,
    margin_is_auto: Rect<bool>,
    padding: Rect<f32>,
    border: Rect<f32>,
    flex_basis: f32,
    inner_flex_basis: f32,
    violation: f32,
    frozen: bool,
    content_flex_fraction: f32,
    hypothetical_inner_size: Size<f32>,
    hypothetical_outer_size: Size<f32>,
    target_size: Size<f32>,
    outer_target_size: Size<f32>,
    baseline: f32,
    /// The last baseline, from the final pass only.
    last_baseline: f32,
    offset_main: f32,
    offset_cross: f32,
}

impl FlexItem {
    #[inline]
    fn participates_in_baseline_alignment(&self, dir: impl FlexDir) -> bool {
        self.align_self.keyword == crate::style::AlignItemsKeyword::Baseline && !self.margin_is_auto.cross_start(dir) && !self.margin_is_auto.cross_end(dir)
    }
}

/// A run of items, as an index range into the item list.
pub(super) struct FlexLine {
    start: usize,
    end: usize,
    cross_size: f32,
    offset_cross: f32,
}

impl FlexLine {
    #[inline]
    fn items<'a>(&self, items: &'a [FlexItem]) -> &'a [FlexItem] {
        &items[self.start..self.end]
    }
    #[inline]
    fn items_mut<'a>(&self, items: &'a mut [FlexItem]) -> &'a mut [FlexItem] {
        &mut items[self.start..self.end]
    }
    #[inline]
    fn len(&self) -> usize {
        self.end - self.start
    }
}

pub(super) struct AlgoConstants<D> {
    dir: D,
    layout_direction: Direction,
    is_row: bool,
    is_column: bool,
    is_wrap: bool,
    is_wrap_reverse: bool,
    is_balance: bool,
    line_count: Option<u16>,
    min_size: Size<Option<f32>>,
    max_size: Size<Option<f32>>,
    border: Rect<f32>,
    content_box_inset: Rect<f32>,
    scrollbar_gutter: Point<f32>,
    is_scroll_container: bool,
    gap: Size<f32>,
    align_items: AlignItems,
    align_content: AlignContent,
    justify_content: Option<JustifyContent>,
    node_outer_size: Size<Option<f32>>,
    node_inner_size: Size<Option<f32>>,
    known_main_size_is_definite: bool,
    has_definite_main_size: bool,
    has_definite_cross_size: bool,
    cross_axis_available_space_is_definite: bool,
    container_size: Size<f32>,
    inner_container_size: Size<f32>,
    /// The inner cross size a stretched item takes while the main sizes are found, when the
    /// container's own cross size is indefinite. See [`base_size::indefinite_stretch_cross`].
    indefinite_stretch_cross: Option<f32>,
    /// The basis items' percentage insets resolve against: the inner size when the items were
    /// generated, before an indefinite main size is found.
    inset_basis: Size<Option<f32>>,
    /// The container was given a height its items' percentages do not resolve against (a row
    /// flex item's that is not stretched, css-flexbox-1 §9.8); a width always resolves them.
    cross_size_is_indefinite: bool,
}

impl<D: FlexDir> AlgoConstants<D> {
    /// The size the items' percentages resolve against: the inner size, less an indefinite cross
    /// size.
    #[inline]
    fn percent_basis(&self) -> Size<Option<f32>> {
        let basis = if self.cross_size_is_indefinite { self.node_inner_size.with_cross(self.dir, None) } else { self.node_inner_size };
        // Scrollbars wider than the box leave no room, not negative room.
        basis.maybe_max(Size::ZERO.map(Some))
    }
    /// Cross space per line when `flex-line-count` splits it.
    #[inline]
    fn divided_cross_space(&self, cross_available_space: f32) -> f32 {
        if let Some(n) = self.line_count
            && n > 1
        {
            let n = n as f32;
            return (cross_available_space - (n - 1.0) * self.gap.cross(self.dir)) / n;
        }
        cross_available_space
    }
}

pub fn compute_flexbox_layout<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    inputs: LayoutInput,
) -> LayoutOutput {
    let LayoutInput { known_dimensions, parent_size, run_mode, .. } = inputs;
    let style = tree.style(node);
    let calc = |id, basis| tree.resolve_calc(id, basis);
    let contain = style.contain;
    let aspect_ratio = style.aspect_ratio;
    let padding = style.padding.map(|p| p.resolve_or_zero(parent_size.width, &calc));
    let border = style.border.map(|b| b.resolve_or_zero(parent_size.width, &calc));
    let padding_border_sum = padding.sum_axes() + border.sum_axes();
    let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { padding_border_sum } else { Size::ZERO };
    let resolve = |s: Size<crate::style::Length>| crate::compute::block::resolve_size(s, parent_size, &calc);
    let min_size = resolve(style.min_size.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment);
    let max_size = resolve(style.max_size.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment);
    let clamped_style_size = if inputs.sizing_mode == SizingMode::InherentSize {
        resolve(style.size.map(|v| v.raw()))
            .maybe_apply_aspect_ratio(aspect_ratio)
            .maybe_add(box_sizing_adjustment)
            .maybe_clamp(min_size, max_size)
    } else {
        Size::NONE
    };
    let min_max_definite_size = min_size.zip_map(max_size, |min, max| match (min, max) {
        (Some(min), Some(max)) if max <= min => Some(min),
        _ => None,
    });
    let styled_known =
        known_dimensions.or(min_max_definite_size.or(clamped_style_size).maybe_max(padding_border_sum.map(Some)));
    // A known axis transfers through `aspect-ratio`, as in block layout.
    let transferred = styled_known.transfer_aspect_ratio(aspect_ratio, min_size, max_size, padding_border_sum);

    let known_dimensions_are_definite =
        inputs.known_dimensions_are_definite.zip_map(known_dimensions, |definite, known| definite || known.is_none());
    // A transferred axis is as definite as the axis it came from.
    let known_dimensions_are_definite = Size {
        width: if styled_known.width.is_none() && transferred.width.is_some() {
            known_dimensions_are_definite.height
        } else {
            known_dimensions_are_definite.width
        },
        height: if styled_known.height.is_none() && transferred.height.is_some() {
            known_dimensions_are_definite.width
        } else {
            known_dimensions_are_definite.height
        },
    };
    let styled_known = transferred;

    if run_mode == RunMode::ComputeSize {
        if let Size { width: Some(width), height: Some(height) } = styled_known {
            return LayoutOutput::from_outer_size(Size { width, height });
        }
        if inputs.axis == RequestedAxis::Horizontal
            && let Some(width) = styled_known.width
        {
            return LayoutOutput::from_outer_size(Size { width, height: 0.0 });
        }
    }

    let mut inputs = LayoutInput { known_dimensions: styled_known, known_dimensions_are_definite, ..inputs };
    let dir = tree.style(node).flex_direction;
    // A column container of unknown width finds its width first and lays its items out at it,
    // so their percentages resolve against it (as Chrome).
    if !dir.is_row() && styled_known.width.is_none() && !(run_mode == RunMode::ComputeSize && inputs.axis == RequestedAxis::Horizontal) {
        let width_question = LayoutInput { run_mode: RunMode::ComputeSize, axis: RequestedAxis::Horizontal, ..inputs };
        let width = compute_preliminary(tree, node, width_question, Column(dir.is_reverse())).size.width;
        inputs.known_dimensions.width = Some(width);
        inputs.known_dimensions_are_definite.width = true;
    }
    let mut output = if dir.is_row() {
        compute_preliminary(tree, node, inputs, Row(dir.is_reverse()))
    } else {
        compute_preliminary(tree, node, inputs, Column(dir.is_reverse()))
    };
    if contain.suppresses_baseline() {
        output.baselines = Baselines::NONE;
    }
    output
}

/// A single-line row container of unknown width in definite space is shrink-to-fit (CSS 2.2
/// §10.3.5, css-sizing-3 §5.1): its max-content width, here found as its content's main size, but
/// no wider than the space unless its min-content width is. Narrows `c`'s sizes to that.
fn shrink_row_to_fit<D: FlexDir, T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    inputs: &LayoutInput,
    available_space: Size<AvailableSpace>,
    c: &mut AlgoConstants<D>,
) {
    let dir = c.dir;
    let AvailableSpace::Definite(space) = available_space.main(dir) else { return };
    if !dir.is_row() || c.is_wrap || inputs.known_dimensions.width.is_some() || c.inner_container_size.main(dir) <= space {
        return;
    }
    let min_content = tree.measure_child_size(
        node,
        Size { width: None, height: inputs.known_dimensions.height },
        inputs.parent_size,
        Size { width: AvailableSpace::MinContent, height: inputs.available_space.height },
        inputs.sizing_mode,
        AbsoluteAxis::Horizontal,
        inputs.vertical_margins_are_collapsible,
    );
    let inset = c.content_box_inset.main_axis_sum(dir);
    let outer = (space + inset).max(min_content).maybe_clamp(c.min_size.main(dir), c.max_size.main(dir)).max(inset);
    if outer < c.container_size.main(dir) {
        c.container_size.set_main(dir, outer);
        c.inner_container_size.set_main(dir, outer - inset);
    }
}

fn compute_preliminary<T: LayoutTree + CacheAccess + ?Sized, D: FlexDir>(
    tree: &mut T,
    node: NodeId,
    inputs: LayoutInput,
    dir: D,
) -> LayoutOutput {
    let LayoutInput { known_dimensions, parent_size, available_space, run_mode, .. } = inputs;
    let mut constants =
        compute_constants(tree, node, dir, known_dimensions, inputs.known_dimensions_are_definite, parent_size, available_space);
    #[cfg(feature = "stats")]
    if constants.is_wrap {
        crate::compute::stats::bump(&crate::compute::stats::FLEX_WRAP);
    }
    if fused::enabled() {
        if constants.is_wrap_reverse || constants.is_balance {
            #[cfg(feature = "stats")]
            count_miss(tree, None);
        } else {
            match fused::compute(tree, node, inputs, &mut constants) {
                Ok(output) => {
                    #[cfg(feature = "stats")]
                    crate::compute::stats::bump(&crate::compute::stats::FLEX_FUSED);
                    return output;
                }
                #[cfg(feature = "stats")]
                Err(miss) => count_miss(tree, Some(miss)),
                #[cfg(not(feature = "stats"))]
                Err(_) => {}
            }
        }
    }
    let (mut items, has_other_children) = generate_anonymous_flex_items(tree, node, &constants);
    let available_space = determine_available_space(known_dimensions, available_space, &constants);
    // Line breaking and the intrinsic main size of a multi-line container tell min-content from
    // max-content main space; everything else only passes the kind on to the items' questions.
    if constants.is_wrap && !available_space.main(dir).is_definite() {
        tree.note_observed(crate::compute::observed_bit(dir.main_axis()));
    }
    constants.indefinite_stretch_cross = base_size::indefinite_stretch_cross(tree, &constants, available_space, &items);
    base_size::determine_flex_base_size(tree, &constants, available_space, &mut items);
    let mut lines = if constants.is_balance {
        lines::collect_balanced_flex_lines(&constants, available_space, &items)
    } else {
        lines::collect_flex_lines(&constants, available_space, &items)
    };

    let dir = constants.dir;
    if let Some(inner_main_size) = constants.node_inner_size.main(dir) {
        let outer_main_size = inner_main_size + constants.content_box_inset.main_axis_sum(dir);
        constants.inner_container_size.set_main(dir, inner_main_size);
        constants.container_size.set_main(dir, outer_main_size);
    } else {
        main_size::determine_container_main_size(tree, available_space, &mut lines, &mut items, &mut constants);
        shrink_row_to_fit(tree, node, &inputs, available_space, &mut constants);
        constants.node_inner_size.set_main(dir, Some(constants.inner_container_size.main(dir)));
        constants.node_outer_size.set_main(dir, Some(constants.container_size.main(dir)));
        // A percentage column gap resolves against the now known width; a percentage row gap
        // against an indefinite height stays zero.
        if dir.is_row() {
            let style = tree.style(node);
            let calc = |id, basis| tree.resolve_calc(id, basis);
            let inner = constants.inner_container_size.main(dir);
            let new_gap = style.gap.main(dir).resolve(Some(inner), &calc).unwrap_or(0.0);
            constants.gap.set_main(dir, new_gap);
        }
    }

    // The main size is all a main-axis size question needs; the other axis of a single-axis
    // answer is unspecified.
    if run_mode == RunMode::ComputeSize && inputs.axis == RequestedAxis::from(dir.main_axis()) {
        return LayoutOutput::from_outer_size(constants.container_size);
    }

    for line in &lines {
        flexible::resolve_flexible_lengths(line.items_mut(&mut items), &constants);
    }
    // A single line of known cross size has that size whatever its items measure.
    let fixed_line_cross = if constants.is_wrap { None } else { known_dimensions.cross(dir) };
    for line in &lines {
        cross::determine_hypothetical_cross_size(tree, line.items_mut(&mut items), &constants, available_space, fixed_line_cross);
    }
    cross::calculate_children_base_lines(tree, run_mode, known_dimensions, available_space, &lines, &mut items, &constants);
    cross::calculate_cross_size(&mut lines, &items, known_dimensions, &constants);
    cross::handle_align_content_stretch(&mut lines, known_dimensions, &constants);
    cross::determine_used_cross_size(tree, &lines, &mut items, &constants);
    align::distribute_remaining_free_space(&lines, &mut items, &constants);
    align::resolve_cross_axis_auto_margins(&lines, &mut items, &constants);
    let total_line_cross_size = cross::determine_container_cross_size(&lines, known_dimensions, &mut constants);

    if run_mode == RunMode::ComputeSize {
        return LayoutOutput::from_outer_size(constants.container_size);
    }

    align::align_flex_lines_per_align_content(&mut lines, &constants, total_line_cross_size);
    let mut through = Vec::new();
    let inflow_overflow_rect = final_pass::final_layout_pass(tree, &lines, &mut items, &mut through, &constants);
    // Absolutely positioned and hidden children were found while generating the items.
    let (absolute_overflow_rect, oof) = finish_children(tree, node, &constants, has_other_children, through);

    // The container's baseline comes from its first line's first baseline-participating item.
    let first_line = if constants.is_wrap_reverse { lines.last() } else { lines.first() };
    let first_vertical_baseline = first_line.and_then(|line| {
        let line_items = line.items(&items);
        if constants.is_column {
            let item = if dir.is_reverse() { line_items.last() } else { line_items.first() };
            item.map(|c| c.baseline)
        } else {
            line_items
                .iter()
                .find(|item| item.participates_in_baseline_alignment(dir))
                .or_else(|| line_items.first())
                .map(|c| c.baseline)
        }
    });

    // No item takes part in last-baseline alignment, so the last baseline is the endmost item's on
    // the last line.
    let last_line = if constants.is_wrap_reverse { lines.first() } else { lines.last() };
    let last_vertical_baseline = last_line.and_then(|line| {
        let line_items = line.items(&items);
        let item = if constants.is_column && dir.is_reverse() { line_items.first() } else { line_items.last() };
        item.map(|c| c.last_baseline)
    });

    let baselines = Baselines { first: first_vertical_baseline, last: last_vertical_baseline };
    let mut output = LayoutOutput::from_sizes_and_baselines(constants.container_size, Rect::ZERO, baselines)
        .with_overflow(inflow_overflow_rect, absolute_overflow_rect);
    output.oof = oof;
    output
}

/// Counts a container the fused path does not take, by reason: `None` for `wrap-reverse` or
/// `balance`.
#[cfg(feature = "stats")]
fn count_miss<T: LayoutTree + ?Sized>(tree: &T, miss: Option<fused::Miss>) {
    use crate::compute::stats::*;
    bump(&FLEX_NOT_FUSED);
    bump(match miss {
        None => &FLEX_MISS_WRAP,
        Some(fused::Miss::NoPlan) => &FLEX_MISS_NO_PLAN,
        Some(fused::Miss::Baseline) => &FLEX_MISS_BASELINE,
        Some(fused::Miss::Empty) => &FLEX_MISS_EMPTY,
        Some(fused::Miss::NotPlain(item)) => {
            let style = tree.style(item);
            let (basis, m) = (style.flex_basis.raw(), style.margin);
            if style.aspect_ratio.is_some() {
                &FLEX_MISS_ASPECT_RATIO
            } else if m.left.is_auto() || m.right.is_auto() || m.top.is_auto() || m.bottom.is_auto() {
                &FLEX_MISS_AUTO_MARGIN
            } else if basis.uses_percentage() || basis.is_content() || basis.is_sizing_keyword() {
                &FLEX_MISS_BASIS
            } else {
                &FLEX_MISS_SIZE_KEYWORD
            }
        }
    });
}

/// Lays out the hidden children, and ends the final layout's out-of-flow work (see
/// [`crate::compute::oof`]) given the items passing candidates through, in `entries`; returns
/// the overflow of the out-of-flow boxes laid out and the bits passed on.
fn finish_children<T: LayoutTree + CacheAccess + ?Sized, D: FlexDir>(
    tree: &mut T,
    node: NodeId,
    constants: &AlgoConstants<D>,
    has_other_children: bool,
    mut entries: Vec<crate::compute::oof::OofEntry>,
) -> (Rect<f32>, u8) {
    if has_other_children {
        let hidden: Scratch<(usize, NodeId)> =
            Scratch::collect(tree.children(node).enumerate().filter(|(_, c)| tree.style(*c).generates_no_box()));
        for &(order, child) in &hidden {
            tree.set_unrounded_layout(child, &Layout::with_order(order as u32));
            tree.perform_child_layout(child, Size::NONE, Size::NONE, Size::MAX_CONTENT, SizingMode::ContentSize, Line::FALSE);
        }
        if tree.speculation() == 0 {
            out_of_flow_seeds(tree, node, constants, &mut entries);
        }
    }
    if entries.is_empty() {
        crate::compute::oof::finish_none(tree, node);
        return (Rect::ZERO, 0);
    }
    if tree.speculation() > 0 {
        return (Rect::ZERO, 0);
    }
    // Items are placed in visual order; the record is in document order.
    entries.sort_by_key(|e| e.order());
    let c = constants;
    let rtl = c.layout_direction == Direction::Rtl;
    let inset = c.border
        + Rect {
            left: if rtl { c.scrollbar_gutter.x } else { 0.0 },
            right: if rtl { 0.0 } else { c.scrollbar_gutter.x },
            top: 0.0,
            bottom: c.scrollbar_gutter.y,
        };
    let geometry = crate::compute::oof::OofGeometry {
        area_size: c.container_size - inset.sum_axes(),
        area_offset: Point { x: inset.left, y: inset.top },
        direction: c.layout_direction,
        overflow: tree.style(node).overflow,
        #[cfg(feature = "grid")]
        grid: None,
    };
    crate::compute::oof::finish(tree, node, entries, || geometry)
}

/// The static positions of the out-of-flow children (css-flexbox-1 §4.1): the content box in both
/// axes, aligned per `justify-content` and the child's `align-self`.
fn out_of_flow_seeds<T: LayoutTree + ?Sized, D: FlexDir>(
    tree: &T,
    node: NodeId,
    c: &AlgoConstants<D>,
    entries: &mut Vec<crate::compute::oof::OofEntry>,
) {
    use crate::compute::oof::{AxisStaticAlign, AxisStaticEdge, AxisStaticPosition, OofEntry, position_bit};
    use crate::style::{AlignContentKeyword as C, AlignItemsKeyword as I};
    let dir = c.dir;
    let rtl = c.layout_direction == Direction::Rtl;
    let main_is_rtl = c.is_row && rtl;
    let cross_is_rtl = !c.is_row && rtl;
    let main_reversed = dir.is_reverse() ^ main_is_rtl;
    let cross_reversed = c.is_wrap_reverse ^ cross_is_rtl;
    let main_area = Line { start: c.content_box_inset.main_start(dir), end: c.container_size.main(dir) - c.content_box_inset.main_end(dir) };
    let cross_area = Line { start: c.content_box_inset.cross_start(dir), end: c.container_size.cross(dir) - c.content_box_inset.cross_end(dir) };
    // `stretch` and `normal` justify as `flex-start`. Chrome applies no `safe` fallback here.
    let justify = c.justify_content.map(|j| j.keyword);
    let main_start_position = match justify {
        Some(C::Start) => !main_is_rtl,
        Some(C::End) => main_is_rtl,
        _ => true,
    };
    let main_edge = match (justify, main_reversed) {
        (None | Some(C::SpaceBetween | C::Stretch | C::FlexStart), false) | (Some(C::FlexEnd), true) => AxisStaticEdge::Start,
        (Some(C::Start | C::End), _) => {
            if main_start_position { AxisStaticEdge::Start } else { AxisStaticEdge::End }
        }
        (Some(C::FlexEnd), false) | (None | Some(C::FlexStart | C::Stretch | C::SpaceBetween), true) => AxisStaticEdge::End,
        (Some(C::SpaceEvenly | C::SpaceAround | C::Center), _) => AxisStaticEdge::Center,
    };
    // `start`, `end` and `baseline` (whose fallback is `start`) follow the writing mode;
    // `flex-start`, `flex-end` and `stretch` the flex lines.
    let cross_edge_for = |keyword: I| {
        let start_position = match keyword {
            I::Start | I::Baseline => !cross_is_rtl,
            I::End => cross_is_rtl,
            _ => true,
        };
        match (keyword, cross_reversed) {
            (I::Start | I::End | I::Baseline, _) => {
                if start_position { AxisStaticEdge::Start } else { AxisStaticEdge::End }
            }
            (I::Stretch | I::FlexStart, false) | (I::FlexEnd, true) => AxisStaticEdge::Start,
            (I::Stretch | I::FlexStart, true) | (I::FlexEnd, false) => AxisStaticEdge::End,
            (I::Center, _) => AxisStaticEdge::Center,
            (I::SelfStart | I::SelfEnd, _) => unreachable!("resolved against the directions"),
        }
    };
    for (index, child) in tree.children(node).enumerate() {
        let style = tree.style(child);
        if style.generates_no_box() || !style.position.is_out_of_flow() {
            continue;
        }
        let align_self = style.align_self.unwrap_or(c.align_items).resolve_self_relative(style.direction, c.layout_direction, c.is_column);
        let cross = AxisStaticPosition {
            area: cross_area,
            align: AxisStaticAlign {
                keyword: cross_edge_for(align_self.keyword),
                safety: align_self.safety,
                fallback: cross_edge_for(crate::compute::common::alignment::resolve_self_alignment_safety(align_self, true)),
            },
        };
        let main = AxisStaticPosition { area: main_area, align: AxisStaticAlign::from_keyword(main_edge) };
        let static_position = if c.is_row { Point { x: main, y: cross } } else { Point { x: cross, y: main } };
        entries.push(OofEntry::Seed { node: child, order: index as u32, bit: position_bit(style.position), static_position });
    }
}

fn compute_constants<T: LayoutTree + ?Sized, D: FlexDir>(
    tree: &T,
    node: NodeId,
    dir: D,
    known_dimensions: Size<Option<f32>>,
    known_dimensions_are_definite: Size<bool>,
    parent_size: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
) -> AlgoConstants<D> {
    let style = tree.style(node);
    let calc = |id, basis| tree.resolve_calc(id, basis);
    let is_row = dir.is_row();
    let is_column = dir.is_column();
    let flex_wrap = style.flex_wrap;
    let is_wrap = flex_wrap.is_multi_line();
    let is_wrap_reverse = flex_wrap.is_reverse();
    let is_balance = flex_wrap.is_balance();
    let line_count = if is_wrap { Some(style.flex_line_count.max(1)) } else { None };
    let aspect_ratio = style.aspect_ratio;
    let padding = style.padding.map(|p| p.resolve_or_zero(parent_size.width, &calc));
    let border = style.border.map(|b| b.resolve_or_zero(parent_size.width, &calc));
    let padding_border_sum = padding.sum_axes() + border.sum_axes();
    let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { padding_border_sum } else { Size::ZERO };
    let align_items = style.align_items.unwrap_or(AlignItems::STRETCH);
    let align_content = style.align_content.unwrap_or(AlignContent::STRETCH);
    let justify_content = style.justify_content;
    let layout_direction = style.direction;
    let scrollbar_gutter = style.overflow.transpose().map(|o| if o == Overflow::Scroll { style.scrollbar_width } else { 0.0 });
    let is_scroll_container = style.is_scroll_container();
    let mut content_box_inset = padding + border;
    content_box_inset.bottom += scrollbar_gutter.y;
    match layout_direction {
        Direction::Ltr => content_box_inset.right += scrollbar_gutter.x,
        Direction::Rtl => content_box_inset.left += scrollbar_gutter.x,
    }
    let node_outer_size = known_dimensions;
    let node_inner_size = node_outer_size.maybe_sub(content_box_inset.sum_axes());
    let known_main_size_is_definite = known_dimensions_are_definite.main(dir);
    let has_definite_main_size = known_main_size_is_definite && known_dimensions.main(dir).is_some();
    let has_definite_cross_size = known_dimensions_are_definite.cross(dir) && known_dimensions.cross(dir).is_some();
    let cross_axis_available_space_is_definite =
        has_definite_cross_size || matches!(available_space.cross(dir), AvailableSpace::Definite(_));
    let cross_size_is_indefinite = is_row && known_dimensions.height.is_some() && !known_dimensions_are_definite.height;
    let percent_basis =
        if cross_size_is_indefinite { node_inner_size.with_cross(dir, None) } else { node_inner_size }.maybe_max(Size::ZERO.map(Some));
    let gap_basis = if is_column && !known_main_size_is_definite { percent_basis.with_main(dir, None) } else { percent_basis };
    let gap_basis = gap_basis.or(Size::ZERO.map(Some));
    let gap = Size {
        width: style.gap.width.resolve_or_zero(gap_basis.width, &calc),
        height: style.gap.height.resolve_or_zero(gap_basis.height, &calc),
    };
    let resolve = |s: Size<crate::style::Length>| crate::compute::block::resolve_size(s, parent_size, &calc);
    AlgoConstants {
        dir,
        layout_direction,
        is_row,
        is_column,
        is_wrap,
        is_wrap_reverse,
        is_balance,
        line_count,
        min_size: resolve(style.min_size.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment),
        max_size: resolve(style.max_size.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment),
        border,
        gap,
        content_box_inset,
        scrollbar_gutter,
        is_scroll_container,
        align_items,
        align_content,
        justify_content,
        node_outer_size,
        node_inner_size,
        known_main_size_is_definite,
        has_definite_main_size,
        has_definite_cross_size,
        cross_axis_available_space_is_definite,
        container_size: Size::ZERO,
        inner_container_size: Size::ZERO,
        indefinite_stretch_cross: None,
        inset_basis: percent_basis,
        cross_size_is_indefinite,
    }
}

/// The in-flow items, and whether any child was left out (hidden or out of flow).
fn generate_anonymous_flex_items<D: FlexDir, T: LayoutTree + ?Sized>(tree: &T, node: NodeId, constants: &AlgoConstants<D>) -> (Scratch<FlexItem>, bool) {
    let calc = |id, basis| tree.resolve_calc(id, basis);
    // Percentages against an indefinite main size resolve to `None`.
    let percent_resolution_size = if constants.known_main_size_is_definite {
        constants.percent_basis()
    } else {
        constants.percent_basis().with_main(constants.dir, None)
    };
    let mut items = Scratch::with_capacity(tree.child_count(node));
    let mut skipped = false;
    for (index, child) in tree.children(node).enumerate() {
        let style = tree.style(child);
        if style.position.is_out_of_flow() || style.generates_no_box() {
            skipped = true;
            continue;
        }
        let aspect_ratio = style.aspect_ratio;
        // Lengths that resolve against nothing are the same against any basis.
        let (size, min_size, max_size, margin, padding, border) = match tree.style_plan(child) {
            Some(p) => (p.size, p.min_size, p.max_size, p.margin, p.padding, p.border),
            None => {
                let resolve = |s: Size<crate::style::Length>| crate::compute::block::resolve_size(s, percent_resolution_size, &calc);
                (
                    resolve(style.size.map(|v| v.raw())),
                    resolve(style.min_size.map(|v| v.raw())),
                    resolve(style.max_size.map(|v| v.raw())),
                    style.margin.map(|m| m.resolve_or_zero(constants.percent_basis().width, &calc)),
                    style.padding.map(|p| p.resolve_or_zero(constants.percent_basis().width, &calc)),
                    style.border.map(|b| b.resolve_or_zero(constants.percent_basis().width, &calc)),
                )
            }
        };
        let pb_sum = (padding + border).sum_axes();
        let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { pb_sum } else { Size::ZERO };
        items.push(FlexItem {
            node: child,
            order: index as u32,
            size: size.maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment),
            size_style: style.size,
            min_size: min_size.maybe_add(box_sizing_adjustment),
            max_size: max_size.maybe_add(box_sizing_adjustment),
            aspect_ratio,
            margin,
            margin_is_auto: style.margin.map(|m| m.is_auto()),
            padding,
            border,
            align_self: style.align_self.unwrap_or(constants.align_items).resolve_self_relative(
                style.direction,
                constants.layout_direction,
                constants.is_column,
            ),
            overflow: style.overflow,
            contain: style.contain,
            scrollbar_width: style.scrollbar_width,
            flex_grow: style.flex_grow,
            flex_shrink: style.flex_shrink,
            flex_basis_is_definite: false,
            flex_basis: 0.0,
            inner_flex_basis: 0.0,
            violation: 0.0,
            frozen: false,
            resolved_minimum_main_size: 0.0,
            hypothetical_inner_size: Size::ZERO,
            hypothetical_outer_size: Size::ZERO,
            target_size: Size::ZERO,
            outer_target_size: Size::ZERO,
            content_flex_fraction: 0.0,
            baseline: 0.0,
            last_baseline: 0.0,
            offset_main: 0.0,
            offset_cross: 0.0,
        });
    }
    (items, skipped)
}

/// Content-box available space.
#[inline]
fn determine_available_space<D: FlexDir>(
    known_dimensions: Size<Option<f32>>,
    outer: Size<AvailableSpace>,
    c: &AlgoConstants<D>,
) -> Size<AvailableSpace> {
    Size {
        width: match known_dimensions.width {
            Some(w) => AvailableSpace::Definite((w - c.content_box_inset.horizontal_axis_sum()).max(0.0)),
            None => outer.width.maybe_sub(c.content_box_inset.horizontal_axis_sum()).maybe_max(0.0),
        },
        height: match known_dimensions.height {
            Some(h) => AvailableSpace::Definite((h - c.content_box_inset.vertical_axis_sum()).max(0.0)),
            None => outer.height.maybe_sub(c.content_box_inset.vertical_axis_sum()).maybe_max(0.0),
        },
    }
}

/// Which of an item's known dimensions count as definite for its own percentage resolution.
#[inline]
fn item_known_dimension_definiteness<D: FlexDir>(c: &AlgoConstants<D>, item: &FlexItem) -> Size<bool> {
    let dir = c.dir;
    let main_is_definite = c.has_definite_main_size || item.flex_basis_is_definite;
    let has_cross_auto_margins = item.margin_is_auto.cross_start(dir) || item.margin_is_auto.cross_end(dir);
    let cross_size = item.size_style.cross(dir);
    let is_stretched =
        !has_cross_auto_margins && (cross_size.is_stretch() || (item.align_self.keyword == crate::style::AlignItemsKeyword::Stretch && cross_size.is_auto()));
    let cross_is_definite =
        is_stretched || item.size.cross(dir).is_some() || (!dir.is_row() && c.cross_axis_available_space_is_definite);
    Size::TRUE.with_main(dir, main_is_definite).with_cross(dir, cross_is_definite)
}

crate::compute::scratch::pooled!(FlexItem, FlexLine);
