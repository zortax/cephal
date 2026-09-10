//! Per-child data the block algorithm works on.

use crate::compute::scratch::Scratch;
use crate::geometry::{MaybeMath, Point, Rect, Size};
use crate::style::{BoxSizing, Clear, Contain, Dimension, Float, LengthPercentageAuto, Overflow, Position};
use crate::tree::{Layout, LayoutTree, NodeId};

pub(super) struct BlockItem {
    pub node: NodeId,
    pub order: u32,
    pub is_table: bool,
    pub is_replaced: bool,
    pub is_in_same_bfc: bool,
    pub float: Float,
    pub clear: Clear,
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
        let padding = style.padding.map(|p| p.resolve_or_zero(node_inner_size.width, &calc));
        let border = style.border.map(|b| b.resolve_or_zero(node_inner_size.width, &calc));
        let pb_sum = (padding + border).sum_axes();
        let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { pb_sum } else { Size::ZERO };
        let position = style.position;
        let overflow = style.overflow;
        let float = style.float;
        let is_table = style.item_is_table;
        let contain = style.contain;
        let is_in_same_bfc = style.is_block()
            && !is_table
            && position != Position::Absolute
            && float == Float::None
            && !style.is_scroll_container()
            && !contain.establishes_independent_formatting_context();
        let resolve = |s: Size<crate::style::Length>| super::resolve_size(s, node_inner_size, &calc);
        items.push(BlockItem {
            node: child,
            order,
            is_table,
            is_replaced: style.item_is_replaced,
            is_in_same_bfc,
            float,
            clear: style.clear,
            size_style: style.size,
            size: resolve(style.size.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment),
            min_size: resolve(style.min_size.map(|v| v.raw()))
                .maybe_apply_aspect_ratio(aspect_ratio)
                .maybe_add(box_sizing_adjustment),
            max_size: resolve(style.max_size.map(|v| v.raw()))
                .maybe_apply_aspect_ratio(aspect_ratio)
                .maybe_add(box_sizing_adjustment),
            overflow,
            contain,
            scrollbar_width: style.scrollbar_width,
            position,
            inset: style.inset,
            margin: style.margin,
            padding,
            border,
            padding_border_sum: pb_sum,
            static_position: Point::ZERO,
            can_be_collapsed_through: false,
            final_layout: None,
        });
        order += 1;
    }
    items
}

crate::compute::scratch::pooled!(BlockItem);
