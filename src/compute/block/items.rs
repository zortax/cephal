//! Per-child data the block algorithm works on.

use crate::compute::scratch::Scratch;
use crate::geometry::{MaybeMath, Point, Rect, Size};
use crate::style::{AlignItems, AlignItemsKeyword, AlignSelf, BoxSizing, Clear, Contain, Dimension, Float, LengthPercentageAuto, Overflow, Position};
use crate::tree::{Layout, LayoutTree, NodeId};

pub(super) struct BlockItem {
    pub node: NodeId,
    pub order: u32,
    /// A table or replaced box not stretched by `justify-self` resolves its own width.
    pub sizes_itself: bool,
    pub is_in_same_bfc: bool,
    pub float: Float,
    pub clear: Clear,
    /// `justify-self`, or the container's `justify-items` for `auto`; `None` is `normal`.
    pub justify_self: Option<AlignSelf>,
    /// Raw size styles, kept for sizing keywords.
    pub size_style: Size<Dimension>,
    pub size: Size<Option<f32>>,
    pub min_size: Size<Option<f32>>,
    pub max_size: Size<Option<f32>>,
    pub overflow: Point<Overflow>,
    pub contain: Contain,
    pub scrollbar_width: f32,
    pub position: Position,
    pub inset: Rect<LengthPercentageAuto>,
    /// Unresolved so auto margins survive.
    pub margin: Rect<LengthPercentageAuto>,
    pub padding: Rect<f32>,
    pub border: Rect<f32>,
    pub padding_border_sum: Size<f32>,
    pub static_position: Point<f32>,
    pub can_be_collapsed_through: bool,
    /// Held back so `align-content` can shift it before commit.
    pub final_layout: Option<Layout>,
    /// The out-of-flow bits of its final layout.
    pub oof: u8,
}

impl BlockItem {
    #[inline]
    pub fn is_floated(&self) -> bool {
        self.float != Float::None
    }
}

pub(super) fn generate_item_list<T: LayoutTree + ?Sized>(
    tree: &T,
    node: NodeId,
    node_inner_size: Size<Option<f32>>,
    justify_items: Option<AlignItems>,
) -> Scratch<BlockItem> {
    let calc = |id, basis| tree.resolve_calc(id, basis);
    let mut items = Scratch::with_capacity(tree.child_count(node));
    let mut order = 0u32;
    for child in tree.children(node) {
        let style = tree.style(child);
        if style.generates_no_box() {
            continue;
        }
        let aspect_ratio = style.aspect_ratio;
        // Lengths that resolve against nothing are the same against any basis.
        let plan = tree.style_plan(child);
        let (padding, border) = match plan {
            Some(p) => (p.padding, p.border),
            None => (
                style.padding.map(|p| p.resolve_or_zero(node_inner_size.width, &calc)),
                style.border.map(|b| b.resolve_or_zero(node_inner_size.width, &calc)),
            ),
        };
        let pb_sum = (padding + border).sum_axes();
        let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { pb_sum } else { Size::ZERO };
        let position = style.position;
        let overflow = style.overflow;
        let float = style.float;
        let is_table = style.item_is_table;
        let contain = style.contain;
        let is_in_same_bfc = style.is_block()
            && !is_table
            && !position.is_out_of_flow()
            && float == Float::None
            && !style.is_scroll_container()
            && !contain.establishes_independent_formatting_context()
            // css-align-3 §5.1.1: so does a non-`normal` `align-content`.
            && style.align_content.is_none();
        let justify_self = style.justify_self.or(justify_items);
        let resolve = |s: Size<crate::style::Length>| super::resolve_size(s, node_inner_size, &calc);
        let (size, min_size, max_size) = match plan {
            Some(p) => (p.size, p.min_size, p.max_size),
            None => (resolve(style.size.map(|v| v.raw())), resolve(style.min_size.map(|v| v.raw())), resolve(style.max_size.map(|v| v.raw()))),
        };
        items.push(BlockItem {
            node: child,
            order,
            sizes_itself: (is_table || style.item_is_replaced) && !justify_self.is_some_and(|a| a.keyword == AlignItemsKeyword::Stretch),
            is_in_same_bfc,
            float,
            clear: style.clear,
            justify_self,
            size_style: style.size,
            // A height the aspect ratio would give is left to the child, for whom it is only a
            // minimum when the content is taller (its automatic minimum size).
            size: if aspect_ratio.is_none() || ratio_height_is_minimum(style, size) { size } else { size.maybe_apply_aspect_ratio(aspect_ratio) }
                .maybe_add(box_sizing_adjustment),
            // Limits transfer through the ratio only to an axis the ratio gives the size of.
            min_size: transfer_limits(min_size, size, aspect_ratio).maybe_add(box_sizing_adjustment),
            max_size: transfer_limits(max_size, size, aspect_ratio).maybe_add(box_sizing_adjustment),
            overflow,
            contain,
            scrollbar_width: style.scrollbar_width,
            position,
            // Read only to offset an in-flow box, which only `position: relative` does.
            inset: if position.offsets_in_flow() { style.inset } else { Rect::splat(LengthPercentageAuto::AUTO) },
            margin: style.margin,
            padding,
            border,
            padding_border_sum: pb_sum,
            static_position: Point::ZERO,
            can_be_collapsed_through: false,
            final_layout: None,
            oof: 0,
        });
        order += 1;
    }
    items
}

crate::compute::scratch::pooled!(BlockItem);

/// Whether a height the aspect ratio gives from the width is only a minimum, the content's height
/// winning when larger (css-sizing-4 §5.2.1): an automatic height and min-height, no scrolling.
#[inline]
pub(crate) fn ratio_height_is_minimum(style: &crate::style::Style, size: Size<Option<f32>>) -> bool {
    style.aspect_ratio.is_some()
        && size.height.is_none()
        && style.size.height.is_auto()
        && style.min_size.height.is_auto()
        && !style.is_scroll_container()
        && !style.item_is_replaced
}

/// `limits` with the missing axis filled from the other through `ratio`, where `size` leaves that
/// axis to the ratio.
#[inline]
fn transfer_limits(limits: Size<Option<f32>>, size: Size<Option<f32>>, ratio: Option<f32>) -> Size<Option<f32>> {
    if ratio.is_none() {
        return limits;
    }
    let transferred = limits.maybe_apply_aspect_ratio(ratio);
    Size {
        width: if size.width.is_some() { limits.width } else { transferred.width },
        height: if size.height.is_some() { limits.height } else { transferred.height },
    }
}
