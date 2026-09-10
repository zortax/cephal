//! §9.4 cross sizes and baselines.

use super::axis::{FlexAxisRect, FlexAxisSize, FlexAxisSum, sum_axis_gaps};
use super::{AlgoConstants, FlexItem, FlexLine, item_known_dimension_definiteness};
use crate::compute::common::sizing_keyword::{SizingKeywordResolution, resolve_sizing_keyword};
use crate::compute::scratch::Scratch;
use crate::geometry::{AvailableSpace, Line, MaybeMath, Size};
use crate::style::{AlignContent, AlignSelf, BoxSizing};
use crate::tree::{CacheAccess, ChildRequest, LayoutInput, LayoutOutput, LayoutTree, RequestedAxis, RunMode, SizingMode};

/// Per-item state carried across the batch of cross-size questions.
struct CrossPending {
    min: Option<f32>,
    max: Option<f32>,
    padding_border_sum: f32,
    /// The cross size when the style fixes it, else the batch index of its measurement.
    known: Result<f32, usize>,
}

pub(super) fn determine_hypothetical_cross_size<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    items: &mut [FlexItem],
    c: &AlgoConstants,
    available_space: Size<AvailableSpace>,
) {
    let dir = c.dir;
    let mut requests: Scratch<ChildRequest> = Scratch::with_capacity(items.len());
    let mut pending: Scratch<CrossPending> = Scratch::with_capacity(items.len());
    for child in items.iter() {
        let padding_border_sum = (child.padding + child.border).cross_axis_sum(dir);
        let child_known_main = AvailableSpace::Definite(c.container_size.main(dir));
        let transferred_min_cross = child.min_size.maybe_apply_aspect_ratio(child.aspect_ratio).cross(dir);
        let transferred_max_cross = child.max_size.maybe_apply_aspect_ratio(child.aspect_ratio).cross(dir);
        let child_cross =
            child.size.cross(dir).maybe_clamp(transferred_min_cross, transferred_max_cross).maybe_max(padding_border_sum);
        let child_available_cross = available_space
            .cross(dir)
            .map_definite(|val| c.divided_cross_space(val))
            .maybe_clamp(transferred_min_cross, transferred_max_cross)
            .maybe_max(padding_border_sum);
        let cross_stretch_size =
            c.node_inner_size.cross(dir).map(|v| c.divided_cross_space(v)).maybe_sub(child.margin.cross_axis_sum(dir)).maybe_max(0.0);
        let child_available_cross =
            match resolve_sizing_keyword(child.size_style.cross(dir).raw(), cross_stretch_size, c.node_inner_size.cross(dir)) {
                Some(SizingKeywordResolution::Measure(a)) => a,
                _ => child_available_cross,
            };
        let known = match child_cross {
            Some(v) => Ok(v),
            None => {
                requests.push(ChildRequest {
                    node: child.node,
                    input: LayoutInput {
                        run_mode: RunMode::ComputeSize,
                        sizing_mode: SizingMode::ContentSize,
                        axis: dir.cross_axis().into(),
                        known_dimensions: Size {
                            width: if c.is_row { Some(child.target_size.width) } else { child_cross },
                            height: if c.is_row { child_cross } else { Some(child.target_size.height) },
                        },
                        known_dimensions_are_definite: item_known_dimension_definiteness(c, child),
                        parent_size: c.node_inner_size,
                        available_space: Size {
                            width: if c.is_row { child_known_main } else { child_available_cross },
                            height: if c.is_row { child_available_cross } else { child_known_main },
                        },
                        vertical_margins_are_collapsible: Line::FALSE,
                        context_key: 0,
                    },
                });
                Err(requests.len() - 1)
            }
        };
        pending.push(CrossPending { min: transferred_min_cross, max: transferred_max_cross, padding_border_sum, known });
    }
    // Each item's cross size depends only on its own target main size.
    let mut outputs: Scratch<LayoutOutput> = Scratch::with_capacity(requests.len());
    tree.compute_child_layouts(&requests, &mut outputs);
    for (child, p) in items.iter_mut().zip(pending.iter()) {
        let child_inner_cross = match p.known {
            Ok(v) => v,
            Err(i) => outputs[i].size.get(dir.cross_axis()).maybe_clamp(p.min, p.max).max(p.padding_border_sum),
        };
        child.hypothetical_inner_size.set_cross(dir, child_inner_cross);
        child.hypothetical_outer_size.set_cross(dir, child_inner_cross + child.margin.cross_axis_sum(dir));
    }
}

pub(super) fn calculate_children_base_lines<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    run_mode: RunMode,
    node_size: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    lines: &[FlexLine],
    items: &mut [FlexItem],
    c: &AlgoConstants,
) {
    if !c.is_row || !lines.iter().any(|l| l.items(items).iter().filter(|ch| ch.participates_in_baseline_alignment(c.dir)).count() > 1) {
        return;
    }
    if run_mode == RunMode::ComputeSize {
        crate::compute::speculate(tree, |tree| baselines_inner(tree, node_size, available_space, lines, items, c));
    } else {
        baselines_inner(tree, node_size, available_space, lines, items, c);
    }
}

fn baselines_inner<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node_size: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    lines: &[FlexLine],
    items: &mut [FlexItem],
    c: &AlgoConstants,
) {
    let dir = c.dir;
    let mut requests: Scratch<ChildRequest> = Scratch::with_capacity(items.len());
    let mut indexes: Scratch<usize> = Scratch::with_capacity(items.len());
    for line in lines {
        let line_items = line.items(items);
        if line_items.iter().filter(|ch| ch.participates_in_baseline_alignment(dir)).count() <= 1 {
            continue;
        }
        for (offset, child) in line_items.iter().enumerate() {
            if !child.participates_in_baseline_alignment(dir) {
                continue;
            }
            indexes.push(line.start + offset);
            requests.push(ChildRequest {
                node: child.node,
                input: LayoutInput {
                    run_mode: RunMode::PerformLayout,
                    sizing_mode: SizingMode::ContentSize,
                    axis: RequestedAxis::Both,
                    known_dimensions: Size {
                        width: Some(if c.is_row { child.target_size.width } else { child.hypothetical_inner_size.width }),
                        height: Some(if c.is_row { child.hypothetical_inner_size.height } else { child.target_size.height }),
                    },
                    known_dimensions_are_definite: item_known_dimension_definiteness(c, child),
                    parent_size: c.node_inner_size,
                    available_space: Size {
                        width: if c.is_row {
                            AvailableSpace::Definite(c.container_size.width)
                        } else {
                            available_space.width.maybe_set(node_size.width)
                        },
                        height: if c.is_row {
                            available_space.height.maybe_set(node_size.height)
                        } else {
                            AvailableSpace::Definite(c.container_size.height)
                        },
                    },
                    vertical_margins_are_collapsible: Line::FALSE,
                    context_key: 0,
                },
            });
        }
    }
    let mut outputs: Scratch<LayoutOutput> = Scratch::with_capacity(requests.len());
    tree.compute_child_layouts(&requests, &mut outputs);
    for (&i, out) in indexes.iter().zip(outputs.iter()) {
        let child = &mut items[i];
        let height = out.size.height;
        let baseline = if child.overflow.y.is_scroll_container() {
            out.baselines.first.unwrap_or(height).min(height).max(0.0)
        } else {
            out.baselines.first.unwrap_or(height)
        };
        child.baseline = baseline + child.margin.top;
    }
}

trait AvailMaybeSet {
    fn maybe_set(self, v: Option<f32>) -> Self;
}
impl AvailMaybeSet for AvailableSpace {
    #[inline]
    fn maybe_set(self, v: Option<f32>) -> Self {
        v.map_or(self, AvailableSpace::Definite)
    }
}

pub(super) fn calculate_cross_size(lines: &mut [FlexLine], items: &[FlexItem], node_size: Size<Option<f32>>, c: &AlgoConstants) {
    let dir = c.dir;
    let cross_axis_padding_border = c.content_box_inset.cross_axis_sum(dir);
    let cross_min_size = c.min_size.cross(dir);
    let cross_max_size = c.max_size.cross(dir);
    if !c.is_wrap && node_size.cross(dir).is_some() {
        lines[0].cross_size = node_size
            .cross(dir)
            .maybe_clamp(cross_min_size, cross_max_size)
            .maybe_sub(cross_axis_padding_border)
            .maybe_max(0.0)
            .unwrap_or(0.0);
    } else {
        for line in lines.iter_mut() {
            let line_items = line.items(items);
            let max_baseline = line_items.iter().map(|ch| ch.baseline).fold(0.0f32, f32::max);
            line.cross_size = line_items
                .iter()
                .map(|ch| {
                    if ch.participates_in_baseline_alignment(dir) {
                        max_baseline - ch.baseline + ch.hypothetical_outer_size.cross(dir)
                    } else {
                        ch.hypothetical_outer_size.cross(dir)
                    }
                })
                .fold(0.0f32, f32::max);
        }
        if !c.is_wrap {
            lines[0].cross_size = lines[0].cross_size.maybe_clamp(
                cross_min_size.maybe_sub(cross_axis_padding_border),
                cross_max_size.maybe_sub(cross_axis_padding_border),
            );
        }
    }
}

pub(super) fn handle_align_content_stretch(lines: &mut [FlexLine], node_size: Size<Option<f32>>, c: &AlgoConstants) {
    if c.align_content != AlignContent::STRETCH {
        return;
    }
    let dir = c.dir;
    let cross_axis_padding_border = c.content_box_inset.cross_axis_sum(dir);
    let cross_min_size = c.min_size.cross(dir);
    let cross_max_size = c.max_size.cross(dir);
    let container_min_inner_cross = node_size
        .cross(dir)
        .or(cross_min_size)
        .maybe_clamp(cross_min_size, cross_max_size)
        .maybe_sub(cross_axis_padding_border)
        .maybe_max(0.0)
        .unwrap_or(0.0);
    let total_cross_axis_gap = sum_axis_gaps(c.gap.cross(dir), lines.len());
    let lines_total_cross: f32 = lines.iter().map(|l| l.cross_size).sum::<f32>() + total_cross_axis_gap;
    if lines_total_cross < container_min_inner_cross {
        let addition = (container_min_inner_cross - lines_total_cross) / lines.len() as f32;
        for line in lines.iter_mut() {
            line.cross_size += addition;
        }
    }
}

pub(super) fn determine_used_cross_size<T: LayoutTree + ?Sized>(
    tree: &T,
    lines: &[FlexLine],
    items: &mut [FlexItem],
    c: &AlgoConstants,
) {
    let dir = c.dir;
    let calc = |id, basis| tree.resolve_calc(id, basis);
    for line in lines {
        let line_cross_size = line.cross_size;
        for child in line.items_mut(items) {
            let style = tree.style(child.node);
            let cross_is_stretch = child.size_style.cross(dir).is_stretch();
            let stretched = !child.margin_is_auto.cross_start(dir)
                && !child.margin_is_auto.cross_end(dir)
                && (cross_is_stretch || (child.align_self == AlignSelf::STRETCH && style.size.cross(dir).is_auto()));
            let target_cross = if stretched {
                let padding = style.padding.map(|p| p.resolve_or_zero(c.node_inner_size.width, &calc));
                let border = style.border.map(|b| b.resolve_or_zero(c.node_inner_size.width, &calc));
                let pb_sum = (padding + border).sum_axes();
                let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { pb_sum } else { Size::ZERO };
                let max_size_ignoring_aspect_ratio =
                    crate::compute::block::resolve_size(style.max_size.map(|v| v.raw()), c.node_inner_size, &calc)
                        .maybe_add(box_sizing_adjustment);
                (line_cross_size - child.margin.cross_axis_sum(dir))
                    .max(0.0)
                    .maybe_clamp(child.min_size.cross(dir), max_size_ignoring_aspect_ratio.cross(dir))
            } else {
                child.hypothetical_inner_size.cross(dir)
            };
            child.target_size.set_cross(dir, target_cross);
            child.outer_target_size.set_cross(dir, target_cross + child.margin.cross_axis_sum(dir));
        }
    }
}

/// Returns the total cross size of all lines.
pub(super) fn determine_container_cross_size(lines: &[FlexLine], node_size: Size<Option<f32>>, c: &mut AlgoConstants) -> f32 {
    let dir = c.dir;
    let total_cross_axis_gap = sum_axis_gaps(c.gap.cross(dir), lines.len());
    let total_line_cross_size: f32 = lines.iter().map(|l| l.cross_size).sum();
    let padding_border_sum = c.content_box_inset.cross_axis_sum(dir);
    let cross_scrollbar_gutter = c.scrollbar_gutter.cross(dir);
    let outer = node_size
        .cross(dir)
        .unwrap_or(total_line_cross_size + total_cross_axis_gap + padding_border_sum)
        .maybe_clamp(c.min_size.cross(dir), c.max_size.cross(dir))
        .max(padding_border_sum - cross_scrollbar_gutter);
    let inner = (outer - padding_border_sum).max(0.0);
    c.container_size.set_cross(dir, outer);
    c.inner_container_size.set_cross(dir, inner);
    total_line_cross_size
}

crate::compute::scratch::pooled!(CrossPending, usize);
