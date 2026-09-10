//! Track alignment and item alignment within grid areas.

use super::track::GridTrack;
use crate::compute::LayoutTreeExt;
use crate::compute::common::alignment::{apply_alignment_fallback, compute_alignment_offset, resolve_self_alignment_safety};
use crate::compute::common::overflow::scrollable_overflow_contribution;
use crate::compute::common::sizing_keyword::{SizingKeywordResolution, resolve_sizing_keyword};
use crate::geometry::{AbsoluteAxis, AvailableSpace, Line, MaybeMath, Point, Rect, Size};
use crate::style::{AlignContent, AlignItems, AlignItemsKeyword, AlignSelf, BoxSizing, Direction, Overflow, Position};
use crate::compute::layout_request;
use crate::tree::{CacheAccess, ChildRequest, Layout, LayoutOutput, LayoutTree, NodeId, SizingMode};

/// Positions tracks (gutters included) per `align-content`/`justify-content`.
pub(super) fn align_tracks(
    content_box_size: f32,
    padding: Line<f32>,
    border: Line<f32>,
    tracks: &mut [GridTrack],
    alignment: AlignContent,
    axis_is_reversed: bool,
) {
    let used: f32 = tracks.iter().map(|t| t.base_size).sum();
    let free_space = content_box_size - used;
    let origin = padding.start + border.start;
    let num_tracks = tracks.iter().skip(1).step_by(2).filter(|t| !t.is_collapsed).count();
    let keyword = apply_alignment_fallback(free_space, num_tracks, alignment);
    let keyword = if axis_is_reversed { keyword.reversed() } else { keyword };
    let empty_grid_offset = if num_tracks == 0 { compute_alignment_offset(free_space, num_tracks, 0.0, keyword, true, false) } else { 0.0 };
    let mut total_offset = origin + empty_grid_offset;
    let mut seen_track = false;
    let mut position = |i: usize, track: &mut GridTrack| {
        let is_gutter = i.is_multiple_of(2);
        let is_track = !is_gutter && !track.is_collapsed;
        let is_first = is_track && !seen_track;
        let offset = if is_track { compute_alignment_offset(free_space, num_tracks, 0.0, keyword, is_first, false) } else { 0.0 };
        track.offset = total_offset + offset;
        total_offset += offset + track.base_size;
        if is_track {
            seen_track = true;
        }
    };
    if axis_is_reversed {
        tracks.iter_mut().rev().enumerate().for_each(|(i, t)| position(i, t));
    } else {
        tracks.iter_mut().enumerate().for_each(|(i, t)| position(i, t));
    }
}

pub(super) struct ContainerAlignment {
    pub horizontal: Option<AlignItems>,
    pub vertical: Option<AlignItems>,
}

/// An item sized within its grid area, ready for its final layout.
pub(super) struct ItemPrep {
    node: NodeId,
    grid_area: Rect<f32>,
    grid_area_size: Size<f32>,
    area_minus_margins: Size<f32>,
    size: Size<Option<f32>>,
    min_size: Size<Option<f32>>,
    max_size: Size<Option<f32>>,
    margin: Rect<Option<f32>>,
    padding: Rect<f32>,
    border: Rect<f32>,
    inset_h: Line<Option<f32>>,
    inset_v: Line<Option<f32>>,
    alignment_h: AlignSelf,
    alignment_v: AlignSelf,
    justify_self: Option<AlignSelf>,
    align_self: Option<AlignSelf>,
    position: Position,
    baseline_shim: f32,
    overflow: Point<Overflow>,
    contain: crate::style::Contain,
    scrollbar_width: f32,
}

impl ItemPrep {
    /// The final-layout question for this item.
    pub fn request(&self) -> ChildRequest {
        layout_request(
            self.node,
            self.size,
            self.grid_area_size.map(Some),
            self.area_minus_margins.map(AvailableSpace::Definite),
            SizingMode::InherentSize,
        )
    }
}

/// Sizes, aligns and lays out one item in `grid_area`; returns its overflow contribution, y and height.
#[allow(clippy::too_many_arguments)]
pub(super) fn align_and_position_item<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    order: u32,
    grid_area: Rect<f32>,
    container_alignment: &ContainerAlignment,
    baseline_shim: f32,
    direction: Direction,
    container_border_box_width: f32,
    container_border: Rect<f32>,
    container_is_scroll_container: bool,
) -> (Rect<f32>, f32, f32) {
    let prep = prepare_item(tree, node, grid_area, container_alignment, baseline_shim, direction);
    let r = prep.request();
    let out = tree.compute_child_layout(r.node, r.input, None);
    place_item(tree, &prep, out, order, direction, container_border_box_width, container_border, container_is_scroll_container)
}

/// Resolves an item's size within `grid_area`; measures only for sizing keywords and absolute items.
pub(super) fn prepare_item<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    grid_area: Rect<f32>,
    container_alignment: &ContainerAlignment,
    baseline_shim: f32,
    direction: Direction,
) -> ItemPrep {
    let grid_area_size = Size { width: grid_area.right - grid_area.left, height: grid_area.bottom - grid_area.top };
    let style = tree.style(node);
    let calc = |id, basis| tree.resolve_calc(id, basis);
    let overflow = style.overflow;
    let contain = style.contain;
    let scrollbar_width = style.scrollbar_width;
    let aspect_ratio = style.aspect_ratio;
    let item_direction = style.direction;
    let justify_self = style.justify_self.map(|a| a.resolve_self_relative(item_direction, direction, true));
    let align_self = style.align_self.map(|a| a.resolve_self_relative(item_direction, direction, false));
    let container_h = container_alignment.horizontal.map(|a| a.resolve_self_relative(item_direction, direction, true));
    let container_v = container_alignment.vertical.map(|a| a.resolve_self_relative(item_direction, direction, false));
    let position = style.position;
    let inset_h = Line { start: style.inset.left.resolve(Some(grid_area_size.width), &calc), end: style.inset.right.resolve(Some(grid_area_size.width), &calc) };
    let inset_v = Line { start: style.inset.top.resolve(Some(grid_area_size.height), &calc), end: style.inset.bottom.resolve(Some(grid_area_size.height), &calc) };
    let padding = style.padding.map(|p| p.resolve_or_zero(Some(grid_area_size.width), &calc));
    let border = style.border.map(|b| b.resolve_or_zero(Some(grid_area_size.width), &calc));
    let padding_border_size = (padding + border).sum_axes();
    let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };
    let size_style = style.size;
    let resolve = |s: Size<crate::style::Length>| crate::compute::block::resolve_size(s, grid_area_size.map(Some), &calc);
    let inherent_size = resolve(size_style.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment);
    let min_size = resolve(style.min_size.map(|v| v.raw()))
        .maybe_add(box_sizing_adjustment)
        .or(padding_border_size.map(Some))
        .maybe_max(padding_border_size.map(Some))
        .maybe_apply_aspect_ratio(aspect_ratio);
    let max_size = resolve(style.max_size.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment);
    let alignment_h = justify_self.or(container_h).unwrap_or(if inherent_size.width.is_some() || size_style.width.is_sizing_keyword() {
        AlignSelf::START
    } else {
        AlignSelf::STRETCH
    });
    let alignment_v = align_self.or(container_v).unwrap_or(
        if inherent_size.height.is_some() || size_style.height.is_sizing_keyword() || aspect_ratio.is_some() {
            AlignSelf::START
        } else {
            AlignSelf::STRETCH
        },
    );
    let margin = style.margin.map(|m| m.resolve(Some(grid_area_size.width), &calc));
    let area_minus_margins = Size {
        width: grid_area_size.width.maybe_sub(margin.left).maybe_sub(margin.right).max(0.0),
        height: (grid_area_size.height.maybe_sub(margin.top).maybe_sub(margin.bottom) - baseline_shim).max(0.0),
    };

    let keyword_width = inherent_size
        .width
        .is_none()
        .then(|| resolve_sizing_keyword(size_style.width.raw(), Some(area_minus_margins.width), Some(grid_area_size.width)));
    let keyword_height = inherent_size
        .height
        .is_none()
        .then(|| resolve_sizing_keyword(size_style.height.raw(), Some(area_minus_margins.height), Some(grid_area_size.height)));
    let keyword_measured: Size<Option<f32>> = match (&keyword_width, &keyword_height) {
        (Some(Some(SizingKeywordResolution::Measure(aw))), Some(Some(SizingKeywordResolution::Measure(ah))))
            if position != Position::Absolute =>
        {
            tree.measure_child_size_both(
                node,
                Size::NONE,
                grid_area_size.map(Some),
                Size { width: *aw, height: *ah },
                SizingMode::InherentSize,
                Line::FALSE,
            )
            .map(Some)
        }
        _ => Size::NONE,
    };

    let width = match inherent_size.width {
        Some(w) => Some(w),
        None => 'w: {
            if position == Position::Absolute
                && let (Some(l), Some(r)) = (inset_h.start, inset_h.end)
            {
                break 'w Some((area_minus_margins.width - l - r).max(0.0));
            }
            if let Some(Some(res)) = keyword_width {
                break 'w Some(match res {
                    SizingKeywordResolution::Exact(w) => w,
                    SizingKeywordResolution::Measure(aw) => match keyword_measured.width {
                        Some(w) => w,
                        None => tree.measure_child_size(
                            node,
                            Size::NONE,
                            grid_area_size.map(Some),
                            Size { width: aw, height: AvailableSpace::Definite(area_minus_margins.height) },
                            SizingMode::InherentSize,
                            AbsoluteAxis::Horizontal,
                            Line::FALSE,
                        ),
                    },
                });
            }
            if margin.left.is_some() && margin.right.is_some() && alignment_h == AlignSelf::STRETCH && position != Position::Absolute {
                break 'w Some(area_minus_margins.width);
            }
            None
        }
    };
    let Size { width, height } = Size { width, height: inherent_size.height }.maybe_apply_aspect_ratio(aspect_ratio);
    let height = match height {
        Some(h) => Some(h),
        None => 'h: {
            if position == Position::Absolute
                && let (Some(t), Some(b)) = (inset_v.start, inset_v.end)
            {
                break 'h Some((area_minus_margins.height - t - b).max(0.0));
            }
            if let Some(Some(res)) = keyword_height {
                break 'h Some(match res {
                    SizingKeywordResolution::Exact(h) => h,
                    SizingKeywordResolution::Measure(ah) => match keyword_measured.height {
                        Some(h) => h,
                        None => tree.measure_child_size(
                            node,
                            Size { width, height: None },
                            grid_area_size.map(Some),
                            Size {
                                width: width.map_or(AvailableSpace::Definite(area_minus_margins.width), AvailableSpace::Definite),
                                height: ah,
                            },
                            SizingMode::InherentSize,
                            AbsoluteAxis::Vertical,
                            Line::FALSE,
                        ),
                    },
                });
            }
            if margin.top.is_some() && margin.bottom.is_some() && alignment_v == AlignSelf::STRETCH && position != Position::Absolute {
                break 'h Some(area_minus_margins.height);
            }
            None
        }
    };
    let size = Size { width, height }.maybe_apply_aspect_ratio(aspect_ratio).maybe_clamp(min_size, max_size);
    let size = if position == Position::Absolute && (size.width.is_none() || size.height.is_none()) {
        tree.measure_child_size_both(
            node,
            size,
            grid_area_size.map(Some),
            area_minus_margins.map(AvailableSpace::Definite),
            SizingMode::InherentSize,
            Line::FALSE,
        )
        .map(Some)
    } else {
        size
    };
    ItemPrep {
        node,
        grid_area,
        grid_area_size,
        area_minus_margins,
        size,
        min_size,
        max_size,
        margin,
        padding,
        border,
        inset_h,
        inset_v,
        alignment_h,
        alignment_v,
        justify_self,
        align_self,
        position,
        baseline_shim,
        overflow,
        contain,
        scrollbar_width,
    }
}

/// Aligns a laid-out item in its area and records its layout; returns its overflow contribution, y and height.
#[allow(clippy::too_many_arguments)]
pub(super) fn place_item<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    prep: &ItemPrep,
    out: LayoutOutput,
    order: u32,
    direction: Direction,
    container_border_box_width: f32,
    container_border: Rect<f32>,
    container_is_scroll_container: bool,
) -> (Rect<f32>, f32, f32) {
    let ItemPrep {
        node,
        grid_area,
        size,
        min_size,
        max_size,
        margin,
        padding,
        border,
        inset_h,
        inset_v,
        alignment_h,
        alignment_v,
        justify_self,
        align_self,
        position,
        baseline_shim,
        overflow,
        contain,
        scrollbar_width,
        ..
    } = *prep;
    let Size { width, height } = size.unwrap_or(out.size).maybe_clamp(min_size, max_size);

    let (x, x_margin) = align_item_within_area(
        Line { start: grid_area.left, end: grid_area.right },
        justify_self.unwrap_or(alignment_h),
        width,
        position,
        inset_h,
        Line { start: margin.left, end: margin.right },
        0.0,
        direction,
    );
    let (y, y_margin) = align_item_within_area(
        Line { start: grid_area.top, end: grid_area.bottom },
        align_self.unwrap_or(alignment_v),
        height,
        position,
        inset_v,
        Line { start: margin.top, end: margin.bottom },
        baseline_shim,
        Direction::Ltr,
    );
    let scrollbar_size = Size {
        width: if overflow.y == Overflow::Scroll { scrollbar_width } else { 0.0 },
        height: if overflow.x == Overflow::Scroll { scrollbar_width } else { 0.0 },
    };
    tree.set_unrounded_layout(
        node,
        &Layout {
            order,
            location: Point { x, y },
            size: Size { width, height },
            scrollable_overflow_rect: out.scrollable_overflow_rect,
            scrollbar_size,
            padding,
            border,
            margin: Rect { left: x_margin.start, right: x_margin.end, top: y_margin.start, bottom: y_margin.end },
        },
    );
    let contribution_location = if direction == Direction::Rtl {
        Point { x: container_border_box_width - (x + width) - container_border.right, y: y - container_border.top }
    } else {
        Point { x: x - container_border.left, y: y - container_border.top }
    };
    let contribution = scrollable_overflow_contribution(
        contribution_location,
        Size { width, height },
        out.scrollable_overflow_rect,
        overflow,
        contain,
        container_is_scroll_container,
    );
    (contribution, y, height)
}

/// Offset of an item's start edge within a grid area, plus its resolved margins.
#[allow(clippy::too_many_arguments)]
fn align_item_within_area(
    grid_area: Line<f32>,
    alignment: AlignSelf,
    resolved_size: f32,
    position: Position,
    inset: Line<Option<f32>>,
    margin: Line<Option<f32>>,
    baseline_shim: f32,
    direction: Direction,
) -> (f32, Line<f32>) {
    let non_auto_margin = Line { start: margin.start.unwrap_or(0.0) + baseline_shim, end: margin.end.unwrap_or(0.0) };
    let area_size = (grid_area.end - grid_area.start).max(0.0);
    let free_space = (area_size - resolved_size - non_auto_margin.sum()).max(0.0);
    let auto_count = margin.start.is_none() as u8 + margin.end.is_none() as u8;
    let auto_size = if auto_count > 0 { free_space / auto_count as f32 } else { 0.0 };
    let resolved_margin = Line { start: margin.start.unwrap_or(auto_size) + baseline_shim, end: margin.end.unwrap_or(auto_size) };
    let overflows = resolved_size + non_auto_margin.sum() > area_size;
    let keyword = resolve_self_alignment_safety(alignment, overflows);
    let rtl = direction == Direction::Rtl;
    let alignment_offset = match keyword {
        AlignItemsKeyword::Start | AlignItemsKeyword::FlexStart | AlignItemsKeyword::Baseline | AlignItemsKeyword::Stretch => {
            if rtl { area_size - resolved_size - resolved_margin.end } else { resolved_margin.start }
        }
        AlignItemsKeyword::End | AlignItemsKeyword::FlexEnd => {
            if rtl { resolved_margin.start } else { area_size - resolved_size - resolved_margin.end }
        }
        AlignItemsKeyword::Center => (area_size - resolved_size + resolved_margin.start - resolved_margin.end) / 2.0,
        AlignItemsKeyword::SelfStart | AlignItemsKeyword::SelfEnd => unreachable!("resolved earlier"),
    };
    let offset = if position == Position::Absolute {
        match (inset.start, inset.end) {
            (Some(start), Some(end)) => {
                if rtl { area_size - end - resolved_size - non_auto_margin.end } else { start + non_auto_margin.start }
            }
            (Some(start), None) => start + non_auto_margin.start,
            (None, Some(end)) => area_size - end - resolved_size - non_auto_margin.end,
            (None, None) => alignment_offset,
        }
    } else {
        alignment_offset
    };
    let mut start = grid_area.start + offset;
    if position == Position::Relative {
        let relative = if rtl { inset.end.map(|p| -p).or(inset.start) } else { inset.start.or(inset.end.map(|p| -p)) };
        start += relative.unwrap_or(0.0);
    }
    (start, resolved_margin)
}
