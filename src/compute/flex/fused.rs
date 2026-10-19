//! The flex algorithm for a single line of plain items, fused into a few passes over a compact
//! item record.
//!
//! An item is plain when its lengths resolve without a parent size (it has a
//! [`StylePlan`](crate::compute::plan::StylePlan)) and it has no `aspect-ratio`, auto margin,
//! baseline alignment, sizing keyword, `content` basis or percentage basis. Each step below is the
//! general algorithm's with the branches such items cannot take removed: the same questions are
//! asked in the same order and batches, and every float is computed by the same expression.

use super::axis::{FlexAxisRect, FlexAxisSize, FlexAxisSum, FlexDir, abs_main, from_cross, sum_axis_gaps};
use super::flexible::{Flexible, resolve_flexible_lengths};
use super::{AlgoConstants, FlexLine, align, cross};
use crate::compute::common::alignment::{apply_alignment_fallback, compute_alignment_offset};
use crate::compute::common::overflow::{scrollable_overflow_contribution, union};
use crate::compute::scratch::Scratch;
use crate::compute::{LayoutTreeExt, Questions, size_request};
use crate::geometry::{AvailableSpace, Line, MaybeMath, Point, Rect, Size};
use crate::style::{AlignItemsKeyword, AlignSelf, BoxSizing, Contain, Direction, JustifyContent, Overflow};
use crate::tree::{
    Baselines, CacheAccess, ChildRequest, Layout, LayoutInput, LayoutOutput, LayoutTree, NodeId, RequestedAxis, RunMode,
    SizingMode,
};
use core::sync::atomic::{AtomicU8, Ordering::Relaxed};

/// 0 until first read, then 1 when the fused path runs and 2 when it does not.
static STATE: AtomicU8 = AtomicU8::new(0);

/// Turns the fused path on or off for the whole process, so tests can compare it with the general
/// algorithm. It starts on unless `CEPHAL_GENERIC_FLEX` is set.
#[doc(hidden)]
pub fn set_fused_flex(enabled: bool) {
    STATE.store(if enabled { 1 } else { 2 }, Relaxed);
}

#[inline]
pub(super) fn enabled() -> bool {
    match STATE.load(Relaxed) {
        1 => true,
        2 => false,
        _ => init(),
    }
}

#[cold]
fn init() -> bool {
    let on = std::env::var_os("CEPHAL_GENERIC_FLEX").is_none();
    let _ = STATE.compare_exchange(0, if on { 1 } else { 2 }, Relaxed, Relaxed);
    STATE.load(Relaxed) == 1
}

/// No question asked.
const NONE: u32 = u32::MAX;

/// Asks `request` of an item.
#[inline(always)]
fn ask<A: crate::compute::scratch::Pooled + Copy, T: LayoutTree + CacheAccess + ?Sized>(
    questions: &mut Questions<A>,
    tree: &mut T,
    cheap_leaf: bool,
    request: ChildRequest,
    read: impl FnOnce(&LayoutOutput) -> A,
) -> usize {
    if cheap_leaf { questions.ask_cheap_leaf(tree, request, read) } else { questions.ask(tree, request, read) }
}

pub(super) struct Item {
    node: NodeId,
    order: u32,
    align_self: AlignSelf,
    overflow: Point<Overflow>,
    contain: Contain,
    flex_basis_is_definite: bool,
    measures_min: bool,
    /// The line stretches the item to its cross size.
    stretched: bool,
    /// A cheap leaf, answered without dispatch.
    cheap_leaf: bool,
    frozen: bool,
    scrollbar_width: f32,
    /// Sizes after box sizing.
    size: Size<Option<f32>>,
    min_size: Size<Option<f32>>,
    max_size: Size<Option<f32>>,
    margin: Rect<f32>,
    /// `(padding + border).sum_axes()`.
    padding_border: Size<f32>,
    /// Padding's main-axis sum plus border's.
    padding_border_main: f32,
    flex_grow: f32,
    flex_shrink: f32,
    flex_basis: f32,
    inner_flex_basis: f32,
    resolved_minimum_main_size: f32,
    violation: f32,
    hypothetical_inner_size: Size<f32>,
    hypothetical_outer_main: f32,
    target_size: Size<f32>,
    outer_target_main: f32,
    offset_main: f32,
    offset_cross: f32,
    /// Indexes of the item's questions in the current batch.
    question: [u32; 2],
}

impl Flexible for Item {
    #[inline(always)]
    fn hypothetical_inner_main(&self, dir: impl FlexDir) -> f32 {
        self.hypothetical_inner_size.main(dir)
    }
    #[inline(always)]
    fn hypothetical_outer_main(&self, _: impl FlexDir) -> f32 {
        self.hypothetical_outer_main
    }
    #[inline(always)]
    fn flex_basis(&self) -> f32 {
        self.flex_basis
    }
    #[inline(always)]
    fn inner_flex_basis(&self) -> f32 {
        self.inner_flex_basis
    }
    #[inline(always)]
    fn flex_grow(&self) -> f32 {
        self.flex_grow
    }
    #[inline(always)]
    fn flex_shrink(&self) -> f32 {
        self.flex_shrink
    }
    #[inline(always)]
    fn margin_main_sum(&self, dir: impl FlexDir) -> f32 {
        self.margin.main_axis_sum(dir)
    }
    #[inline(always)]
    fn resolved_minimum_main_size(&self) -> f32 {
        self.resolved_minimum_main_size
    }
    #[inline(always)]
    fn max_main(&self, dir: impl FlexDir) -> Option<f32> {
        self.max_size.main(dir)
    }
    #[inline(always)]
    fn frozen(&self) -> bool {
        self.frozen
    }
    #[inline(always)]
    fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
    }
    #[inline(always)]
    fn target_main(&self, dir: impl FlexDir) -> f32 {
        self.target_size.main(dir)
    }
    #[inline(always)]
    fn set_target_main(&mut self, dir: impl FlexDir, v: f32) {
        self.target_size.set_main(dir, v);
    }
    #[inline(always)]
    fn outer_target_main(&self, _: impl FlexDir) -> f32 {
        self.outer_target_main
    }
    #[inline(always)]
    fn set_outer_target_main(&mut self, _: impl FlexDir, v: f32) {
        self.outer_target_main = v;
    }
    #[inline(always)]
    fn violation(&self) -> f32 {
        self.violation
    }
    #[inline(always)]
    fn set_violation(&mut self, v: f32) {
        self.violation = v;
    }
}

impl Item {
    #[inline]
    fn automatic_min_main(&self, dir: impl FlexDir) -> Option<f32> {
        let automatic_min: Size<Option<f32>> = Size {
            width: self.overflow.x.maybe_into_automatic_min_size(),
            height: self.overflow.y.maybe_into_automatic_min_size(),
        };
        self.min_size.or(automatic_min).main(dir)
    }

    /// See [`super::item_known_dimension_definiteness`].
    #[inline]
    fn known_dimension_definiteness<D: FlexDir>(&self, c: &AlgoConstants<D>) -> Size<bool> {
        let dir = c.dir;
        let main_is_definite = c.has_definite_main_size || self.flex_basis_is_definite;
        let cross_is_definite =
            self.stretched || self.size.cross(dir).is_some() || (!dir.is_row() && c.cross_axis_available_space_is_definite);
        Size::TRUE.with_main(dir, main_is_definite).with_cross(dir, cross_is_definite)
    }
}

/// The plain in-flow items and whether any child was left out, or `None` when an item is not
/// plain.
fn generate_items<D: FlexDir, T: LayoutTree + ?Sized>(tree: &T, node: NodeId, c: &AlgoConstants<D>) -> Option<(Scratch<Item>, bool)> {
    let dir = c.dir;
    let direct = tree.forwards_child_layout();
    let mut items: Scratch<Item> = Scratch::with_capacity(tree.child_count(node));
    let mut skipped = false;
    for (index, child) in tree.children(node).enumerate() {
        let style = tree.style(child);
        if style.position.is_out_of_flow() || style.generates_no_box() {
            skipped = true;
            continue;
        }
        let plan = tree.style_plan(child)?;
        let align_self =
            style.align_self.unwrap_or(c.align_items).resolve_self_relative(style.direction, c.layout_direction, c.is_column);
        if !plan.plain_flex_item || align_self.keyword == AlignItemsKeyword::Baseline {
            return None;
        }
        let padding_border = (plan.padding + plan.border).sum_axes();
        let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { padding_border } else { Size::ZERO };
        let size = plan.size.maybe_add(box_sizing_adjustment);
        let min_size = plan.min_size.maybe_add(box_sizing_adjustment);
        let max_size = plan.max_size.maybe_add(box_sizing_adjustment);
        let basis = plan.flex_basis.maybe_add(Some(box_sizing_adjustment.main(dir))).or(size.main(dir));
        let automatic_min: Size<Option<f32>> = Size {
            width: style.overflow.x.maybe_into_automatic_min_size(),
            height: style.overflow.y.maybe_into_automatic_min_size(),
        };
        items.push(Item {
            node: child,
            order: index as u32,
            align_self,
            overflow: style.overflow,
            contain: style.contain,
            flex_basis_is_definite: basis.is_some(),
            measures_min: min_size.or(automatic_min).main(dir).is_none(),
            stretched: align_self == AlignSelf::STRETCH && style.size.cross(dir).is_auto(),
            cheap_leaf: direct && {
                let meta = tree.query_meta(child);
                meta.cheap_leaf && !meta.hidden
            },
            frozen: false,
            scrollbar_width: style.scrollbar_width,
            size,
            min_size,
            max_size,
            margin: plan.margin,
            padding_border,
            padding_border_main: plan.padding.main_axis_sum(dir) + plan.border.main_axis_sum(dir),
            flex_grow: style.flex_grow,
            flex_shrink: style.flex_shrink,
            flex_basis: basis.unwrap_or(f32::NAN),
            inner_flex_basis: 0.0,
            resolved_minimum_main_size: 0.0,
            violation: 0.0,
            hypothetical_inner_size: Size::ZERO,
            hypothetical_outer_main: 0.0,
            target_size: Size::ZERO,
            outer_target_main: 0.0,
            offset_main: 0.0,
            offset_cross: 0.0,
            question: [NONE; 2],
        });
    }
    if items.is_empty() {
        return None;
    }
    Some((items, skipped))
}

/// Lays out `node` when its items are all plain; `None`, having asked nothing, otherwise.
pub(super) fn compute<D: FlexDir, T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    inputs: LayoutInput,
    c: &mut AlgoConstants<D>,
) -> Option<LayoutOutput> {
    let LayoutInput { known_dimensions, run_mode, .. } = inputs;
    let (mut items, has_other_children) = generate_items(tree, node, c)?;
    let dir = c.dir;
    let available_space = super::determine_available_space(known_dimensions, inputs.available_space, c);
    c.indefinite_stretch_cross = indefinite_stretch_cross(tree, c, available_space, &items);
    determine_flex_base_size(tree, c, available_space, &mut items);

    if let Some(inner_main_size) = c.node_inner_size.main(dir) {
        let outer_main_size = inner_main_size + c.content_box_inset.main_axis_sum(dir);
        c.inner_container_size.set_main(dir, inner_main_size);
        c.container_size.set_main(dir, outer_main_size);
    } else {
        determine_container_main_size(tree, available_space, &mut items, c);
        c.node_inner_size.set_main(dir, Some(c.inner_container_size.main(dir)));
        c.node_outer_size.set_main(dir, Some(c.container_size.main(dir)));
        let style = tree.style(node);
        let calc = |id, basis| tree.resolve_calc(id, basis);
        let inner = c.inner_container_size.main(dir);
        let new_gap = style.gap.main(dir).resolve(Some(inner), &calc).unwrap_or(0.0);
        c.gap.set_main(dir, new_gap);
    }

    if run_mode == RunMode::ComputeSize && inputs.axis == RequestedAxis::from(dir.main_axis()) {
        return Some(LayoutOutput::from_outer_size(c.container_size));
    }

    resolve_flexible_lengths(&mut items, c);
    // A single line of known cross size has that size whatever its items measure.
    let fixed_line_cross = known_dimensions.cross(dir);
    determine_hypothetical_cross_size(tree, &mut items, c, available_space, fixed_line_cross);
    let cross_size = calculate_cross_size(&items, known_dimensions, c);
    let mut lines = [FlexLine { start: 0, end: items.len(), cross_size, offset_cross: 0.0 }];
    cross::handle_align_content_stretch(&mut lines, known_dimensions, c);
    let line_cross_size = lines[0].cross_size;
    for item in items.iter_mut() {
        let target_cross = if item.stretched {
            let (min, max) = (item.min_size.cross(dir), item.max_size.cross(dir));
            (line_cross_size - item.margin.cross_axis_sum(dir)).max(0.0).maybe_clamp(min, max)
        } else {
            item.hypothetical_inner_size.cross(dir)
        };
        item.target_size.set_cross(dir, target_cross);
    }
    distribute_remaining_free_space(&mut items, c);
    for item in items.iter_mut() {
        let outer_target_cross = item.target_size.cross(dir) + item.margin.cross_axis_sum(dir);
        let free_space = line_cross_size - outer_target_cross;
        item.offset_cross = align::cross_axis_offset(item.align_self, free_space, c, || unreachable!("no baseline alignment"));
    }
    let total_line_cross_size = cross::determine_container_cross_size(&lines, known_dimensions, c);

    if run_mode == RunMode::ComputeSize {
        return Some(LayoutOutput::from_outer_size(c.container_size));
    }

    super::align::align_flex_lines_per_align_content(&mut lines, c, total_line_cross_size);
    let (inflow_overflow_rect, baselines) = final_layout_pass(tree, &lines[0], &items, c);
    let absolute_overflow_rect = if has_other_children { super::lay_out_other_children(tree, node, c) } else { Rect::ZERO };
    Some(
        LayoutOutput::from_sizes_and_baselines(c.container_size, Rect::ZERO, baselines)
            .with_overflow(inflow_overflow_rect, absolute_overflow_rect),
    )
}

/// See [`super::base_size::indefinite_stretch_cross`].
fn indefinite_stretch_cross<D: FlexDir, T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    c: &AlgoConstants<D>,
    available_space: Size<AvailableSpace>,
    items: &[Item],
) -> Option<f32> {
    let dir = c.dir;
    if !c.is_column || c.node_inner_size.cross(dir).is_some() {
        return None;
    }
    if !items.iter().any(|item| item.size.cross(dir).is_none() && item.align_self == AlignSelf::STRETCH) {
        return None;
    }
    let available_cross = available_space.cross(dir);
    let read_cross = |out: &LayoutOutput| out.size.get(dir.cross_axis());
    let mut questions: Questions<f32> = Questions::new(tree, items.len());
    for item in items.iter().filter(|item| item.size.cross(dir).is_none()) {
        let request = size_request(
            item.node,
            item.size.with_cross(dir, None),
            c.node_inner_size,
            Size::MAX_CONTENT.with_cross(dir, available_cross),
            SizingMode::ContentSize,
            dir.cross_axis().into(),
        );
        ask(&mut questions, tree, item.cheap_leaf, request, read_cross);
    }
    questions.finish(tree, read_cross);
    let mut measured = 0;
    let mut outer = 0.0f32;
    for item in items {
        let min = item.min_size.cross(dir);
        let max = item.max_size.cross(dir);
        let padding_border = item.padding_border.cross(dir);
        let inner = match item.size.cross(dir) {
            Some(size) => size,
            None => {
                measured += 1;
                questions.get(measured - 1)
            }
        };
        let size = inner.maybe_clamp(min, max).max(padding_border);
        outer = outer.max(size + item.margin.cross_axis_sum(dir));
    }
    let inset = c.content_box_inset.cross_axis_sum(dir);
    let cross = outer.maybe_clamp(c.min_size.cross(dir).maybe_sub(inset), c.max_size.cross(dir).maybe_sub(inset));
    Some(cross.max(0.0))
}

/// See [`super::base_size::determine_flex_base_size`].
fn determine_flex_base_size<D: FlexDir, T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    c: &AlgoConstants<D>,
    available_space: Size<AvailableSpace>,
    items: &mut [Item],
) {
    let dir = c.dir;
    let read_main = |out: &LayoutOutput| out.size.get(abs_main(dir));
    let mut questions: Questions<f32> = Questions::new(tree, items.len() * 2);
    let mut observes_cross = false;
    let cross_axis_parent_size = c.node_inner_size.cross(dir);
    let child_parent_size = from_cross(dir, cross_axis_parent_size);
    let cross_axis_margin_sum = c.margin.cross_axis_sum(dir);
    let available_cross = available_space.cross(dir);
    let main_available =
        if available_space.main(dir) == AvailableSpace::MinContent { AvailableSpace::MinContent } else { AvailableSpace::MaxContent };
    for child in items.iter_mut() {
        if child.flex_basis_is_definite && !child.measures_min {
            continue;
        }
        let child_min_cross = child.min_size.cross(dir).maybe_add(cross_axis_margin_sum);
        let child_max_cross = child.max_size.cross(dir).maybe_add(cross_axis_margin_sum);
        let clamp_tells_apart = (child_min_cross.is_some() || child_max_cross.is_some()) && !available_cross.is_definite();
        let cross_axis_available_space = match available_cross {
            AvailableSpace::Definite(val) => AvailableSpace::Definite(
                c.divided_cross_space(cross_axis_parent_size.unwrap_or(val)).maybe_clamp(child_min_cross, child_max_cross),
            ),
            AvailableSpace::MinContent => child_min_cross.map_or(AvailableSpace::MinContent, AvailableSpace::Definite),
            AvailableSpace::MaxContent => child_max_cross.map_or(AvailableSpace::MaxContent, AvailableSpace::Definite),
        };
        let mut stretched_from_space = false;
        let mut child_known_dimensions = child.size.with_main(dir, None);
        child_known_dimensions.set_cross(
            dir,
            child_known_dimensions.cross(dir).maybe_clamp(child.min_size.cross(dir), child.max_size.cross(dir)),
        );
        if child.align_self == AlignSelf::STRETCH && child_known_dimensions.cross(dir).is_none() {
            let stretched = match c.indefinite_stretch_cross {
                Some(cross) => Some(
                    (cross - child.margin.cross_axis_sum(dir)).maybe_clamp(child.min_size.cross(dir), child.max_size.cross(dir)),
                ),
                None => {
                    stretched_from_space = true;
                    cross_axis_available_space.into_option().maybe_sub(child.margin.cross_axis_sum(dir))
                }
            };
            child_known_dimensions.set_cross(dir, stretched.maybe_max(0.0));
        }
        observes_cross |= clamp_tells_apart && (stretched_from_space || child_known_dimensions.cross(dir).is_none());

        if !child.flex_basis_is_definite {
            let child_available_space = Size::MAX_CONTENT.with_main(dir, main_available).with_cross(dir, cross_axis_available_space);
            let request = size_request(
                child.node,
                child_known_dimensions,
                child_parent_size,
                child_available_space,
                SizingMode::ContentSize,
                abs_main(dir).into(),
            );
            child.question[0] = ask(&mut questions, tree, child.cheap_leaf, request, read_main) as u32;
        }
        if child.measures_min {
            let child_available_space = Size::MIN_CONTENT.with_cross(dir, cross_axis_available_space);
            let request = size_request(
                child.node,
                child_known_dimensions,
                child_parent_size,
                child_available_space,
                SizingMode::ContentSize,
                abs_main(dir).into(),
            );
            child.question[1] = ask(&mut questions, tree, child.cheap_leaf, request, read_main) as u32;
        }
    }

    if observes_cross {
        tree.note_observed(crate::compute::observed_bit(dir.cross_axis()));
    }
    questions.finish(tree, read_main);

    for child in items.iter_mut() {
        if child.question[0] != NONE {
            child.flex_basis = questions.get(child.question[0] as usize);
        }
        let padding_border_sum = child.padding_border_main;
        child.flex_basis = child.flex_basis.max(padding_border_sum);
        child.inner_flex_basis = child.flex_basis - padding_border_sum;

        let padding_border_main = Some(child.padding_border.main(dir));
        child.resolved_minimum_main_size = match child.automatic_min_main(dir) {
            Some(v) => v,
            None => {
                let min_content_main_size = questions.get(child.question[1] as usize);
                let clamped = min_content_main_size.maybe_min(child.size.main(dir)).maybe_min(child.max_size.main(dir));
                clamped.maybe_max(padding_border_main)
            }
        };
        let hypothetical_inner_min_main =
            child.resolved_minimum_main_size.maybe_max(child.min_size.main(dir)).maybe_max(padding_border_main);
        let hypothetical_inner_size = child.flex_basis.maybe_clamp(Some(hypothetical_inner_min_main), child.max_size.main(dir));
        child.hypothetical_inner_size.set_main(dir, hypothetical_inner_size);
        child.hypothetical_outer_main = hypothetical_inner_size + child.margin.main_axis_sum(dir);
        child.question = [NONE; 2];
    }
}

/// See [`super::main_size::determine_container_main_size`]; one line.
fn determine_container_main_size<D: FlexDir, T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    available_space: Size<AvailableSpace>,
    items: &mut [Item],
    c: &mut AlgoConstants<D>,
) {
    let dir = c.dir;
    let main_content_box_inset = c.content_box_inset.main_axis_sum(dir);
    let outer_main_size: f32 = match c.node_outer_size.main(dir) {
        Some(v) => v,
        None => match available_space.main(dir) {
            AvailableSpace::Definite(_) => {
                let gap = c.gap.main(dir);
                let line_length = items
                    .iter()
                    .map(|ch| {
                        (ch.flex_basis.maybe_max(ch.min_size.main(dir)) + ch.margin.main_axis_sum(dir)).max(ch.padding_border.main(dir))
                    })
                    .sum::<f32>()
                    + sum_axis_gaps(gap, items.len());
                let longest_line_length = f32::max(f32::NEG_INFINITY, line_length).max(0.0);
                longest_line_length + main_content_box_inset
            }
            AvailableSpace::MinContent | AvailableSpace::MaxContent => {
                let mut main_size = 0.0f32;
                // Summed as `sum` would: from -0.0, in item order.
                let mut item_main_size_sum = -0.0f32;
                for item in items.iter() {
                    let style_min = item.min_size.main(dir);
                    let style_preferred = item.size.main(dir);
                    let style_max = item.max_size.main(dir);
                    let clamping_basis = Some(item.flex_basis).maybe_max(style_preferred);
                    let flex_basis_min = clamping_basis.filter(|_| item.flex_shrink == 0.0);
                    let flex_basis_max = clamping_basis.filter(|_| item.flex_grow == 0.0);
                    let min_main_size = style_min
                        .maybe_max(flex_basis_min)
                        .or(flex_basis_min)
                        .unwrap_or(item.resolved_minimum_main_size)
                        .max(item.resolved_minimum_main_size);
                    let max_main_size = style_max.maybe_min(flex_basis_max).or(flex_basis_max).unwrap_or(f32::INFINITY);
                    let content_contribution = match (min_main_size, style_preferred, max_main_size) {
                        (min, Some(pref), max) if max <= min || max <= pref => {
                            pref.min(max).max(min) + item.margin.main_axis_sum(dir)
                        }
                        (min, _, max) if max <= min => min + item.margin.main_axis_sum(dir),
                        _ if item.overflow.x.is_scroll_container() || item.overflow.y.is_scroll_container() => {
                            item.flex_basis + item.margin.main_axis_sum(dir)
                        }
                        (_, Some(pref), _) => {
                            let inner_main_size = pref.max(item.padding_border_main);
                            if c.is_row {
                                (inner_main_size + item.margin.main_axis_sum(dir)).maybe_clamp(style_min, style_max)
                            } else {
                                (inner_main_size.max(item.flex_basis) + item.margin.main_axis_sum(dir)).maybe_clamp(style_min, style_max)
                            }
                        }
                        _ => {
                            let cross_axis_parent_size = c.node_inner_size.cross(dir);
                            let cross_axis_margin_sum = c.margin.cross_axis_sum(dir);
                            let child_min_cross = item.min_size.cross(dir).maybe_add(cross_axis_margin_sum);
                            let child_max_cross = item.max_size.cross(dir).maybe_add(cross_axis_margin_sum);
                            let cross_axis_available_space = available_space
                                .cross(dir)
                                .map_definite(|val| c.divided_cross_space(cross_axis_parent_size.unwrap_or(val)))
                                .maybe_clamp(child_min_cross, child_max_cross);
                            let mut child_known_dimensions = item.size.with_main(dir, None);
                            child_known_dimensions.set_cross(
                                dir,
                                child_known_dimensions.cross(dir).maybe_clamp(item.min_size.cross(dir), item.max_size.cross(dir)),
                            );
                            if item.align_self == AlignSelf::STRETCH && child_known_dimensions.cross(dir).is_none() {
                                let stretched = c.indefinite_stretch_cross.or(cross_axis_available_space.into_option());
                                child_known_dimensions
                                    .set_cross(dir, stretched.maybe_sub(item.margin.cross_axis_sum(dir)).maybe_max(0.0));
                            }
                            let child_available_space = available_space.with_cross(dir, cross_axis_available_space);
                            let measured_main_size = tree.measure_child_size(
                                item.node,
                                child_known_dimensions,
                                c.node_inner_size,
                                child_available_space,
                                SizingMode::ContentSize,
                                abs_main(dir),
                                Line::FALSE,
                            );
                            let inner_main_size = measured_main_size.maybe_max(None);
                            if c.is_row {
                                (inner_main_size + item.margin.main_axis_sum(dir)).maybe_clamp(style_min, style_max)
                            } else {
                                (inner_main_size.max(item.flex_basis) + item.margin.main_axis_sum(dir))
                                    .maybe_clamp(style_min, style_max)
                            }
                        }
                    };
                    let flex_fraction = {
                        let diff = content_contribution - item.flex_basis;
                        if diff > 0.0 {
                            diff / item.flex_grow.max(1.0)
                        } else if diff < 0.0 {
                            let scaled_shrink_factor = item.flex_shrink.max(1.0) * item.inner_flex_basis;
                            diff / scaled_shrink_factor
                        } else {
                            0.0
                        }
                    };
                    let flex_contribution = if flex_fraction > 0.0 {
                        item.flex_grow.max(1.0) * flex_fraction
                    } else if flex_fraction < 0.0 {
                        let scaled_shrink_factor = item.flex_shrink.max(1.0) * item.inner_flex_basis;
                        if scaled_shrink_factor == 0.0 { 0.0 } else { scaled_shrink_factor * flex_fraction }
                    } else {
                        0.0
                    };
                    // The general algorithm also stores this as the target size, which resolving
                    // the flexible lengths overwrites unread.
                    item_main_size_sum += item.flex_basis + flex_contribution;
                }
                let gap_sum = sum_axis_gaps(c.gap.main(dir), items.len());
                main_size = main_size.max(item_main_size_sum + gap_sum);
                main_size + main_content_box_inset
            }
        },
    };
    let outer_main_size = outer_main_size
        .maybe_clamp(c.min_size.main(dir), c.max_size.main(dir))
        .max(main_content_box_inset - c.scrollbar_gutter.main(dir));
    let inner_main_size = (outer_main_size - main_content_box_inset).max(0.0);
    c.container_size.set_main(dir, outer_main_size);
    c.inner_container_size.set_main(dir, inner_main_size);
    c.node_inner_size.set_main(dir, Some(inner_main_size));
}

/// See [`super::cross::determine_hypothetical_cross_size`].
fn determine_hypothetical_cross_size<D: FlexDir, T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    items: &mut [Item],
    c: &AlgoConstants<D>,
    available_space: Size<AvailableSpace>,
    fixed_line_cross: Option<f32>,
) {
    let dir = c.dir;
    let read_cross = |out: &LayoutOutput| out.size.get(dir.cross_axis());
    let mut questions: Questions<f32> = Questions::new(tree, items.len());
    let mut asked = false;
    for child in items.iter_mut() {
        let padding_border_sum = child.padding_border.cross(dir);
        let min = child.min_size.cross(dir);
        let max = child.max_size.cross(dir);
        let child_cross = child.size.cross(dir).maybe_clamp(min, max).maybe_max(padding_border_sum);
        let known = child_cross.or_else(|| {
            let stretched_cross = c.indefinite_stretch_cross.filter(|_| child.stretched).map(|cross| {
                (cross - child.margin.cross_axis_sum(dir)).maybe_clamp(min, max).max(padding_border_sum)
            });
            let unread = fixed_line_cross.is_some() && child.stretched;
            stretched_cross.or(unread.then_some(f32::NAN))
        });
        if let Some(child_inner_cross) = known {
            child.hypothetical_inner_size.set_cross(dir, child_inner_cross);
            continue;
        }
        let child_known_main = AvailableSpace::Definite(c.container_size.main(dir));
        let child_available_cross = available_space
            .cross(dir)
            .map_definite(|val| c.divided_cross_space(val))
            .maybe_clamp(min, max)
            .maybe_max(padding_border_sum);
        let request = ChildRequest {
            node: child.node,
            input: LayoutInput {
                run_mode: RunMode::ComputeSize,
                sizing_mode: SizingMode::ContentSize,
                axis: dir.cross_axis().into(),
                known_dimensions: Size {
                    width: if c.is_row { Some(child.target_size.width) } else { None },
                    height: if c.is_row { None } else { Some(child.target_size.height) },
                },
                known_dimensions_are_definite: child.known_dimension_definiteness(c),
                parent_size: c.node_inner_size,
                available_space: Size {
                    width: if c.is_row { child_known_main } else { child_available_cross },
                    height: if c.is_row { child_available_cross } else { child_known_main },
                },
                vertical_margins_are_collapsible: Line::FALSE,
                context_key: 0,
            },
        };
        child.question[0] = ask(&mut questions, tree, child.cheap_leaf, request, read_cross) as u32;
        asked = true;
    }
    if !asked {
        return;
    }
    questions.finish(tree, read_cross);
    for child in items.iter_mut().filter(|child| child.question[0] != NONE) {
        let child_inner_cross = questions
            .get(child.question[0] as usize)
            .maybe_clamp(child.min_size.cross(dir), child.max_size.cross(dir))
            .max(child.padding_border.cross(dir));
        child.hypothetical_inner_size.set_cross(dir, child_inner_cross);
        child.question[0] = NONE;
    }
}

/// See [`super::cross::calculate_cross_size`]; one line, no baseline alignment.
fn calculate_cross_size<D: FlexDir>(items: &[Item], node_size: Size<Option<f32>>, c: &AlgoConstants<D>) -> f32 {
    let dir = c.dir;
    let cross_axis_padding_border = c.content_box_inset.cross_axis_sum(dir);
    let cross_min_size = c.min_size.cross(dir);
    let cross_max_size = c.max_size.cross(dir);
    if node_size.cross(dir).is_some() {
        node_size
            .cross(dir)
            .maybe_clamp(cross_min_size, cross_max_size)
            .maybe_sub(cross_axis_padding_border)
            .maybe_max(0.0)
            .unwrap_or(0.0)
    } else {
        items
            .iter()
            .map(|ch| ch.hypothetical_inner_size.cross(dir) + ch.margin.cross_axis_sum(dir))
            .fold(0.0f32, f32::max)
            .maybe_clamp(cross_min_size.maybe_sub(cross_axis_padding_border), cross_max_size.maybe_sub(cross_axis_padding_border))
    }
}

/// See [`super::align::distribute_remaining_free_space`]; no auto margins.
fn distribute_remaining_free_space<D: FlexDir>(items: &mut [Item], c: &AlgoConstants<D>) {
    let dir = c.dir;
    let total_main_axis_gap = sum_axis_gaps(c.gap.main(dir), items.len());
    let used_space: f32 = total_main_axis_gap + items.iter().map(|ch| ch.outer_target_main).sum::<f32>();
    let free_space = c.inner_container_size.main(dir) - used_space;
    let num_items = items.len();
    let layout_reverse = dir.is_reverse();
    let gap = c.gap.main(dir);
    let raw = c.justify_content.unwrap_or(JustifyContent::FLEX_START);
    let keyword = apply_alignment_fallback(free_space, num_items, raw);
    let justify = |(i, ch): (usize, &mut Item)| {
        ch.offset_main = compute_alignment_offset(free_space, num_items, gap, keyword, i == 0, layout_reverse);
    };
    if layout_reverse {
        items.iter_mut().rev().enumerate().for_each(justify);
    } else {
        items.iter_mut().enumerate().for_each(justify);
    }
}

/// The final-layout question asked of an item; see [`super::final_pass`].
#[inline]
fn item_request<D: FlexDir>(item: &Item, c: &AlgoConstants<D>) -> ChildRequest {
    ChildRequest {
        node: item.node,
        input: LayoutInput {
            run_mode: RunMode::PerformLayout,
            sizing_mode: SizingMode::ContentSize,
            axis: RequestedAxis::Both,
            known_dimensions: item.target_size.map(Some),
            known_dimensions_are_definite: item.known_dimension_definiteness(c),
            parent_size: c.node_inner_size,
            available_space: c.container_size.map(AvailableSpace::Definite),
            vertical_margins_are_collapsible: Line::FALSE,
            context_key: 0,
        },
    }
}

/// See [`super::final_pass::final_layout_pass`]; returns the in-flow overflow and the container's
/// baselines.
fn final_layout_pass<D: FlexDir, T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    line: &FlexLine,
    items: &[Item],
    c: &AlgoConstants<D>,
) -> (Rect<f32>, Baselines) {
    let dir = c.dir;
    let mut total_offset_cross = if c.is_column && c.layout_direction == Direction::Rtl {
        c.container_size.width - c.content_box_inset.cross_end(dir)
    } else {
        c.content_box_inset.cross_start(dir)
    };
    let mut overflow_rect = Rect::ZERO;
    let mut questions: Questions<LayoutOutput> = Questions::new(tree, items.len());
    for item in items.iter() {
        ask(&mut questions, tree, item.cheap_leaf, item_request(item, c), |out| *out);
    }
    questions.finish(tree, |out| *out);
    let outputs = questions.answers();

    let padding_border = c.content_box_inset;
    let mut total_offset_main = if c.layout_direction == Direction::Rtl && dir.is_row() {
        c.container_size.width - padding_border.main_end(dir)
    } else {
        padding_border.main_start(dir)
    };
    let line_offset_cross = line.offset_cross;
    let is_rtl_column = c.layout_direction == Direction::Rtl && dir.is_column();
    if is_rtl_column {
        total_offset_cross -= line_offset_cross + line.cross_size;
    }
    let is_rtl_row = dir.is_row() && c.layout_direction == Direction::Rtl;
    let container_size = c.container_size;
    let border = c.border;
    // Only the first item's first baseline and the endmost item's last baseline are read.
    let first_index = if c.is_column && dir.is_reverse() { items.len() - 1 } else { 0 };
    let last_index = if c.is_column && dir.is_reverse() { 0 } else { items.len() - 1 };
    let mut first_baseline = 0.0;
    let mut last_baseline = 0.0;
    let mut place = |index: usize, item: &Item, out: &LayoutOutput| {
        let size = out.size;
        let plan = tree.style_plan(item.node).expect("plain items have a plan");
        let inset = if plan.offsets_in_flow { plan.inset } else { Rect::NONE };
        let (padding, item_border) = (plan.padding, plan.border);
        let main_relative_inset = if is_rtl_row {
            inset.main_end(dir).or(inset.main_start(dir).map(|p| -p)).unwrap_or(0.0)
        } else {
            inset.main_start(dir).or(inset.main_end(dir).map(|p| -p)).unwrap_or(0.0)
        };
        let cross_relative_inset = if is_rtl_column {
            inset.cross_end(dir).map(|p| -p).or(inset.cross_start(dir)).unwrap_or(0.0)
        } else {
            inset.cross_start(dir).or(inset.cross_end(dir).map(|p| -p)).unwrap_or(0.0)
        };
        let effective_line_offset_cross = if is_rtl_column { 0.0 } else { line_offset_cross };
        let offset_main = if is_rtl_row {
            total_offset_main - item.offset_main - item.margin.main_end(dir) - main_relative_inset - size.width
        } else {
            total_offset_main + item.offset_main + item.margin.main_start(dir) + main_relative_inset
        };
        let offset_cross =
            total_offset_cross + item.offset_cross + effective_line_offset_cross + item.margin.cross_start(dir) + cross_relative_inset;

        if index == first_index || index == last_index {
            let (baseline, item_last_baseline) = if dir.is_row() {
                let baseline_offset_cross =
                    total_offset_cross + item.offset_cross + effective_line_offset_cross + item.margin.cross_start(dir);
                let inner_baseline = |baseline: Option<f32>| {
                    let baseline = baseline.unwrap_or(size.height);
                    if item.overflow.y.is_scroll_container() { baseline.min(size.height).max(0.0) } else { baseline }
                };
                (
                    baseline_offset_cross + inner_baseline(out.baselines.first),
                    baseline_offset_cross + inner_baseline(out.baselines.last),
                )
            } else {
                let baseline_offset_main = total_offset_main + item.offset_main + item.margin.main_start(dir);
                (
                    baseline_offset_main + out.baselines.first.unwrap_or(size.height),
                    baseline_offset_main + out.baselines.last.unwrap_or(size.height),
                )
            };
            if index == first_index {
                first_baseline = baseline;
            }
            if index == last_index {
                last_baseline = item_last_baseline;
            }
        }

        let location =
            if dir.is_row() { Point { x: offset_main, y: offset_cross } } else { Point { x: offset_cross, y: offset_main } };
        let scrollbar_size = Size {
            width: if item.overflow.y == Overflow::Scroll { item.scrollbar_width } else { 0.0 },
            height: if item.overflow.x == Overflow::Scroll { item.scrollbar_width } else { 0.0 },
        };
        let layout = Layout {
            order: item.order,
            size,
            scrollable_overflow_rect: out.scrollable_overflow_rect,
            scrollbar_size,
            location,
            padding,
            border: item_border,
            margin: item.margin,
        };
        tree.set_unrounded_layout(item.node, &layout);

        if is_rtl_row {
            total_offset_main -= item.offset_main + item.margin.main_axis_sum(dir) + size.main(dir);
        } else {
            total_offset_main += item.offset_main + item.margin.main_axis_sum(dir) + size.main(dir);
        }

        let contribution_location = if c.layout_direction == Direction::Rtl {
            Point { x: container_size.width - (location.x + size.width) - border.right, y: location.y - border.top }
        } else {
            Point { x: location.x - border.left, y: location.y - border.top }
        };
        overflow_rect = union(
            overflow_rect,
            scrollable_overflow_contribution(
                contribution_location,
                size,
                out.scrollable_overflow_rect,
                item.overflow,
                item.contain,
                c.is_scroll_container,
            ),
        );
    };
    if dir.is_reverse() {
        for (index, (item, out)) in items.iter().zip(outputs).enumerate().rev() {
            place(index, item, out);
        }
    } else {
        for (index, (item, out)) in items.iter().zip(outputs).enumerate() {
            place(index, item, out);
        }
    }

    if c.is_scroll_container {
        overflow_rect.right += if c.layout_direction == Direction::Rtl {
            c.content_box_inset.left - c.border.left - c.scrollbar_gutter.x
        } else {
            c.content_box_inset.right - c.border.right - c.scrollbar_gutter.x
        };
        overflow_rect.bottom += c.content_box_inset.bottom - c.border.bottom - c.scrollbar_gutter.y;
    }
    (overflow_rect, Baselines { first: Some(first_baseline), last: Some(last_baseline) })
}

crate::compute::scratch::pooled!(Item);
