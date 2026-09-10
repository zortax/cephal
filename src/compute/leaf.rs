//! The box model around a measured leaf.

use crate::geometry::{AvailableSpace, MaybeMath, Rect, Size};
use crate::style::{BoxSizing, Overflow, Position};
use crate::tree::{
    Baselines, CollapsibleMarginSet, LayoutInput, LayoutOutput, LayoutTree, MeasureInput, NodeId, RunMode, SizingMode,
};

/// A leaf style's lengths resolved once; valid when none of them is a percentage or `calc()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeafBox {
    pub margin: Rect<f32>,
    pub padding: Rect<f32>,
    pub border: Rect<f32>,
    /// Border-box sizes after aspect ratio and box sizing.
    pub size: Size<Option<f32>>,
    pub min_size: Size<Option<f32>>,
    pub max_size: Size<Option<f32>>,
}

impl LeafBox {
    /// Resolves `style` with no parent size; `None` when any length needs one.
    pub fn from_style(style: &crate::style::Style) -> Option<Self> {
        let all = [
            style.size.width.raw(),
            style.size.height.raw(),
            style.min_size.width.raw(),
            style.min_size.height.raw(),
            style.max_size.width.raw(),
            style.max_size.height.raw(),
        ];
        let rects = [style.inset, style.margin];
        let lps = [style.padding, style.border];
        let percent = all.iter().any(|l| l.uses_percentage())
            || rects.iter().any(|r| [r.left, r.right, r.top, r.bottom].iter().any(|l| l.raw().uses_percentage()))
            || lps.iter().any(|r| [r.left, r.right, r.top, r.bottom].iter().any(|l| l.raw().uses_percentage()));
        if percent {
            return None;
        }
        let calc = |_, _| 0.0;
        let none: Size<Option<f32>> = Size::NONE;
        let r = resolve_box(style, none, &calc);
        Some(Self { margin: r.margin, padding: r.padding, border: r.border, size: r.size, min_size: r.min_size, max_size: r.max_size })
    }
}

/// The resolved box of a leaf against `parent_size`.
fn resolve_box(style: &crate::style::Style, parent_size: Size<Option<f32>>, calc: &impl Fn(crate::style::CalcId, f32) -> f32) -> LeafBox {
    // Vertical margins, padding and borders resolve against the inline size too.
    let margin = style.margin.map(|m| m.resolve_or_zero(parent_size.width, calc));
    let padding = style.padding.map(|p| p.resolve_or_zero(parent_size.width, calc));
    let border = style.border.map(|b| b.resolve_or_zero(parent_size.width, calc));
    let pb_sum = (padding + border).sum_axes();
    let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { pb_sum } else { Size::ZERO };
    let aspect_ratio = style.aspect_ratio;
    let size = Size {
        width: style.size.width.resolve(parent_size.width, calc),
        height: style.size.height.resolve(parent_size.height, calc),
    }
    .maybe_apply_aspect_ratio(aspect_ratio)
    .maybe_add(box_sizing_adjustment);
    let min_size = Size {
        width: style.min_size.width.resolve(parent_size.width, calc),
        height: style.min_size.height.resolve(parent_size.height, calc),
    }
    .maybe_apply_aspect_ratio(aspect_ratio)
    .maybe_add(box_sizing_adjustment);
    let max_size = Size {
        width: style.max_size.width.resolve(parent_size.width, calc),
        height: style.max_size.height.resolve(parent_size.height, calc),
    }
    .maybe_add(box_sizing_adjustment);
    LeafBox { margin, padding, border, size, min_size, max_size }
}

pub fn compute_leaf_layout<T: LayoutTree + ?Sized>(tree: &mut T, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
    let LayoutInput { known_dimensions, parent_size, available_space, sizing_mode, run_mode, .. } = inputs;
    let style = tree.style(node);
    let resolved = match tree.leaf_box(node) {
        Some(b) => b,
        None => {
            let calc = |id, basis| tree.resolve_calc(id, basis);
            resolve_box(style, parent_size, &calc)
        }
    };
    let LeafBox { margin, padding, border, .. } = resolved;
    let padding_border = padding + border;
    let pb_sum = padding_border.sum_axes();

    let (node_size, node_min_size, node_max_size, aspect_ratio) = match sizing_mode {
        SizingMode::ContentSize => (known_dimensions, Size::NONE, Size::NONE, None),
        SizingMode::InherentSize => (known_dimensions.or(resolved.size), resolved.min_size, resolved.max_size, style.aspect_ratio),
    };

    // A vertically scrolling box reserves horizontal space and vice versa.
    let overflow = style.overflow;
    let scrollbar_gutter = Size {
        width: if overflow.y == Overflow::Scroll { style.scrollbar_width } else { 0.0 },
        height: if overflow.x == Overflow::Scroll { style.scrollbar_width } else { 0.0 },
    };
    let content_box_inset = Rect {
        left: padding_border.left,
        right: padding_border.right + scrollbar_gutter.width,
        top: padding_border.top,
        bottom: padding_border.bottom + scrollbar_gutter.height,
    };

    let has_styles_preventing_being_collapsed_through = !style.is_block()
        || overflow.x.is_scroll_container()
        || overflow.y.is_scroll_container()
        || style.position == Position::Absolute
        || style.contain.establishes_independent_formatting_context()
        || padding.top > 0.0
        || padding.bottom > 0.0
        || border.top > 0.0
        || border.bottom > 0.0
        || matches!(node_size.height, Some(h) if h > 0.0)
        || matches!(node_min_size.height, Some(h) if h > 0.0);

    if run_mode == RunMode::ComputeSize
        && has_styles_preventing_being_collapsed_through
        && let Size { width: Some(width), height: Some(height) } = node_size
    {
        let size = Size { width, height }.maybe_clamp(node_min_size, node_max_size).f32_max(pb_sum);
        return LayoutOutput {
            size,
            scrollable_overflow_rect: Rect::ZERO,
            inflow_overflow_rect: Rect::ZERO,
            baselines: Baselines::NONE,
            top_margin: CollapsibleMarginSet::ZERO,
            bottom_margin: CollapsibleMarginSet::ZERO,
            margins_can_collapse_through: false,
        };
    }

    // Content-box available space: margins subtracted, clamped by min/max.
    let available_for_measure = Size {
        width: known_dimensions
            .width
            .map(AvailableSpace::Definite)
            .unwrap_or(available_space.width)
            .maybe_sub(margin.horizontal_axis_sum())
            .maybe_set(node_size.width)
            .map_definite(|w| w.maybe_clamp(node_min_size.width, node_max_size.width) - content_box_inset.horizontal_axis_sum()),
        height: known_dimensions
            .height
            .map(AvailableSpace::Definite)
            .unwrap_or(available_space.height)
            .maybe_sub(margin.vertical_axis_sum())
            .maybe_set(node_size.height)
            .map_definite(|h| h.maybe_clamp(node_min_size.height, node_max_size.height) - content_box_inset.vertical_axis_sum()),
    };

    let measure_known = match run_mode {
        RunMode::ComputeSize => known_dimensions,
        RunMode::PerformLayout => Size::NONE,
        RunMode::PerformHiddenLayout => unreachable!("hidden layout never measures"),
    };
    #[cfg(feature = "stats")]
    crate::compute::stats::bump(&crate::compute::stats::MEASURE);
    let measured = tree.measure(node, MeasureInput { known_dimensions: measure_known, available_space: available_for_measure });
    let measured_size = measured.size;

    let clamped = known_dimensions
        .or(node_size)
        .unwrap_or(measured_size + content_box_inset.sum_axes())
        .maybe_clamp(node_min_size, node_max_size);
    let size = Size {
        width: clamped.width,
        height: clamped.height.max(aspect_ratio.map_or(0.0, |r| clamped.width / r)),
    }
    .f32_max(pb_sum);

    let content_start = if tree.style(node).direction == crate::style::Direction::Rtl {
        (padding.right, padding.left)
    } else {
        (padding.left, padding.right)
    };
    let (is_scroll_x, is_scroll_y) = (overflow.x.is_scroll_container(), overflow.y.is_scroll_container());
    let scrollable_overflow_rect = Rect {
        left: 0.0,
        top: 0.0,
        right: content_start.0 + measured_size.width + if is_scroll_x || is_scroll_y { content_start.1 } else { 0.0 },
        bottom: padding.top + measured_size.height + if is_scroll_x || is_scroll_y { padding.bottom } else { 0.0 },
    };

    let baselines = Baselines {
        first: measured.baselines.first.map(|b| b + content_box_inset.top),
        last: measured.baselines.last.map(|b| b + content_box_inset.top),
    };

    LayoutOutput {
        size,
        scrollable_overflow_rect,
        inflow_overflow_rect: scrollable_overflow_rect,
        baselines,
        top_margin: CollapsibleMarginSet::ZERO,
        bottom_margin: CollapsibleMarginSet::ZERO,
        margins_can_collapse_through: !has_styles_preventing_being_collapsed_through
            && size.height == 0.0
            && measured_size.height == 0.0,
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
