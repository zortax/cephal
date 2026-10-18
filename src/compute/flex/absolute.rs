//! Absolutely positioned children of a flex container.

use super::axis::FlexDir;
use super::AlgoConstants;
use super::axis::{FlexAxisRect, FlexAxisSize, FlexAxisSum};
use crate::style::{AlignItems, FlexDirection};
use crate::compute::LayoutTreeExt;
use crate::compute::absolute::{AbsoluteItemSizing, resolve_absolute_item_size};
use crate::compute::common::alignment::resolve_self_alignment_safety;
use crate::compute::common::overflow::{out_of_flow_contribution, scrollable_overflow_contribution, union};
use crate::geometry::{AvailableSpace, Line, MaybeMath, Point, Rect, Size};
use crate::style::{AlignContentKeyword, AlignItemsKeyword, Direction, JustifyContent, Overflow};
use crate::tree::{CacheAccess, Layout, LayoutTree, NodeId, SizingMode};

/// The container state absolute placement reads.
#[derive(Clone, Debug, PartialEq)]
pub struct AbsoluteFlexContext {
    pub dir: FlexDirection,
    pub layout_direction: Direction,
    pub is_row: bool,
    pub is_column: bool,
    pub is_wrap_reverse: bool,
    pub container_size: Size<f32>,
    pub border: Rect<f32>,
    pub content_box_inset: Rect<f32>,
    pub scrollbar_gutter: Point<f32>,
    pub node_inner_size: Size<Option<f32>>,
    pub align_items: AlignItems,
    pub justify_content: Option<JustifyContent>,
    pub is_scroll_container: bool,
}

impl AbsoluteFlexContext {
    pub(super) fn from_constants<D: FlexDir>(c: &AlgoConstants<D>) -> Self {
        Self {
            dir: c.dir.direction(),
            layout_direction: c.layout_direction,
            is_row: c.is_row,
            is_column: c.is_column,
            is_wrap_reverse: c.is_wrap_reverse,
            container_size: c.container_size,
            border: c.border,
            content_box_inset: c.content_box_inset,
            scrollbar_gutter: c.scrollbar_gutter,
            node_inner_size: c.node_inner_size,
            align_items: c.align_items,
            justify_content: c.justify_content,
            is_scroll_container: c.is_scroll_container,
        }
    }

}

pub(super) fn perform_absolute_layout_on_absolute_children<D: FlexDir, T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    constants: &AlgoConstants<D>,
) -> Rect<f32> {
    let c = AbsoluteFlexContext::from_constants(constants);
    let mut overflow_rect = Rect::ZERO;
    let absolute: crate::compute::scratch::Scratch<(usize, NodeId)> = crate::compute::scratch::Scratch::collect(
        tree
        .children(node)
        .enumerate()
        .filter(|(_, c)| {
            let s = tree.style(*c);
            !s.generates_no_box() && s.position.is_out_of_flow()
        }),
    );
    if absolute.is_empty() {
        return overflow_rect;
    }
    let mut record = crate::compute::reposition::AbsoluteContext::new(crate::compute::reposition::AbsoluteGeometry::Flex(c.clone()));
    for &(order, child) in &absolute {
        let contribution = layout_absolute_flex_child(tree, child, order as u32, &c);
        record.contributions.push((child, contribution));
        overflow_rect = union(overflow_rect, contribution);
    }
    tree.set_absolute_context(node, record);
    overflow_rect
}

/// Places one absolutely positioned child; returns its overflow contribution.
pub(crate) fn layout_absolute_flex_child<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    child: NodeId,
    order: u32,
    c: &AbsoluteFlexContext,
) -> Rect<f32> {
    let dir = c.dir;
    let container_width = c.container_size.width;
    let container_height = c.container_size.height;
    let inset_relative_size = c.container_size - c.border.sum_axes() - Size { width: c.scrollbar_gutter.x, height: c.scrollbar_gutter.y };
    {
        let style = tree.style(child);
        let overflow = style.overflow;
        let contain = style.contain;
        let scrollbar_width = style.scrollbar_width;
        let align_self = style.align_self.unwrap_or(c.align_items).resolve_self_relative(style.direction, c.layout_direction, c.is_column);

        let AbsoluteItemSizing { inset: Rect { left, right, top, bottom }, margin, padding, border, min_size, max_size, final_size } =
            resolve_absolute_item_size(tree, child, inset_relative_size);

        let available = Size {
            width: AvailableSpace::Definite(container_width.maybe_clamp(min_size.width, max_size.width)),
            height: AvailableSpace::Definite(container_height.maybe_clamp(min_size.height, max_size.height)),
        };
        let out = tree.perform_child_layout(child, final_size.map(Some), c.node_inner_size, available, SizingMode::ContentSize, Line::FALSE);

        let non_auto_margin = margin.map(|m| m.unwrap_or(0.0));
        let free_space = Size {
            width: c.container_size.width - final_size.width - non_auto_margin.horizontal_axis_sum(),
            height: c.container_size.height - final_size.height - non_auto_margin.vertical_axis_sum(),
        }
        .f32_max(Size::ZERO);
        let auto_margin_size = Size {
            width: {
                let n = margin.left.is_none() as u8 + margin.right.is_none() as u8;
                if n > 0 && left.is_some() && right.is_some() { free_space.width / n as f32 } else { 0.0 }
            },
            height: {
                let n = margin.top.is_none() as u8 + margin.bottom.is_none() as u8;
                if n > 0 && top.is_some() && bottom.is_some() { free_space.height / n as f32 } else { 0.0 }
            },
        };
        let resolved_margin = Rect {
            left: margin.left.unwrap_or(auto_margin_size.width),
            right: margin.right.unwrap_or(auto_margin_size.width),
            top: margin.top.unwrap_or(auto_margin_size.height),
            bottom: margin.bottom.unwrap_or(auto_margin_size.height),
        };

        let (start_main, end_main) = if c.is_row { (left, right) } else { (top, bottom) };
        let (start_cross, end_cross) = if c.is_row { (top, bottom) } else { (left, right) };
        let main_is_rtl = c.is_row && c.layout_direction == Direction::Rtl;
        let cross_is_rtl = !c.is_row && c.layout_direction == Direction::Rtl;
        let main_axis_flex_start_reversed = dir.is_reverse() ^ main_is_rtl;
        let cross_axis_flex_start_reversed = c.is_wrap_reverse ^ cross_is_rtl;
        let main_start_scrollbar_offset = if main_is_rtl { c.scrollbar_gutter.main(dir) } else { 0.0 };
        let cross_start_scrollbar_offset = if cross_is_rtl { c.scrollbar_gutter.cross(dir) } else { 0.0 };
        let main_end_scrollbar_offset = if main_is_rtl { 0.0 } else { c.scrollbar_gutter.main(dir) };
        let cross_end_scrollbar_offset = if cross_is_rtl { 0.0 } else { c.scrollbar_gutter.cross(dir) };
        let container_main = c.container_size.main(dir);
        let container_cross = c.container_size.cross(dir);
        let inset = c.content_box_inset;

        let offset_main = if start_main.is_some() || end_main.is_some() {
            if main_is_rtl && end_main.is_some() {
                container_main - c.border.main_end(dir) - main_end_scrollbar_offset - final_size.main(dir) - end_main.unwrap_or(0.0)
                    - resolved_margin.main_end(dir)
            } else if let Some(start) = start_main {
                start + c.border.main_start(dir) + main_start_scrollbar_offset + resolved_margin.main_start(dir)
            } else {
                container_main - c.border.main_end(dir) - main_end_scrollbar_offset - final_size.main(dir) - end_main.unwrap_or(0.0)
                    - resolved_margin.main_end(dir)
            }
        } else {
            let justify = c.justify_content.unwrap_or(JustifyContent::FLEX_START).keyword;
            let start_position = match justify {
                AlignContentKeyword::Start => !main_is_rtl,
                AlignContentKeyword::End => main_is_rtl,
                _ => true,
            };
            let at_start = inset.main_start(dir) + resolved_margin.main_start(dir);
            let at_end = container_main - inset.main_end(dir) - final_size.main(dir) - resolved_margin.main_end(dir);
            match (justify, main_axis_flex_start_reversed) {
                (AlignContentKeyword::SpaceBetween, false)
                | (AlignContentKeyword::Stretch, false)
                | (AlignContentKeyword::FlexStart, false)
                | (AlignContentKeyword::FlexEnd, true) => at_start,
                (AlignContentKeyword::Start | AlignContentKeyword::End, _) => {
                    if start_position { at_start } else { at_end }
                }
                (AlignContentKeyword::FlexEnd, false)
                | (AlignContentKeyword::FlexStart, true)
                | (AlignContentKeyword::Stretch, true)
                | (AlignContentKeyword::SpaceBetween, true) => at_end,
                (AlignContentKeyword::SpaceEvenly, _) | (AlignContentKeyword::SpaceAround, _) | (AlignContentKeyword::Center, _) => {
                    (container_main + inset.main_start(dir) - inset.main_end(dir) - final_size.main(dir)
                        + resolved_margin.main_start(dir)
                        - resolved_margin.main_end(dir))
                        / 2.0
                }
            }
        };

        let offset_cross = if start_cross.is_some() || end_cross.is_some() {
            if cross_is_rtl && end_cross.is_some() {
                container_cross - c.border.cross_end(dir) - cross_end_scrollbar_offset - final_size.cross(dir) - end_cross.unwrap_or(0.0)
                    - resolved_margin.cross_end(dir)
            } else if let Some(start) = start_cross {
                start + c.border.cross_start(dir) + cross_start_scrollbar_offset + resolved_margin.cross_start(dir)
            } else {
                container_cross - c.border.cross_end(dir) - cross_end_scrollbar_offset - final_size.cross(dir) - end_cross.unwrap_or(0.0)
                    - resolved_margin.cross_end(dir)
            }
        } else {
            let cross_overflows =
                final_size.cross(dir) + resolved_margin.cross_axis_sum(dir) > container_cross - inset.cross_axis_sum(dir);
            let keyword = resolve_self_alignment_safety(align_self, cross_overflows);
            let start_position = match keyword {
                AlignItemsKeyword::Start | AlignItemsKeyword::Baseline => !cross_is_rtl,
                AlignItemsKeyword::End => cross_is_rtl,
                _ => true,
            };
            let at_start = inset.cross_start(dir) + resolved_margin.cross_start(dir);
            let at_end = container_cross - inset.cross_end(dir) - final_size.cross(dir) - resolved_margin.cross_end(dir);
            match (keyword, cross_axis_flex_start_reversed) {
                (AlignItemsKeyword::Start | AlignItemsKeyword::End | AlignItemsKeyword::Baseline, _) => {
                    if start_position { at_start } else { at_end }
                }
                (AlignItemsKeyword::Stretch | AlignItemsKeyword::FlexStart, false) | (AlignItemsKeyword::FlexEnd, true) => at_start,
                (AlignItemsKeyword::Stretch | AlignItemsKeyword::FlexStart, true) | (AlignItemsKeyword::FlexEnd, false) => at_end,
                (AlignItemsKeyword::Center, _) => {
                    (container_cross + inset.cross_start(dir) - inset.cross_end(dir) - final_size.cross(dir)
                        + resolved_margin.cross_start(dir)
                        - resolved_margin.cross_end(dir))
                        / 2.0
                }
                (AlignItemsKeyword::SelfStart | AlignItemsKeyword::SelfEnd, _) => unreachable!("resolved above"),
            }
        };

        let location = if c.is_row { Point { x: offset_main, y: offset_cross } } else { Point { x: offset_cross, y: offset_main } };
        let scrollbar_size = Size {
            width: if overflow.y == Overflow::Scroll { scrollbar_width } else { 0.0 },
            height: if overflow.x == Overflow::Scroll { scrollbar_width } else { 0.0 },
        };
        tree.set_unrounded_layout(
            child,
            &Layout {
                order,
                size: final_size,
                scrollable_overflow_rect: out.scrollable_overflow_rect,
                scrollbar_size,
                location,
                padding,
                border,
                margin: resolved_margin,
            },
        );

        let area_offset = Point {
            x: c.border.left + if c.layout_direction == Direction::Rtl { c.scrollbar_gutter.x } else { 0.0 },
            y: c.border.top,
        };
        let relative_location = Point { x: location.x - area_offset.x, y: location.y - area_offset.y };
        let contribution_location = if c.layout_direction == Direction::Rtl {
            Point { x: inset_relative_size.width - relative_location.x - final_size.width, y: relative_location.y }
        } else {
            relative_location
        };
        let contribution =
            scrollable_overflow_contribution(contribution_location, final_size, out.scrollable_overflow_rect, overflow, contain, c.is_scroll_container);
        out_of_flow_contribution(tree.style(child).position, contribution)
    }
}
