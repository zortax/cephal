//! The box model around a measured leaf.

use crate::geometry::{AvailableSpace, MaybeMath, Rect, Size};
use crate::style::{BoxSizing, Overflow};
use crate::tree::{
    Baselines, CollapsibleMarginSet, LayoutInput, LayoutOutput, LayoutTree, MeasureInput, MeasureOutput, NodeId, RunMode, SizingMode,
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
    /// Padding plus border, summed per axis.
    pub padding_border_sum: Size<f32>,
    /// Padding, border and scrollbar gutter around the content box.
    pub content_box_inset: Rect<f32>,
    /// Inline-start and inline-end padding.
    pub content_start: (f32, f32),
    pub aspect_ratio: Option<f32>,
    /// A scroll container on either axis.
    pub scrolls: bool,
    /// The style alone keeps the box from collapsing through.
    pub style_prevents_collapse: bool,
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
        Some(resolve_box(style, none, &calc))
    }

    /// Resolves `style` against `parent_size`, for any style.
    pub fn resolve(
        style: &crate::style::Style,
        parent_size: Size<Option<f32>>,
        calc: &impl Fn(crate::style::CalcId, f32) -> f32,
    ) -> Self {
        resolve_box(style, parent_size, calc)
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

    // A vertically scrolling box reserves horizontal space and vice versa.
    let overflow = style.overflow;
    let scrollbar_gutter = Size {
        width: if overflow.y == Overflow::Scroll { style.scrollbar_width } else { 0.0 },
        height: if overflow.x == Overflow::Scroll { style.scrollbar_width } else { 0.0 },
    };
    let padding_border = padding + border;
    let content_box_inset = Rect {
        left: padding_border.left,
        right: padding_border.right + scrollbar_gutter.width,
        top: padding_border.top,
        bottom: padding_border.bottom + scrollbar_gutter.height,
    };
    let scrolls = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
    let style_prevents_collapse = !style.is_block()
        || scrolls
        || style.position.is_out_of_flow()
        || style.contain.establishes_independent_formatting_context()
        || padding.top > 0.0
        || padding.bottom > 0.0
        || border.top > 0.0
        || border.bottom > 0.0;
    let content_start =
        if style.direction == crate::style::Direction::Rtl { (padding.right, padding.left) } else { (padding.left, padding.right) };
    LeafBox {
        margin,
        padding,
        border,
        size,
        min_size,
        max_size,
        padding_border_sum: padding_border.sum_axes(),
        content_box_inset,
        content_start,
        aspect_ratio,
        scrolls,
        style_prevents_collapse,
    }
}

pub fn compute_leaf_layout<T: LayoutTree + ?Sized>(tree: &mut T, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
    compute_leaf_layout_with(tree, node, inputs, true, |tree, input| tree.measure(node, input))
}

/// [`compute_leaf_layout`] with an empty leaf's measurement: the known size, else nothing.
#[inline(always)]
pub fn compute_empty_leaf_layout<T: LayoutTree + ?Sized>(tree: &mut T, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
    compute_leaf_layout_with(tree, node, inputs, false, |_, input| MeasureOutput::from_size(input.known_dimensions.unwrap_or(Size::ZERO)))
}

/// What the box model reads of a leaf's resolved box, for one question.
struct Prepared {
    node_size: Size<Option<f32>>,
    node_min_size: Size<Option<f32>>,
    node_max_size: Size<Option<f32>>,
    aspect_ratio: Option<f32>,
    has_styles_preventing_being_collapsed_through: bool,
    content_box_inset_sum: Size<f32>,
    content_box_inset_top: f32,
    pb_sum: Size<f32>,
    content_start: (f32, f32),
    scrolls: bool,
    padding_top: f32,
    padding_bottom: f32,
}

/// Reads `resolved` in place, so a planned box is not copied whole.
#[inline(always)]
fn prepare(resolved: &LeafBox, known_dimensions: Size<Option<f32>>, sizing_mode: SizingMode) -> Prepared {
    let (node_size, node_min_size, node_max_size, aspect_ratio) = match sizing_mode {
        SizingMode::ContentSize => (known_dimensions, Size::NONE, Size::NONE, None),
        SizingMode::InherentSize => (known_dimensions.or(resolved.size), resolved.min_size, resolved.max_size, resolved.aspect_ratio),
    };
    let has_styles_preventing_being_collapsed_through = resolved.style_prevents_collapse
        || matches!(node_size.height, Some(h) if h > 0.0)
        || matches!(node_min_size.height, Some(h) if h > 0.0);
    Prepared {
        node_size,
        node_min_size,
        node_max_size,
        aspect_ratio,
        has_styles_preventing_being_collapsed_through,
        content_box_inset_sum: resolved.content_box_inset.sum_axes(),
        content_box_inset_top: resolved.content_box_inset.top,
        pb_sum: resolved.padding_border_sum,
        content_start: resolved.content_start,
        scrolls: resolved.scrolls,
        padding_top: resolved.padding.top,
        padding_bottom: resolved.padding.bottom,
    }
}

/// The leaf algorithm around any measurement, for trees whose leaves measure through the tree
/// itself, e.g. by laying out nested content.
///
/// `measured`: whether `measure` may read the available space, which is then recorded through
/// [`LayoutTree::note_observed`] on the axes it leaves indefinite.
#[inline(always)]
pub fn compute_leaf_layout_with<T: LayoutTree + ?Sized>(
    tree: &mut T,
    node: NodeId,
    inputs: LayoutInput,
    measured: bool,
    measure: impl FnOnce(&mut T, MeasureInput) -> MeasureOutput,
) -> LayoutOutput {
    let LayoutInput { known_dimensions, parent_size, available_space, sizing_mode, run_mode, .. } = inputs;
    let Prepared {
        node_size,
        node_min_size,
        node_max_size,
        aspect_ratio,
        has_styles_preventing_being_collapsed_through,
        content_box_inset_sum,
        content_box_inset_top,
        pb_sum,
        content_start,
        scrolls,
        padding_top,
        padding_bottom,
    } = match tree.style_plan(node) {
        Some(p) => prepare(&p.leaf, known_dimensions, sizing_mode),
        None => {
            let calc = |id, basis| tree.resolve_calc(id, basis);
            prepare(&resolve_box(tree.style(node), parent_size, &calc), known_dimensions, sizing_mode)
        }
    };

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
            oof: 0,
        };
    }

    // Content-box available space, clamped by min/max. The parent subtracted the margins.
    let available_for_measure = Size {
        width: known_dimensions
            .width
            .map(AvailableSpace::Definite)
            .unwrap_or(available_space.width)
            .maybe_set(node_size.width)
            .map_definite(|w| w.maybe_clamp(node_min_size.width, node_max_size.width) - content_box_inset_sum.width),
        height: known_dimensions
            .height
            .map(AvailableSpace::Definite)
            .unwrap_or(available_space.height)
            .maybe_set(node_size.height)
            .map_definite(|h| h.maybe_clamp(node_min_size.height, node_max_size.height) - content_box_inset_sum.height),
    };

    // The measure function sees content-box sizes.
    let measure_known = match run_mode {
        RunMode::ComputeSize => Size {
            width: known_dimensions.width.map(|w| (w - content_box_inset_sum.width).max(0.0)),
            height: known_dimensions.height.map(|h| (h - content_box_inset_sum.height).max(0.0)),
        },
        RunMode::PerformLayout => Size::NONE,
        RunMode::PerformHiddenLayout => unreachable!("hidden layout never measures"),
    };
    #[cfg(feature = "stats")]
    crate::compute::stats::bump(&crate::compute::stats::MEASURE);
    if measured {
        // The measure function may tell min-content from max-content space on an axis it is not
        // given a size for.
        let width = measure_known.width.is_none() && !available_for_measure.width.is_definite();
        let height = measure_known.height.is_none() && !available_for_measure.height.is_definite();
        if width | height {
            let height = height && !tree.measure_ignores_available_height();
            tree.note_observed((crate::compute::OBSERVED_WIDTH * width as u8) | (crate::compute::OBSERVED_HEIGHT * height as u8));
        }
    }
    let measured = measure(tree, MeasureInput { known_dimensions: measure_known, available_space: available_for_measure });
    let measured_size = measured.size;

    let clamped = known_dimensions
        .or(node_size)
        .unwrap_or(measured_size + content_box_inset_sum)
        .maybe_clamp(node_min_size, node_max_size);
    let size = Size {
        width: clamped.width,
        height: clamped.height.max(aspect_ratio.map_or(0.0, |r| clamped.width / r)),
    }
    .f32_max(pb_sum);

    let scrollable_overflow_rect = Rect {
        left: 0.0,
        top: 0.0,
        right: content_start.0 + measured_size.width + if scrolls { content_start.1 } else { 0.0 },
        bottom: padding_top + measured_size.height + if scrolls { padding_bottom } else { 0.0 },
    };

    let baselines = Baselines {
        first: measured.baselines.first.map(|b| b + content_box_inset_top),
        last: measured.baselines.last.map(|b| b + content_box_inset_top),
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
        oof: 0,
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
