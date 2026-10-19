//! Block flow layout.

mod context;
pub mod floats;
mod flow;
mod items;
mod width;

pub use context::{BlockContext, BlockFormattingContext};

use crate::compute::common::alignment::{apply_alignment_fallback, compute_alignment_offset};
use crate::compute::common::overflow::{scrollable_overflow_contribution, union};
#[allow(unused_imports)]
use crate::compute::common::overflow::union as _overflow_union;
use crate::compute::{LayoutTreeExt, compute_child_layout};
use crate::geometry::{Line, MaybeMath, Point, Rect, Size};
use crate::style::{BoxSizing, Direction, Overflow};
use crate::tree::{
    Baselines, CacheAccess, CollapsibleMarginSet, Layout, LayoutInput, LayoutOutput, LayoutTree, NodeId,
    RequestedAxis, RunMode, SizingMode,
};

pub fn compute_block_layout<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    inputs: LayoutInput,
    block_ctx: Option<&mut BlockContext<'_>>,
) -> LayoutOutput {
    let LayoutInput { known_dimensions, parent_size, run_mode, .. } = inputs;
    let style = tree.style(node);
    let calc = |id, basis| tree.resolve_calc(id, basis);

    let contain = style.contain;
    let establishes_new_bfc = style.is_scroll_container()
        || style.align_content.is_some()
        || contain.establishes_independent_formatting_context();
    let aspect_ratio = style.aspect_ratio;
    let padding = style.padding.map(|p| p.resolve_or_zero(parent_size.width, &calc));
    let border = style.border.map(|b| b.resolve_or_zero(parent_size.width, &calc));
    let padding_border_size = (padding + border).sum_axes();
    let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };

    let min_size = resolve_size(style.min_size.map(|v| v.raw()), parent_size, &calc)
        .maybe_apply_aspect_ratio(aspect_ratio)
        .maybe_add(box_sizing_adjustment);
    let max_size = resolve_size(style.max_size.map(|v| v.raw()), parent_size, &calc)
        .maybe_apply_aspect_ratio(aspect_ratio)
        .maybe_add(box_sizing_adjustment);
    let clamped_style_size = if inputs.sizing_mode == SizingMode::InherentSize {
        resolve_size(style.size.map(|v| v.raw()), parent_size, &calc)
            .maybe_apply_aspect_ratio(aspect_ratio)
            .maybe_add(box_sizing_adjustment)
            .maybe_clamp(min_size, max_size)
    } else {
        Size::NONE
    };

    // `max <= min` pins the size.
    let min_max_definite_size = min_size.zip_map(max_size, |min, max| match (min, max) {
        (Some(min), Some(max)) if max <= min => Some(min),
        _ => None,
    });
    let styled_known = known_dimensions.or(min_max_definite_size).or(clamped_style_size).maybe_max(padding_border_size.map(Some));

    // A parent collapsing margins with this box needs its margins, which depend on its content.
    let margins_wanted = inputs.vertical_margins_are_collapsible != Line::FALSE;
    if run_mode == RunMode::ComputeSize {
        if let Size { width: Some(width), height: Some(height) } = styled_known
            && !margins_wanted
        {
            return LayoutOutput::from_outer_size(Size { width, height });
        }
        if inputs.axis == RequestedAxis::Horizontal
            && let Some(width) = styled_known.width
        {
            return LayoutOutput::from_outer_size(Size { width, height: 0.0 });
        }
    }

    let inputs = LayoutInput { known_dimensions: styled_known, ..inputs };
    let mut output = match block_ctx {
        Some(inherited) if !establishes_new_bfc => compute_inner(tree, node, inputs, inherited),
        _ => {
            let mut bfc = BlockFormattingContext::new();
            let mut root = bfc.root_block_context();
            compute_inner(tree, node, inputs, &mut root)
        }
    };
    if contain.suppresses_baseline() {
        output.baselines = Baselines::NONE;
    }
    output
}

/// Resolves a size against a percentage basis per axis.
#[inline]
pub(crate) fn resolve_size(
    size: Size<crate::style::Length>,
    basis: Size<Option<f32>>,
    calc: &impl Fn(crate::style::CalcId, f32) -> f32,
) -> Size<Option<f32>> {
    Size { width: size.width.resolve(basis.width, calc), height: size.height.resolve(basis.height, calc) }
}

fn compute_inner<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    inputs: LayoutInput,
    block_ctx: &mut BlockContext<'_>,
) -> LayoutOutput {
    let LayoutInput { known_dimensions, parent_size, available_space, run_mode, vertical_margins_are_collapsible, .. } =
        inputs;
    let style = tree.style(node);
    let calc = |id, basis| tree.resolve_calc(id, basis);
    let raw_padding = style.padding;
    let raw_border = style.border;
    let raw_margin = style.margin;
    let aspect_ratio = style.aspect_ratio;
    let padding = raw_padding.map(|p| p.resolve_or_zero(parent_size.width, &calc));
    let border = raw_border.map(|b| b.resolve_or_zero(parent_size.width, &calc));
    let direction = style.direction;

    // A vertically scrolling box reserves horizontal space and vice versa.
    let gutters = style.overflow.transpose().map(|o| if o == Overflow::Scroll { style.scrollbar_width } else { 0.0 });
    let scrollbar_gutter = match direction {
        Direction::Ltr => Rect { top: 0.0, left: 0.0, right: gutters.x, bottom: gutters.y },
        Direction::Rtl => Rect { top: 0.0, left: gutters.x, right: 0.0, bottom: gutters.y },
    };
    let padding_border = padding + border;
    let padding_border_size = padding_border.sum_axes();
    let content_box_inset = padding_border + scrollbar_gutter;
    block_ctx.apply_content_box_inset([content_box_inset.left, content_box_inset.right]);

    let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };
    let size = resolve_size(style.size.map(|v| v.raw()), parent_size, &calc)
        .maybe_apply_aspect_ratio(aspect_ratio)
        .maybe_add(box_sizing_adjustment);
    let min_size = resolve_size(style.min_size.map(|v| v.raw()), parent_size, &calc)
        .maybe_apply_aspect_ratio(aspect_ratio)
        .maybe_add(box_sizing_adjustment);
    let max_size = resolve_size(style.max_size.map(|v| v.raw()), parent_size, &calc)
        .maybe_apply_aspect_ratio(aspect_ratio)
        .maybe_add(box_sizing_adjustment);

    // A definite size in one axis transfers through `aspect-ratio`; only a newly filled axis is clamped.
    let known_dimensions = {
        let derived = known_dimensions.maybe_apply_aspect_ratio(aspect_ratio).maybe_clamp(min_size, max_size);
        Size { width: known_dimensions.width.or(derived.width), height: known_dimensions.height.or(derived.height) }
    };
    let percentage_basis = Size {
        width: known_dimensions.width,
        height: known_dimensions.height.filter(|_| inputs.known_dimensions_are_definite.height),
    };
    let container_content_box_size = percentage_basis.maybe_sub(content_box_inset.sum_axes());

    let is_scroll_container = style.is_scroll_container();
    let establishes_new_bfc =
        is_scroll_container || style.align_content.is_some() || style.contain.establishes_independent_formatting_context();

    // Whether a child's margins could collapse through each block-axis edge of this box.
    let children_margins_adjoin_own_edges = Line {
        start: vertical_margins_are_collapsible.start
            && !establishes_new_bfc
            && !style.position.is_out_of_flow()
            && padding.top == 0.0
            && border.top == 0.0,
        end: vertical_margins_are_collapsible.end
            && !establishes_new_bfc
            && !style.position.is_out_of_flow()
            && padding.bottom == 0.0
            && border.bottom == 0.0,
    };
    let own_margins_collapse_with_children =
        Line { start: children_margins_adjoin_own_edges.start, end: children_margins_adjoin_own_edges.end && size.height.is_none() };
    let has_styles_preventing_being_collapsed_through = !style.is_block()
        || block_ctx.is_bfc_root()
        || establishes_new_bfc
        || style.position.is_out_of_flow()
        || padding.top > 0.0
        || padding.bottom > 0.0
        || border.top > 0.0
        || border.bottom > 0.0;

    let text_align = style.text_align;
    let justify_items = style.justify_items;
    let align_content = style.align_content;

    let mut items = items::generate_item_list(tree, node, container_content_box_size, justify_items);

    let container_outer_width = known_dimensions.width.unwrap_or_else(|| {
        let available_width = available_space.width.maybe_sub(content_box_inset.horizontal_axis_sum());
        let intrinsic_width = width::determine_content_based_container_width(tree, &items, available_width)
            + content_box_inset.horizontal_axis_sum();
        intrinsic_width.maybe_clamp(min_size.width, max_size.width).maybe_max(Some(padding_border_size.width))
    });

    if let (RunMode::ComputeSize, Some(height), false) = (run_mode, known_dimensions.height, vertical_margins_are_collapsible != Line::FALSE) {
        return LayoutOutput::from_outer_size(Size { width: container_outer_width, height });
    }
    if run_mode == RunMode::ComputeSize && inputs.axis == RequestedAxis::Horizontal {
        return LayoutOutput::from_outer_size(Size { width: container_outer_width, height: 0.0 });
    }

    // Under `ContentSize` the box's own `height` and `min-height` are ignored, as a basis too
    // (the content size suggestion must not depend on them, css-flexbox-1 §4.5).
    let container_percentage_resolution_height = percentage_basis
        .height
        .or(if inputs.sizing_mode == SizingMode::InherentSize { size.height.maybe_max(min_size.height) } else { None });

    // Percentage padding and borders resolve against the containing block's width.
    let percentage_resolution_width = parent_size.width.unwrap_or(container_outer_width);
    let calc = |id, basis| tree.resolve_calc(id, basis);
    let resolved_padding = raw_padding.map(|p| p.resolve_or_zero(Some(percentage_resolution_width), &calc));
    let resolved_border = raw_border.map(|b| b.resolve_or_zero(Some(percentage_resolution_width), &calc));
    let resolved_content_box_inset = resolved_padding + resolved_border + scrollbar_gutter;

    let flow = flow::perform_final_layout_on_in_flow_children(
        tree,
        run_mode,
        &mut items,
        flow::FlowParams {
            container_outer_width,
            container_percentage_resolution_height,
            content_box_inset,
            resolved_content_box_inset,
            resolved_border,
            text_align,
            direction,
            own_margins_collapse_with_children,
            children_margins_adjoin_own_edges,
            is_scroll_container,
        },
        block_ctx,
    );
    let mut inflow_overflow_rect = flow.overflow_rect;
    let mut intrinsic_outer_height = flow.content_height;
    let mut first_baseline = flow.first_baseline;
    let mut last_baseline = flow.last_baseline;
    // A BFC root contains its floats.
    if block_ctx.is_bfc_root() || establishes_new_bfc {
        intrinsic_outer_height = intrinsic_outer_height.max(block_ctx.floated_content_height_contribution());
    }

    let container_outer_height = known_dimensions
        .height
        .unwrap_or(intrinsic_outer_height.maybe_clamp(min_size.height, max_size.height))
        .maybe_max(Some(padding_border_size.height));
    let final_outer_size = Size { width: container_outer_width, height: container_outer_height };

    // When `min-height` decides the used height the last child's bottom margin stays inside.
    let height_constrained_by_min_height = matches!(min_size.height, Some(h) if h > 0.0 && h >= container_outer_height);
    let own_bottom_margin_collapses_with_children = own_margins_collapse_with_children.end && !height_constrained_by_min_height;

    // `align-content` shifts the whole stack of in-flow children as one alignment subject.
    if let Some(align_content) = align_content {
        let container_inner_height = container_outer_height - resolved_content_box_inset.vertical_axis_sum();
        let inflow_content_height = intrinsic_outer_height - resolved_content_box_inset.vertical_axis_sum();
        let free_space = container_inner_height - inflow_content_height;
        let group_offset = || {
            let keyword = apply_alignment_fallback(free_space, 1, align_content);
            compute_alignment_offset(free_space, 1, 0.0, keyword, true, false)
        };
        if items.iter().any(|item| item.final_layout.is_some()) {
            let group_offset = group_offset();
            first_baseline = first_baseline.map(|b| b + group_offset);
            last_baseline = last_baseline.map(|b| b + group_offset);
            for item in items.iter_mut() {
                if let Some(layout) = item.final_layout.as_mut() {
                    layout.location.y += group_offset;
                }
                // Static positions move with the in-flow content.
                if item.position.is_out_of_flow() {
                    item.static_position.y += group_offset;
                }
            }
            inflow_overflow_rect = Rect::ZERO;
            for item in items.iter() {
                if let Some(layout) = item.final_layout.as_ref() {
                    let location = if direction == Direction::Rtl {
                        Point {
                            x: container_outer_width - (layout.location.x + layout.size.width) - resolved_border.right,
                            y: layout.location.y - resolved_border.top,
                        }
                    } else {
                        Point { x: layout.location.x - resolved_border.left, y: layout.location.y - resolved_border.top }
                    };
                    inflow_overflow_rect = union(
                        inflow_overflow_rect,
                        scrollable_overflow_contribution(
                            location,
                            layout.size,
                            layout.scrollable_overflow_rect,
                            item.overflow,
                            item.contain,
                            is_scroll_container,
                        ),
                    );
                }
            }
        } else if flow.has_out_of_flow {
            // Without in-flow content the static positions still sit where it would start.
            let group_offset = group_offset();
            for item in items.iter_mut().filter(|item| item.position.is_out_of_flow()) {
                item.static_position.y += group_offset;
            }
        }
    }

    let all_in_flow_children_can_be_collapsed_through =
        items.iter().all(|item| item.is_floated() || item.position.is_out_of_flow() || item.can_be_collapsed_through);
    // Margins collapse through a box whose *used* height is zero, as in browsers (CSS 2.2 §8.3.1
    // speaks of computed heights).
    let can_be_collapsed_through =
        !has_styles_preventing_being_collapsed_through && container_outer_height == 0.0 && all_in_flow_children_can_be_collapsed_through;

    let calc = |id, basis| tree.resolve_calc(id, basis);
    let mut output = LayoutOutput {
        size: final_outer_size,
        scrollable_overflow_rect: Rect::ZERO,
        inflow_overflow_rect: Rect::ZERO,
        baselines: Baselines { first: first_baseline, last: last_baseline },
        top_margin: if own_margins_collapse_with_children.start {
            flow.first_child_top_margin_set
        } else {
            CollapsibleMarginSet::from_margin(raw_margin.top.resolve_or_zero(parent_size.width, &calc))
        },
        bottom_margin: if own_bottom_margin_collapses_with_children {
            flow.last_child_bottom_margin_set
        } else {
            CollapsibleMarginSet::from_margin(raw_margin.bottom.resolve_or_zero(parent_size.width, &calc))
        },
        margins_can_collapse_through: can_be_collapsed_through,
        oof: 0,
    };

    // Parents need the margin metadata even when only sizing.
    if run_mode == RunMode::ComputeSize {
        return output;
    }

    for item in items.iter() {
        if let Some(layout) = item.final_layout.as_ref() {
            tree.set_unrounded_layout(item.node, layout);
        }
    }

    let (absolute_overflow_rect, oof) =
        if flow.has_out_of_flow && tree.speculation() == 0 {
            let entries = out_of_flow_entries(tree, node, &items, container_outer_width, resolved_content_box_inset, direction);
            let overflow = tree.style(node).overflow;
            crate::compute::oof::finish(tree, node, entries, || {
                let inset = resolved_border + scrollbar_gutter;
                crate::compute::oof::OofGeometry {
                    area_size: final_outer_size - inset.sum_axes(),
                    area_offset: Point { x: inset.left, y: inset.top },
                    direction,
                    overflow,
                    #[cfg(feature = "grid")]
                    grid: None,
                }
            })
        } else {
            crate::compute::oof::finish_none(tree, node);
            (Rect::ZERO, 0)
        };
    output.oof = oof;

    // A scroll container's own end padding is part of its scrollable overflow.
    if is_scroll_container {
        inflow_overflow_rect.right += if direction == Direction::Rtl { resolved_padding.left } else { resolved_padding.right };
        inflow_overflow_rect.bottom += resolved_padding.bottom;
    }
    output = output.with_overflow(inflow_overflow_rect, absolute_overflow_rect);

    let mut hidden = tree.children(node).enumerate().filter(|(_, c)| tree.style(*c).generates_no_box()).peekable();
    if hidden.peek().is_some() {
        let hidden: crate::compute::scratch::Scratch<(usize, NodeId)> = crate::compute::scratch::Scratch::collect(hidden);
        for &(order, child) in &hidden {
            tree.set_unrounded_layout(child, &Layout::with_order(order as u32));
            tree.perform_child_layout(child, Size::NONE, Size::NONE, Size::MAX_CONTENT, SizingMode::InherentSize, Line::FALSE);
        }
    }

    output
}

/// The block's out-of-flow record (see [`crate::compute::oof`]): a static position for each
/// out-of-flow child, spanning the content box inline and at its hypothetical position in the
/// block axis, aligned per its self-alignment (css-position-3 §4.6); and each in-flow child that
/// passes candidates through.
fn out_of_flow_entries<T: LayoutTree + ?Sized>(
    tree: &T,
    node: NodeId,
    items: &[items::BlockItem],
    container_outer_width: f32,
    content_box_inset: Rect<f32>,
    direction: Direction,
) -> Vec<crate::compute::oof::OofEntry> {
    use crate::compute::oof::{AxisStaticPosition, OofEntry, position_bit};
    let justify_items = tree.style(node).justify_items;
    let inline_area = Line { start: content_box_inset.left, end: container_outer_width - content_box_inset.right };
    let mut entries = Vec::new();
    for item in items {
        if item.position.is_out_of_flow() {
            let style = tree.style(item.node);
            let justify_self = style.justify_self.or(justify_items).map(|a| a.resolve_self_relative(style.direction, direction, true));
            // Block containers have no `align-items`: `auto` is `normal`.
            let align_self = style.align_self.map(|a| a.resolve_self_relative(style.direction, direction, false));
            let block_area = Line { start: item.static_position.y, end: item.static_position.y };
            entries.push(OofEntry::Seed {
                node: item.node,
                order: item.order,
                bit: position_bit(item.position),
                static_position: Point {
                    x: AxisStaticPosition::from_alignment(justify_self, inline_area, direction == Direction::Rtl),
                    y: AxisStaticPosition::from_alignment(align_self, block_area, false),
                },
            });
        } else if item.oof != 0
            && let Some(layout) = item.final_layout.as_ref()
        {
            entries.push(OofEntry::Through { node: item.node, order: item.order, bits: item.oof, location: layout.location });
        }
    }
    entries
}

/// Lays out one child inside this block's formatting context.
#[inline]
pub(crate) fn compute_block_child<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    child: NodeId,
    inputs: LayoutInput,
    block_ctx: Option<&mut BlockContext<'_>>,
) -> LayoutOutput {
    compute_child_layout(tree, child, inputs, block_ctx)
}
