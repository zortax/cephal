//! Placing in-flow children: margin collapsing, floats and clearance.

use super::BlockContext;
use super::floats::FIT_TOLERANCE;
use super::items::BlockItem;
use crate::compute::LayoutTreeExt;
use crate::compute::common::alignment::resolve_self_alignment_safety;
use crate::compute::common::overflow::{scrollable_overflow_contribution, union};
use crate::compute::common::sizing_keyword::{SizingKeywordResolution, resolve_sizing_keyword};
use crate::geometry::{AbsoluteAxis, AvailableSpace, Line, MaybeMath, Point, Rect, Size};
use crate::style::{AlignItems, AlignItemsKeyword, Dimension, Direction, Length, Overflow, TextAlign};
use crate::tree::{CacheAccess, CollapsibleMarginSet, Layout, LayoutInput, LayoutTree, RequestedAxis, RunMode, SizingMode};

pub(super) struct FlowParams {
    pub container_outer_width: f32,
    pub container_percentage_resolution_height: Option<f32>,
    pub content_box_inset: Rect<f32>,
    pub resolved_content_box_inset: Rect<f32>,
    pub resolved_border: Rect<f32>,
    pub text_align: TextAlign,
    pub direction: Direction,
    pub own_margins_collapse_with_children: Line<bool>,
    pub children_margins_adjoin_own_edges: Line<bool>,
    pub is_scroll_container: bool,
}

pub(super) struct FlowResult {
    pub overflow_rect: Rect<f32>,
    pub content_height: f32,
    pub first_child_top_margin_set: CollapsibleMarginSet,
    pub last_child_bottom_margin_set: CollapsibleMarginSet,
    pub first_baseline: Option<f32>,
    pub last_baseline: Option<f32>,
    /// Some child is out of flow, or passes out-of-flow boxes through.
    pub has_out_of_flow: bool,
}

/// `stretch` is the only block-axis keyword that needs resolving; intrinsic keywords equal `auto`.
///
/// A margin that could collapse through the container's edge on its side counts as zero
/// (csswg-drafts#11044, #13260).
#[inline]
fn resolve_stretch_height(height: Dimension, container_inner_height: Option<f32>, margin: Rect<f32>, margins_treated_as_zero: Line<bool>) -> Option<f32> {
    if !height.raw().is_stretch() {
        return None;
    }
    let top = if margins_treated_as_zero.start { 0.0 } else { margin.top };
    let bottom = if margins_treated_as_zero.end { 0.0 } else { margin.bottom };
    container_inner_height.map(|h| h - top - bottom)
}

/// The offset of a block-level box within its alignment container's free space.
#[inline(never)]
fn justify_offset(justify_self: AlignItems, free: f32, direction: Direction) -> f32 {
    let end = match resolve_self_alignment_safety(justify_self, free < 0.0) {
        AlignItemsKeyword::Center => return free / 2.0,
        AlignItemsKeyword::End | AlignItemsKeyword::FlexEnd | AlignItemsKeyword::SelfEnd => true,
        _ => false,
    };
    if end != (direction == Direction::Rtl) { free } else { 0.0 }
}

#[inline]
fn contribution_location(p: &FlowParams, location: Point<f32>, size: Size<f32>) -> Point<f32> {
    if p.direction == Direction::Rtl {
        Point { x: p.container_outer_width - (location.x + size.width) - p.resolved_border.right, y: location.y - p.resolved_border.top }
    } else {
        Point { x: location.x - p.resolved_border.left, y: location.y - p.resolved_border.top }
    }
}

pub(super) fn perform_final_layout_on_in_flow_children<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    run_mode: RunMode,
    items: &mut [BlockItem],
    p: FlowParams,
    block_ctx: &mut BlockContext<'_>,
) -> FlowResult {
    let container_inner_width = (p.container_outer_width - p.resolved_content_box_inset.horizontal_axis_sum()).max(0.0);
    let container_percentage_resolution_height =
        p.container_percentage_resolution_height.maybe_sub(p.resolved_content_box_inset.vertical_axis_sum());
    let parent_size = Size { width: Some(container_inner_width), height: container_percentage_resolution_height };
    // Block-axis space is indefinite, which `MaxContent` represents.
    let available_space = Size { width: AvailableSpace::Definite(container_inner_width), height: AvailableSpace::MaxContent };

    if block_ctx.is_bfc_root() {
        block_ctx.set_width(p.container_outer_width);
        block_ctx.apply_content_box_inset([p.resolved_content_box_inset.left, p.resolved_content_box_inset.right]);
    }
    // A non-collapsing top margin resolves the strut here.
    if !p.own_margins_collapse_with_children.start {
        block_ctx.commit_strut();
    }

    let mut inflow_overflow_rect = Rect::ZERO;
    let mut committed_y_offset = p.resolved_content_box_inset.top;
    let mut y_offset_for_absolute = p.resolved_content_box_inset.top;
    let mut first_child_top_margin_set = CollapsibleMarginSet::ZERO;
    let mut active_collapsible_margin_set = CollapsibleMarginSet::ZERO;
    let mut is_collapsing_with_first_margin_set = true;
    let mut first_baseline: Option<f32> = None;
    let mut last_baseline: Option<f32> = None;
    // Margins of a self-collapsing cleared box stay inside the parent.
    let mut active_margin_set_has_clearance = false;
    let mut has_active_floats = block_ctx.has_active_floats(committed_y_offset);
    let mut has_out_of_flow = false;

    for item in items.iter_mut() {
        if item.position.is_out_of_flow() {
            let x = match p.direction {
                Direction::Ltr => p.resolved_content_box_inset.left,
                Direction::Rtl => p.container_outer_width - p.resolved_content_box_inset.right,
            };
            item.static_position = Point { x, y: y_offset_for_absolute };
            has_out_of_flow = true;
            continue;
        }
        let calc = |id, basis| tree.resolve_calc(id, basis);
        let item_margin = item.margin.map(|m| m.resolve(Some(container_inner_width), &calc));
        let item_non_auto_margin = item_margin.map(|m| m.unwrap_or(0.0));
        let item_non_auto_x_margin_sum = item_non_auto_margin.horizontal_axis_sum();
        let scrollbar_size = Size {
            width: if item.overflow.y == Overflow::Scroll { item.scrollbar_width } else { 0.0 },
            height: if item.overflow.x == Overflow::Scroll { item.scrollbar_width } else { 0.0 },
        };

        // Floats: shrink-to-fit, placed beside the flow.
        if let Some(float_direction) = item.float.direction() {
            has_active_floats = true;
            let available_width = (container_inner_width - item_non_auto_x_margin_sum).max(0.0);
            let (known_width, item_available_width) =
                match resolve_sizing_keyword(item.size_style.width.raw(), Some(available_width), Some(container_inner_width)) {
                    Some(SizingKeywordResolution::Measure(a)) => (None, a),
                    Some(SizingKeywordResolution::Exact(w)) => (Some(w), AvailableSpace::Definite(w)),
                    None => (None, AvailableSpace::Definite(available_width)),
                };
            // The margins of a float never collapse.
            let known_height = resolve_stretch_height(item.size_style.height, container_percentage_resolution_height, item_non_auto_margin, Line::FALSE);
            let float_inputs = LayoutInput {
                run_mode: RunMode::PerformLayout,
                sizing_mode: SizingMode::InherentSize,
                axis: RequestedAxis::Both,
                known_dimensions: Size { width: known_width, height: known_height },
                known_dimensions_are_definite: Size::TRUE,
                parent_size,
                available_space: Size { width: item_available_width, height: AvailableSpace::MaxContent },
                vertical_margins_are_collapsible: Line::FALSE,
                context_key: 0,
            };
            let item_layout = if run_mode == RunMode::ComputeSize {
                crate::compute::speculate(tree, |tree| super::compute_block_child(tree, item.node, float_inputs, None))
            } else {
                super::compute_block_child(tree, item.node, float_inputs, None)
            };
            let margin_box = item_layout.size + item_non_auto_margin.sum_axes();
            // Pending collapsible margins move the float unless they escape to the container's own margin.
            let adjoins_unresolved_strut = is_collapsing_with_first_margin_set && p.own_margins_collapse_with_children.start;
            let y_for_float = if adjoins_unresolved_strut { committed_y_offset } else { committed_y_offset + active_collapsible_margin_set.resolve() };
            let mut location = block_ctx.place_floated_box(margin_box, y_for_float, float_direction, item.clear, adjoins_unresolved_strut);
            location.y += item_non_auto_margin.top;
            location.x += item_non_auto_margin.left;
            if item_layout.oof != 0 {
                item.oof = item_layout.oof;
                has_out_of_flow = true;
            }
            item.final_layout = Some(Layout {
                order: item.order,
                size: item_layout.size,
                scrollable_overflow_rect: item_layout.scrollable_overflow_rect,
                scrollbar_size,
                location,
                padding: item.padding,
                border: item.border,
                margin: item_non_auto_margin,
            });
            inflow_overflow_rect = union(
                inflow_overflow_rect,
                scrollable_overflow_contribution(
                    contribution_location(&p, location, item_layout.size),
                    item_layout.size,
                    item_layout.scrollable_overflow_rect,
                    item.overflow,
                    item.contain,
                    p.is_scroll_container,
                ),
            );
            continue;
        }

        let mut y_margin_offset = 0.0f32;
        let mut item_avoids_floats = false;
        let mut item_pushed_below_float = false;
        let (stretch_width, float_avoiding_position, float_avoiding_width) = if item.is_in_same_bfc {
            ((container_inner_width - item_non_auto_x_margin_sum).max(0.0), Point::ZERO, 0.0)
        } else {
            if !is_collapsing_with_first_margin_set || !p.own_margins_collapse_with_children.start {
                y_margin_offset = active_collapsible_margin_set.collapse_with_margin(item_non_auto_margin.top).resolve();
            }
            let min_y = committed_y_offset + y_margin_offset;
            if has_active_floats || block_ctx.has_active_floats(min_y) {
                // Find the highest slot where the border box fits beside the floats.
                let x_margins = [item_non_auto_margin.left, item_non_auto_margin.right];
                let min_auto_width = -item_non_auto_x_margin_sum;
                let mut slot_segment = None;
                let slot = loop {
                    let slot = block_ctx.find_bfc_slot(min_y, x_margins, p.direction, item.clear, slot_segment);
                    let Some(segment_id) = slot.segment_id else { break slot };
                    let width = item
                        .size
                        .width
                        .unwrap_or(slot.stretch_width.max(min_auto_width).max(0.0))
                        .maybe_clamp(item.min_size.width, item.max_size.width);
                    if width <= slot.border_width + FIT_TOLERANCE {
                        break slot;
                    }
                    slot_segment = Some(segment_id);
                };
                if slot.y > min_y {
                    item_pushed_below_float = true;
                }
                has_active_floats = slot.segment_id.is_some();
                item_avoids_floats = true;
                (slot.stretch_width.max(min_auto_width).max(0.0), Point { x: slot.x, y: slot.y }, slot.border_width)
            } else {
                (
                    (container_inner_width - item_non_auto_x_margin_sum).max(0.0),
                    Point { x: p.resolved_content_box_inset.left, y: min_y },
                    container_inner_width,
                )
            }
        };

        // `justify-self` (css-align-3 §6.1.1) other than `normal` and `stretch`, which lay the box
        // out by the block rules (`stretch` differs only for tables and replaced boxes).
        let non_stretch_justify_self = match item.justify_self {
            Some(a) if a.keyword != AlignItemsKeyword::Stretch => Some(a.resolve_self_relative(tree.style(item.node).direction, p.direction, true)),
            _ => None,
        };

        let known_dimensions = if item.sizes_itself {
            Size::NONE
        } else {
            // Such alignment makes an auto width `fit-content` instead of `stretch`.
            let width_style = match non_stretch_justify_self {
                Some(_) if item.size_style.width.is_auto() => Length::FIT_CONTENT,
                _ => item.size_style.width.raw(),
            };
            let keyword_width =
                resolve_sizing_keyword(width_style, Some(stretch_width), Some(container_inner_width)).map(|r| match r {
                    SizingKeywordResolution::Exact(w) => w,
                    SizingKeywordResolution::Measure(avail) => tree.measure_child_size(
                        item.node,
                        Size::NONE,
                        parent_size,
                        Size { width: avail, height: AvailableSpace::MaxContent },
                        SizingMode::InherentSize,
                        AbsoluteAxis::Horizontal,
                        Line::TRUE,
                    ),
                });
            let keyword_height = resolve_stretch_height(
                item.size_style.height,
                container_percentage_resolution_height,
                item_non_auto_margin,
                p.children_margins_adjoin_own_edges,
            );
            item.size
                .map_width(|w| Some(w.or(keyword_width).unwrap_or(stretch_width).maybe_clamp(item.min_size.width, item.max_size.width)))
                .map_height(|h| h.or(keyword_height))
                .maybe_clamp(item.min_size, item.max_size)
                // The border box never gets smaller than the padding and border.
                .maybe_max(item.padding_border_sum.map(Some))
        };

        let mut inputs = LayoutInput {
            run_mode,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Both,
            known_dimensions,
            known_dimensions_are_definite: Size::TRUE,
            parent_size,
            available_space: available_space.map_width(|_| AvailableSpace::Definite(stretch_width)),
            vertical_margins_are_collapsible: if item.is_in_same_bfc { Line::TRUE } else { Line::FALSE },
            context_key: 0,
        };
        let clear_threshold = block_ctx.cleared_threshold(item.clear);
        let clear_pos = clear_threshold.unwrap_or(f32::NEG_INFINITY);

        // Height taken by floats inside a same-BFC child, from its border-top.
        let mut child_float_contribution = f32::NEG_INFINITY;
        let item_layout = if item.is_in_same_bfc {
            // The border box is at least as wide as its padding and border.
            let width = known_dimensions.width.unwrap_or(stretch_width).max(item.padding_border_sum.width);
            let inset_left = item_non_auto_margin.left + p.content_box_inset.left;
            let inset_right = p.container_outer_width - width - inset_left;
            // The child's own collapsed-through margins are not known yet.
            let estimated_y = committed_y_offset + active_collapsible_margin_set.collapse_with_margin(item_non_auto_margin.top).resolve();
            let mut child_ctx = block_ctx.sub_context(estimated_y.max(clear_pos), [inset_left, inset_right]);
            inputs.context_key = child_ctx.key();
            let output = super::compute_block_child(tree, item.node, inputs, Some(&mut child_ctx));
            child_float_contribution = child_ctx.floated_content_height_contribution();
            let child_top_adjoining = child_ctx.top_adjoining_floats();
            block_ctx.merge_adjoining_floats(child_top_adjoining);
            output
        } else {
            super::compute_block_child(tree, item.node, inputs, None)
        };
        let final_size = item_layout.size;
        if item_layout.oof != 0 {
            item.oof = item_layout.oof;
            has_out_of_flow = true;
        }

        let top_margin_set = item_layout.top_margin.collapse_with_margin(item_margin.top.unwrap_or(0.0));
        let bottom_margin_set = item_layout.bottom_margin.collapse_with_margin(item_margin.bottom.unwrap_or(0.0));

        // Auto margins share the free inline space; vertical auto margins are zero. Collapsing
        // moves the box but leaves its own used margins alone.
        let free_x_space = (stretch_width - final_size.width).max(0.0);
        let auto_margin_count = item_margin.left.is_none() as u8 + item_margin.right.is_none() as u8;
        let x_auto_margin = if auto_margin_count > 0 { free_x_space / auto_margin_count as f32 } else { 0.0 };
        let resolved_margin = Rect {
            left: item_margin.left.unwrap_or(x_auto_margin),
            right: item_margin.right.unwrap_or(x_auto_margin),
            top: item_non_auto_margin.top,
            bottom: item_non_auto_margin.bottom,
        };

        let calc = |id, basis| tree.resolve_calc(id, basis);
        let inset_basis = Size { width: Some(container_inner_width), height: container_percentage_resolution_height };
        let inset = item.inset.zip_size(inset_basis, |i, basis| i.resolve(basis, &calc));
        let inset_offset = Point {
            x: if p.direction == Direction::Rtl {
                inset.right.map(|x| -x).or(inset.left).unwrap_or(0.0)
            } else {
                inset.left.or(inset.right.map(|x| -x)).unwrap_or(0.0)
            },
            y: inset.top.or(inset.bottom.map(|y| -y)).unwrap_or(0.0),
        };

        if item.is_in_same_bfc && (!is_collapsing_with_first_margin_set || !p.own_margins_collapse_with_children.start) {
            y_margin_offset = active_collapsible_margin_set.collapse_with_set(top_margin_set).resolve();
        }

        // Clearance (CSS 2.2 §9.5.2): the border edge goes below the relevant floats.
        let mut has_clearance = false;
        if item.is_in_same_bfc
            && let Some(threshold) = clear_threshold
        {
            let hypothetical_y = committed_y_offset + active_collapsible_margin_set.collapse_with_set(top_margin_set).resolve();
            let forced_clearance = block_ctx.has_adjoining_float(item.clear);
            if forced_clearance || hypothetical_y < threshold {
                has_clearance = true;
                let escaped_margin = if is_collapsing_with_first_margin_set && p.own_margins_collapse_with_children.start {
                    active_collapsible_margin_set.resolve()
                } else {
                    0.0
                };
                y_margin_offset = threshold - committed_y_offset - escaped_margin;
            }
        }

        item.can_be_collapsed_through = item_layout.margins_can_collapse_through && !has_clearance;
        item.static_position = if item.is_in_same_bfc {
            Point {
                x: match p.direction {
                    Direction::Ltr => p.resolved_content_box_inset.left,
                    Direction::Rtl => p.container_outer_width - p.resolved_content_box_inset.right - final_size.width,
                },
                y: (committed_y_offset + active_collapsible_margin_set.resolve()).max(clear_pos),
            }
        } else {
            Point {
                x: match p.direction {
                    Direction::Ltr => float_avoiding_position.x,
                    Direction::Rtl => float_avoiding_position.x + float_avoiding_width - final_size.width,
                },
                y: float_avoiding_position.y,
            }
        };
        let mut location = if item.is_in_same_bfc {
            Point {
                x: match p.direction {
                    Direction::Ltr => p.resolved_content_box_inset.left + inset_offset.x + resolved_margin.left,
                    Direction::Rtl => {
                        p.container_outer_width - p.resolved_content_box_inset.right - final_size.width - resolved_margin.right + inset_offset.x
                    }
                },
                y: committed_y_offset + y_margin_offset + inset_offset.y,
            }
        } else {
            // Beside floats the non-auto margins are already inside the slot.
            let (extra_left, extra_right) = if item_avoids_floats {
                (resolved_margin.left - item_non_auto_margin.left, resolved_margin.right - item_non_auto_margin.right)
            } else {
                (resolved_margin.left, resolved_margin.right)
            };
            Point {
                x: match p.direction {
                    Direction::Ltr => float_avoiding_position.x + extra_left + inset_offset.x,
                    Direction::Rtl => float_avoiding_position.x + float_avoiding_width - final_size.width - extra_right + inset_offset.x,
                },
                y: float_avoiding_position.y + inset_offset.y,
            }
        };

        // Floats inside count from the child's final, margin-collapsed position.
        if item.is_in_same_bfc {
            block_ctx.add_child_floated_content_height_contribution(location.y - inset_offset.y + child_float_contribution);
        }

        // `justify-self` aligns the margin box and beats `text-align`; auto margins beat `justify-self`.
        let item_outer_width = final_size.width + resolved_margin.horizontal_axis_sum();
        if let Some(justify_self) = non_stretch_justify_self {
            if item_margin.left.is_some() && item_margin.right.is_some() {
                // Beside floats the alignment container is the slot, already inset by the margins.
                let (container_start, free) = if item_avoids_floats {
                    (float_avoiding_position.x - resolved_margin.left, float_avoiding_width - final_size.width)
                } else {
                    (p.resolved_content_box_inset.left, container_inner_width - item_outer_width)
                };
                location.x = container_start + resolved_margin.left + inset_offset.x + justify_offset(justify_self, free, p.direction);
            }
        } else if item_outer_width < container_inner_width {
            let free = container_inner_width - item_outer_width;
            match (p.text_align, p.direction) {
                (TextAlign::Auto, _) | (TextAlign::LegacyLeft, Direction::Ltr) | (TextAlign::LegacyRight, Direction::Rtl) => {}
                (TextAlign::LegacyLeft, Direction::Rtl) => location.x -= free,
                (TextAlign::LegacyRight, Direction::Ltr) => location.x += free,
                (TextAlign::LegacyCenter, Direction::Ltr) => location.x += free / 2.0,
                (TextAlign::LegacyCenter, Direction::Rtl) => location.x -= free / 2.0,
            }
        }

        // Scroll containers clamp their baselines to the border box, synthesising them from the bottom edge.
        let child_baseline = |b: Option<f32>| {
            if item.overflow.y.is_scroll_container() { Some(b.unwrap_or(final_size.height).min(final_size.height).max(0.0)) } else { b }
        };
        if first_baseline.is_none() {
            first_baseline = child_baseline(item_layout.baselines.first).map(|b| location.y + b);
        }
        if let Some(b) = child_baseline(item_layout.baselines.last) {
            last_baseline = Some(location.y + b);
        }

        // Used auto margins reach the containing block's content edges past any floats
        // (csswg-drafts#9174); the alignment above used the float-relative ones.
        let mut used_margin = resolved_margin;
        if item_avoids_floats {
            if item_margin.left.is_none() {
                used_margin.left += float_avoiding_position.x - p.resolved_content_box_inset.left;
            }
            if item_margin.right.is_none() {
                used_margin.right +=
                    p.container_outer_width - p.resolved_content_box_inset.right - float_avoiding_position.x - float_avoiding_width;
            }
        }

        item.final_layout = Some(Layout {
            order: item.order,
            size: final_size,
            scrollable_overflow_rect: item_layout.scrollable_overflow_rect,
            scrollbar_size,
            location,
            padding: item.padding,
            border: item.border,
            margin: used_margin,
        });
        inflow_overflow_rect = union(
            inflow_overflow_rect,
            scrollable_overflow_contribution(
                contribution_location(&p, location, final_size),
                final_size,
                item_layout.scrollable_overflow_rect,
                item.overflow,
                item.contain,
                p.is_scroll_container,
            ),
        );

        // A margin separated from the container's by a float or clearance no longer collapses outward.
        if is_collapsing_with_first_margin_set && (item_pushed_below_float || has_clearance) {
            is_collapsing_with_first_margin_set = false;
        } else if is_collapsing_with_first_margin_set {
            if item.can_be_collapsed_through {
                first_child_top_margin_set = first_child_top_margin_set.collapse_with_set(top_margin_set).collapse_with_set(bottom_margin_set);
            } else {
                first_child_top_margin_set = first_child_top_margin_set.collapse_with_set(top_margin_set);
                is_collapsing_with_first_margin_set = false;
            }
        }

        if item.can_be_collapsed_through {
            active_collapsible_margin_set = active_collapsible_margin_set.collapse_with_set(top_margin_set).collapse_with_set(bottom_margin_set);
            y_offset_for_absolute = committed_y_offset + final_size.height + y_margin_offset;
        } else {
            committed_y_offset = location.y - inset_offset.y + final_size.height;
            if has_clearance && item_layout.margins_can_collapse_through {
                // The border edge stays cleared while the collapsed margin extends below it.
                committed_y_offset -= top_margin_set.resolve();
                active_collapsible_margin_set = top_margin_set.collapse_with_set(bottom_margin_set);
                active_margin_set_has_clearance = true;
            } else {
                active_collapsible_margin_set = bottom_margin_set;
                active_margin_set_has_clearance = false;
            }
            y_offset_for_absolute = committed_y_offset + active_collapsible_margin_set.resolve();
            block_ctx.commit_strut();
        }
    }

    let last_child_bottom_margin_set = if active_margin_set_has_clearance { CollapsibleMarginSet::ZERO } else { active_collapsible_margin_set };
    let bottom_y_margin_offset = if active_margin_set_has_clearance {
        active_collapsible_margin_set.resolve()
    } else if p.own_margins_collapse_with_children.end {
        0.0
    } else {
        last_child_bottom_margin_set.resolve()
    };
    committed_y_offset += p.resolved_content_box_inset.bottom + bottom_y_margin_offset;

    FlowResult {
        overflow_rect: inflow_overflow_rect,
        content_height: committed_y_offset.max(0.0),
        first_child_top_margin_set,
        last_child_bottom_margin_set,
        first_baseline,
        last_baseline,
        has_out_of_flow,
    }
}
