//! Final placement of in-flow items.

use super::axis::{FlexAxisRect, FlexAxisSize, FlexAxisSum};
use super::{AlgoConstants, FlexItem, FlexLine, item_known_dimension_definiteness};
use crate::compute::common::overflow::{scrollable_overflow_contribution, union};
use crate::compute::scratch::Scratch;
use crate::tree::{ChildRequest, LayoutOutput};
use crate::geometry::{AvailableSpace, Line, Point, Rect, Size};
use crate::style::{Direction, Overflow};
use crate::tree::{CacheAccess, Layout, LayoutInput, LayoutTree, RequestedAxis, RunMode, SizingMode};

/// The final-layout question asked of an item.
fn item_request(item: &FlexItem, c: &AlgoConstants) -> ChildRequest {
    ChildRequest {
        node: item.node,
        input: LayoutInput {
            run_mode: RunMode::PerformLayout,
            sizing_mode: SizingMode::ContentSize,
            axis: RequestedAxis::Both,
            known_dimensions: item.target_size.map(Some),
            known_dimensions_are_definite: item_known_dimension_definiteness(c, item),
            parent_size: c.node_inner_size,
            available_space: c.container_size.map(AvailableSpace::Definite),
            vertical_margins_are_collapsible: Line::FALSE,
            context_key: 0,
        },
    }
}

fn calculate_flex_item<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    item: &mut FlexItem,
    out: LayoutOutput,
    total_offset_main: &mut f32,
    total_offset_cross: f32,
    line_offset_cross: f32,
    total_overflow_rect: &mut Rect<f32>,
    c: &AlgoConstants,
) {
    let dir = c.dir;
    let container_size = c.container_size;
    let border = c.border;
    let size = out.size;
    let is_rtl_row = dir.is_row() && c.layout_direction == Direction::Rtl;
    let is_rtl_column = dir.is_column() && c.layout_direction == Direction::Rtl;

    let main_relative_inset = if is_rtl_row {
        item.inset.main_end(dir).or(item.inset.main_start(dir).map(|p| -p)).unwrap_or(0.0)
    } else {
        item.inset.main_start(dir).or(item.inset.main_end(dir).map(|p| -p)).unwrap_or(0.0)
    };
    let cross_relative_inset = if is_rtl_column {
        item.inset.cross_end(dir).map(|p| -p).or(item.inset.cross_start(dir)).unwrap_or(0.0)
    } else {
        item.inset.cross_start(dir).or(item.inset.cross_end(dir).map(|p| -p)).unwrap_or(0.0)
    };
    let effective_line_offset_cross = if is_rtl_column { 0.0 } else { line_offset_cross };
    let offset_main = if is_rtl_row {
        *total_offset_main - item.offset_main - item.margin.main_end(dir) - main_relative_inset - size.width
    } else {
        *total_offset_main + item.offset_main + item.margin.main_start(dir) + main_relative_inset
    };
    let offset_cross =
        total_offset_cross + item.offset_cross + effective_line_offset_cross + item.margin.cross_start(dir) + cross_relative_inset;

    if dir.is_row() {
        let baseline_offset_cross = total_offset_cross + item.offset_cross + effective_line_offset_cross + item.margin.cross_start(dir);
        let baseline = out.baselines.first.unwrap_or(size.height);
        let inner_baseline = if item.overflow.y.is_scroll_container() { baseline.min(size.height).max(0.0) } else { baseline };
        item.baseline = baseline_offset_cross + inner_baseline;
    } else {
        let baseline_offset_main = *total_offset_main + item.offset_main + item.margin.main_start(dir);
        item.baseline = baseline_offset_main + out.baselines.first.unwrap_or(size.height);
    }

    let location = if dir.is_row() { Point { x: offset_main, y: offset_cross } } else { Point { x: offset_cross, y: offset_main } };
    let scrollbar_size = Size {
        width: if item.overflow.y == Overflow::Scroll { item.scrollbar_width } else { 0.0 },
        height: if item.overflow.x == Overflow::Scroll { item.scrollbar_width } else { 0.0 },
    };
    tree.set_unrounded_layout(
        item.node,
        &Layout {
            order: item.order,
            size,
            scrollable_overflow_rect: out.scrollable_overflow_rect,
            scrollbar_size,
            location,
            padding: item.padding,
            border: item.border,
            margin: item.margin,
        },
    );

    if is_rtl_row {
        *total_offset_main -= item.offset_main + item.margin.main_axis_sum(dir) + size.main(dir);
    } else {
        *total_offset_main += item.offset_main + item.margin.main_axis_sum(dir) + size.main(dir);
    }

    let contribution_location = if c.layout_direction == Direction::Rtl {
        Point { x: container_size.width - (location.x + size.width) - border.right, y: location.y - border.top }
    } else {
        Point { x: location.x - border.left, y: location.y - border.top }
    };
    *total_overflow_rect = union(
        *total_overflow_rect,
        scrollable_overflow_contribution(
            contribution_location,
            size,
            out.scrollable_overflow_rect,
            item.overflow,
            item.contain,
            c.is_scroll_container,
        ),
    );
}

fn calculate_layout_line<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    line: &FlexLine,
    items: &mut [FlexItem],
    outputs: &[LayoutOutput],
    total_offset_cross: &mut f32,
    overflow_rect: &mut Rect<f32>,
    c: &AlgoConstants,
) {
    let dir = c.dir;
    let padding_border = c.content_box_inset;
    let mut total_offset_main = if c.layout_direction == Direction::Rtl && dir.is_row() {
        c.container_size.width - padding_border.main_end(dir)
    } else {
        padding_border.main_start(dir)
    };
    let line_offset_cross = line.offset_cross;
    let is_rtl_column = c.layout_direction == Direction::Rtl && dir.is_column();
    if is_rtl_column {
        *total_offset_cross -= line_offset_cross + line.cross_size;
    }
    let (start, end) = (line.start, line.end);
    let line_items = line.items_mut(items);
    let line_outputs = &outputs[start..end];
    if dir.is_reverse() {
        for (item, out) in line_items.iter_mut().zip(line_outputs).rev() {
            calculate_flex_item(tree, item, *out, &mut total_offset_main, *total_offset_cross, line_offset_cross, overflow_rect, c);
        }
    } else {
        for (item, out) in line_items.iter_mut().zip(line_outputs) {
            calculate_flex_item(tree, item, *out, &mut total_offset_main, *total_offset_cross, line_offset_cross, overflow_rect, c);
        }
    }
    if !is_rtl_column {
        *total_offset_cross += line_offset_cross + line.cross_size;
    }
}

pub(super) fn final_layout_pass<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    lines: &[FlexLine],
    items: &mut [FlexItem],
    c: &AlgoConstants,
) -> Rect<f32> {
    let dir = c.dir;
    let mut total_offset_cross = if c.is_column && c.layout_direction == Direction::Rtl {
        c.container_size.width - c.content_box_inset.cross_end(dir)
    } else {
        c.content_box_inset.cross_start(dir)
    };
    let mut overflow_rect = Rect::ZERO;
    // Every item's final layout is independent of the others; ask for them as one batch.
    let requests: Scratch<ChildRequest> = Scratch::collect(items.iter().map(|item| item_request(item, c)));
    let mut outputs: Scratch<LayoutOutput> = Scratch::with_capacity(requests.len());
    tree.compute_child_layouts(&requests, &mut outputs);
    if c.is_wrap_reverse {
        for line in lines.iter().rev() {
            calculate_layout_line(tree, line, items, &outputs, &mut total_offset_cross, &mut overflow_rect, c);
        }
    } else {
        for line in lines.iter() {
            calculate_layout_line(tree, line, items, &outputs, &mut total_offset_cross, &mut overflow_rect, c);
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
    overflow_rect
}
