//! Absolutely positioned children of a block container.

use super::items::BlockItem;
use crate::compute::LayoutTreeExt;
use crate::compute::absolute::{AbsoluteItemSizing, resolve_absolute_item_size};
use crate::compute::common::overflow::{out_of_flow_contribution, scrollable_overflow_contribution, union};
use crate::geometry::{AvailableSpace, Line, MaybeMath, Point, Rect, Size};
use crate::style::{Contain, Direction, Overflow};
use crate::tree::NodeId;
use crate::tree::{CacheAccess, Layout, LayoutTree, SizingMode};

pub(super) fn perform_absolute_layout_on_absolute_children<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    items: &[BlockItem],
    area_size: Size<f32>,
    area_offset: Point<f32>,
    direction: Direction,
    is_scroll_container: bool,
) -> Rect<f32> {
    let mut absolute_overflow_rect = Rect::ZERO;
    if !items.iter().any(|item| item.position.is_out_of_flow()) {
        return absolute_overflow_rect;
    }
    let mut record = crate::compute::reposition::AbsoluteContext::new(crate::compute::reposition::AbsoluteGeometry::Block {
        area_size,
        area_offset,
        direction,
        is_scroll_container,
    });
    for item in items.iter().filter(|item| item.position.is_out_of_flow()) {
        let contribution = layout_absolute_block_item(
            tree,
            item.node,
            item.order,
            item.static_position,
            item.overflow,
            item.contain,
            item.scrollbar_width,
            area_size,
            area_offset,
            direction,
            is_scroll_container,
        );
        record.contributions.push((item.node, contribution));
        absolute_overflow_rect = union(absolute_overflow_rect, contribution);
    }
    tree.set_absolute_context(node, record);
    absolute_overflow_rect
}

/// Places one absolutely positioned child; returns its overflow contribution.
#[allow(clippy::too_many_arguments)]
pub(crate) fn layout_absolute_block_item<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    order: u32,
    static_position: Point<f32>,
    overflow: Point<Overflow>,
    contain: Contain,
    scrollbar_width: f32,
    area_size: Size<f32>,
    area_offset: Point<f32>,
    direction: Direction,
    is_scroll_container: bool,
) -> Rect<f32> {
    {
        let AbsoluteItemSizing { inset: Rect { left, right, top, bottom }, margin, padding, border, min_size, max_size, final_size } =
            resolve_absolute_item_size(tree, node, area_size);

        let layout_output = tree.perform_child_layout(
            node,
            final_size.map(Some),
            area_size.map(Some),
            Size {
                width: AvailableSpace::Definite(area_size.width.maybe_clamp(min_size.width, max_size.width)),
                height: AvailableSpace::Definite(area_size.height.maybe_clamp(min_size.height, max_size.height)),
            },
            SizingMode::ContentSize,
            Line::FALSE,
        );

        let non_auto_margin = Rect {
            left: if left.is_some() { margin.left.unwrap_or(0.0) } else { 0.0 },
            right: if right.is_some() { margin.right.unwrap_or(0.0) } else { 0.0 },
            top: if top.is_some() { margin.top.unwrap_or(0.0) } else { 0.0 },
            bottom: if bottom.is_some() { margin.bottom.unwrap_or(0.0) } else { 0.0 },
        };
        // Auto margins only resolve when the corresponding inset is set.
        let auto_space = Point {
            x: right.map(|r| area_size.width - r - left.unwrap_or(0.0)).unwrap_or(final_size.width),
            y: bottom.map(|b| area_size.height - b - top.unwrap_or(0.0)).unwrap_or(final_size.height),
        };
        let free_space = Size {
            width: auto_space.x - final_size.width - non_auto_margin.horizontal_axis_sum(),
            height: auto_space.y - final_size.height - non_auto_margin.vertical_axis_sum(),
        };
        let auto_margin_size = Size {
            width: auto_margin(margin.left.is_none() as u8 + margin.right.is_none() as u8, free_space.width),
            height: auto_margin(margin.top.is_none() as u8 + margin.bottom.is_none() as u8, free_space.height),
        };
        let resolved_margin = Rect {
            left: margin.left.unwrap_or(auto_margin_size.width),
            right: margin.right.unwrap_or(auto_margin_size.width),
            top: margin.top.unwrap_or(auto_margin_size.height),
            bottom: margin.bottom.unwrap_or(auto_margin_size.height),
        };

        let x_offset = match (left, right) {
            (Some(left), Some(right)) => {
                if direction == Direction::Rtl {
                    area_size.width - final_size.width - right - resolved_margin.right
                } else {
                    left + resolved_margin.left
                }
            }
            (Some(left), None) => left + resolved_margin.left,
            (None, Some(right)) => area_size.width - final_size.width - right - resolved_margin.right,
            (None, None) => {
                if direction == Direction::Rtl {
                    static_position.x - final_size.width - resolved_margin.right - area_offset.x
                } else {
                    static_position.x + resolved_margin.left - area_offset.x
                }
            }
        };
        let location = Point {
            x: x_offset + area_offset.x,
            y: top
                .map(|t| t + resolved_margin.top)
                .or(bottom.map(|b| area_size.height - final_size.height - b - resolved_margin.bottom))
                .maybe_add(Some(area_offset.y))
                .unwrap_or(static_position.y + resolved_margin.top),
        };
        let scrollbar_size = Size {
            width: if overflow.y == Overflow::Scroll { scrollbar_width } else { 0.0 },
            height: if overflow.x == Overflow::Scroll { scrollbar_width } else { 0.0 },
        };
        tree.set_unrounded_layout(
            node,
            &Layout {
                order,
                size: final_size,
                scrollable_overflow_rect: layout_output.scrollable_overflow_rect,
                scrollbar_size,
                location,
                padding,
                border,
                margin: resolved_margin,
            },
        );

        let relative_location = if direction == Direction::Rtl {
            Point { x: area_size.width - (location.x - area_offset.x) - final_size.width, y: location.y - area_offset.y }
        } else {
            Point { x: location.x - area_offset.x, y: location.y - area_offset.y }
        };
        let contribution =
            scrollable_overflow_contribution(relative_location, final_size, layout_output.scrollable_overflow_rect, overflow, contain, is_scroll_container);
        out_of_flow_contribution(tree.style(node).position, contribution)
    }
}

/// Two auto margins with no free space resolve to zero rather than negative.
#[inline]
fn auto_margin(count: u8, free_space: f32) -> f32 {
    if count == 2 && free_space <= 0.0 {
        0.0
    } else if count > 0 {
        free_space / count as f32
    } else {
        0.0
    }
}
