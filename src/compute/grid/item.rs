//! Per-item data during grid layout, with memoised contributions.

use super::coordinates::span_of;
use super::track::GridTrack;
use crate::compute::LayoutTreeExt;
use crate::compute::common::sizing_keyword::{SizingKeywordResolution, resolve_sizing_keyword};
use crate::geometry::{AbstractAxis, AvailableSpace, Line, MaybeMath, Point, Rect, Size};
use crate::style::{
    AlignItems, AlignItemsKeyword, AlignSelf, BoxSizing, CalcId, Dimension, LengthPercentage, LengthPercentageAuto,
    OriginZeroLine, Overflow, Style,
};
use crate::tree::{CacheAccess, LayoutTree, NodeId, SizingMode};
use core::ops::Range;

/// `size`, `min-size` and `max-size` against a grid area, with aspect ratio and box sizing applied.
#[derive(Clone, Copy)]
pub(super) struct ResolvedSizes {
    pub size: Size<Option<f32>>,
    pub min_size: Size<Option<f32>>,
    pub max_size: Size<Option<f32>>,
    pub padding_border: Size<f32>,
}

/// The minimum contribution as the item's style determines it.
#[derive(Clone, Copy)]
enum StyleMinimum {
    Fixed(f32),
    MinContent,
}

pub(super) struct GridItem {
    pub node: NodeId,
    pub source_order: u16,
    pub row: Line<OriginZeroLine>,
    pub column: Line<OriginZeroLine>,
    pub is_compressible_replaced: bool,
    pub overflow: Point<Overflow>,
    pub box_sizing: BoxSizing,
    pub size: Size<Dimension>,
    pub min_size: Size<LengthPercentageAuto>,
    pub max_size: Size<LengthPercentageAuto>,
    pub aspect_ratio: Option<f32>,
    pub padding: Rect<LengthPercentage>,
    pub border: Rect<LengthPercentage>,
    pub margin: Rect<LengthPercentageAuto>,
    /// Set by [`Self::resolve_fixed_lengths`].
    fixed_lengths_resolved: bool,
    /// The sizes, when no size, padding or border depends on the grid area.
    fixed_sizes: Option<ResolvedSizes>,
    /// The margins, when none depends on the percentage basis.
    fixed_margin: Option<Rect<f32>>,
    pub align_self: AlignSelf,
    pub justify_self: AlignSelf,
    pub baseline: Option<f32>,
    pub baseline_shim: f32,
    /// Indices into the interleaved gutter/track lists.
    pub row_indexes: Line<u16>,
    pub column_indexes: Line<u16>,
    pub crosses_flexible_row: bool,
    pub crosses_flexible_column: bool,
    pub crosses_intrinsic_row: bool,
    pub crosses_intrinsic_column: bool,
    pub grid_area_size_cache: Option<Size<Option<f32>>>,
    pub min_content_contribution_cache: Size<Option<f32>>,
    pub minimum_contribution_cache: Size<Option<f32>>,
    pub max_content_contribution_cache: Size<Option<f32>>,
    pub y_position: f32,
    pub height: f32,
    /// The last baseline from the final layout, relative to the item's border box.
    pub last_baseline: f32,
}

impl GridItem {
    pub fn new(
        node: NodeId,
        col_span: Line<OriginZeroLine>,
        row_span: Line<OriginZeroLine>,
        style: &Style,
        parent_align_items: AlignItems,
        parent_justify_items: AlignItems,
        source_order: u16,
    ) -> Self {
        Self {
            node,
            source_order,
            row: row_span,
            column: col_span,
            is_compressible_replaced: style.item_is_replaced,
            overflow: style.overflow,
            box_sizing: style.box_sizing,
            size: style.size,
            min_size: style.min_size,
            max_size: style.max_size,
            aspect_ratio: style.aspect_ratio,
            padding: style.padding,
            border: style.border,
            margin: style.margin,
            fixed_lengths_resolved: false,
            fixed_sizes: None,
            fixed_margin: None,
            align_self: style.align_self.unwrap_or(parent_align_items),
            justify_self: style.justify_self.unwrap_or(parent_justify_items),
            baseline: None,
            baseline_shim: 0.0,
            row_indexes: Line { start: 0, end: 0 },
            column_indexes: Line { start: 0, end: 0 },
            crosses_flexible_row: false,
            crosses_flexible_column: false,
            crosses_intrinsic_row: false,
            crosses_intrinsic_column: false,
            grid_area_size_cache: None,
            min_content_contribution_cache: Size::NONE,
            max_content_contribution_cache: Size::NONE,
            minimum_contribution_cache: Size::NONE,
            y_position: 0.0,
            last_baseline: 0.0,
            height: 0.0,
        }
    }

    /// Resolves, once, the lengths free of percentages and `calc()`: those ignore the basis, so
    /// the many measurements of track sizing can share them.
    pub fn resolve_fixed_lengths(&mut self) {
        if self.fixed_lengths_resolved {
            return;
        }
        self.fixed_lengths_resolved = true;
        let no_basis = |_: CalcId, _: f32| 0.0;
        let (size, min, max, padding, border) = (self.size, self.min_size, self.max_size, self.padding, self.border);
        let sizes = [
            size.width.raw(),
            size.height.raw(),
            min.width.raw(),
            min.height.raw(),
            max.width.raw(),
            max.height.raw(),
            padding.left.raw(),
            padding.right.raw(),
            padding.top.raw(),
            padding.bottom.raw(),
            border.left.raw(),
            border.right.raw(),
            border.top.raw(),
            border.bottom.raw(),
        ];
        if !sizes.iter().any(|l| l.uses_percentage()) {
            self.fixed_sizes = Some(self.resolve_sizes(Size::NONE, &no_basis));
        }
        let margin = self.margin;
        if ![margin.left, margin.right, margin.top, margin.bottom].iter().any(|m| m.uses_percentage()) {
            self.fixed_margin = Some(self.resolve_margin(None, &no_basis));
        }
    }

    fn resolve_sizes(&self, grid_area_size: Size<Option<f32>>, calc: &impl Fn(CalcId, f32) -> f32) -> ResolvedSizes {
        let padding = self.padding.map(|p| p.resolve_or_zero(grid_area_size.width, calc));
        let border = self.border.map(|b| b.resolve_or_zero(grid_area_size.width, calc));
        let padding_border_size = (padding + border).sum_axes();
        let box_sizing_adjustment = if self.box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };
        let resolve = |s: Size<crate::style::Length>| {
            crate::compute::block::resolve_size(s, grid_area_size, calc)
                .maybe_apply_aspect_ratio(self.aspect_ratio)
                .maybe_add(box_sizing_adjustment)
        };
        ResolvedSizes {
            size: resolve(self.size.map(|v| v.raw())),
            min_size: resolve(self.min_size.map(|v| v.raw())),
            max_size: resolve(self.max_size.map(|v| v.raw())),
            padding_border: padding_border_size,
        }
    }

    #[inline]
    fn sizes(&self, grid_area_size: Size<Option<f32>>, calc: &impl Fn(CalcId, f32) -> f32) -> ResolvedSizes {
        match self.fixed_sizes {
            Some(sizes) => sizes,
            None => self.resolve_sizes(grid_area_size, calc),
        }
    }

    /// Inline margins resolve against 0, block margins against the inner width.
    fn resolve_margin(&self, inner_node_width: Option<f32>, calc: &impl Fn(CalcId, f32) -> f32) -> Rect<f32> {
        Rect {
            left: self.margin.left.resolve_or_zero(Some(0.0), calc),
            right: self.margin.right.resolve_or_zero(Some(0.0), calc),
            top: self.margin.top.resolve_or_zero(inner_node_width, calc),
            bottom: self.margin.bottom.resolve_or_zero(inner_node_width, calc),
        }
    }

    #[inline]
    pub fn has_auto_block_margin(&self) -> bool {
        self.margin.top.is_auto() || self.margin.bottom.is_auto()
    }
    #[inline]
    pub fn has_cyclic_block_size_dependency(&self) -> bool {
        self.size.height.uses_percentage() && (self.crosses_intrinsic_row || self.crosses_flexible_row)
    }
    #[inline]
    pub fn participates_in_baseline_alignment(&self) -> bool {
        self.align_self.keyword == AlignItemsKeyword::Baseline
            && !self.has_auto_block_margin()
            && !self.has_cyclic_block_size_dependency()
    }
    #[inline]
    pub fn placement(&self, axis: AbstractAxis) -> Line<OriginZeroLine> {
        match axis {
            AbstractAxis::Block => self.row,
            AbstractAxis::Inline => self.column,
        }
    }
    #[inline]
    pub fn placement_indexes(&self, axis: AbstractAxis) -> Line<u16> {
        match axis {
            AbstractAxis::Block => self.row_indexes,
            AbstractAxis::Inline => self.column_indexes,
        }
    }
    #[inline]
    pub fn track_range_excluding_lines(&self, axis: AbstractAxis) -> Range<usize> {
        let i = self.placement_indexes(axis);
        (i.start as usize + 1)..(i.end as usize)
    }
    #[inline]
    pub fn span(&self, axis: AbstractAxis) -> u16 {
        span_of(self.placement(axis))
    }
    #[inline]
    pub fn crosses_flexible_track(&self, axis: AbstractAxis) -> bool {
        match axis {
            AbstractAxis::Inline => self.crosses_flexible_column,
            AbstractAxis::Block => self.crosses_flexible_row,
        }
    }
    #[inline]
    pub fn crosses_intrinsic_track(&self, axis: AbstractAxis) -> bool {
        match axis {
            AbstractAxis::Inline => self.crosses_intrinsic_column,
            AbstractAxis::Block => self.crosses_intrinsic_row,
        }
    }

    #[inline]
    pub fn spans_track_matching(&self, axis: AbstractAxis, axis_tracks: &[GridTrack], predicate: impl Fn(&GridTrack) -> bool) -> bool {
        axis_tracks[self.track_range_excluding_lines(axis)].iter().any(predicate)
    }

    /// Sum of spanned tracks' definite limits (`fit-content()` counts), if all are definite.
    pub fn spanned_track_limit(
        &self,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        axis_parent_size: Option<f32>,
        calc: &impl Fn(CalcId, f32) -> f32,
    ) -> Option<f32> {
        axis_tracks[self.track_range_excluding_lines(axis)].iter().map(|t| t.max.definite_limit(axis_parent_size, calc)).sum()
    }

    /// Sum of spanned tracks' definite max sizes, if all are definite.
    pub fn spanned_fixed_track_limit(
        &self,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        axis_parent_size: Option<f32>,
        calc: &impl Fn(CalcId, f32) -> f32,
    ) -> Option<f32> {
        axis_tracks[self.track_range_excluding_lines(axis)].iter().map(|t| t.max.definite_value(axis_parent_size, calc)).sum()
    }

    fn known_dimensions<T: LayoutTree + ?Sized>(&self, tree: &T, grid_area_size: Size<Option<f32>>) -> Size<Option<f32>> {
        let calc = |id, basis| tree.resolve_calc(id, basis);
        let margins = self.margins_axis_sums_with_baseline_shims(grid_area_size.width, tree);
        let aspect_ratio = self.aspect_ratio;
        let ResolvedSizes { size: inherent_size, min_size, max_size, .. } = self.sizes(grid_area_size, &calc);
        let grid_area_minus_margins = grid_area_size.maybe_sub(margins).maybe_max(Size::ZERO);
        let width = inherent_size.width.or_else(|| {
            if self.size.width.is_sizing_keyword() {
                return match resolve_sizing_keyword(self.size.width.raw(), grid_area_minus_margins.width, grid_area_size.width) {
                    Some(SizingKeywordResolution::Exact(w)) => Some(w),
                    _ => None,
                };
            }
            if !self.margin.left.is_auto() && !self.margin.right.is_auto() && self.justify_self == AlignSelf::STRETCH {
                return grid_area_minus_margins.width;
            }
            None
        });
        let Size { width, height } = Size { width, height: inherent_size.height }.maybe_apply_aspect_ratio(aspect_ratio);
        let height = height.or_else(|| {
            if self.size.height.is_sizing_keyword() {
                return match resolve_sizing_keyword(self.size.height.raw(), grid_area_minus_margins.height, grid_area_size.height) {
                    Some(SizingKeywordResolution::Exact(h)) => Some(h),
                    _ => None,
                };
            }
            if !self.margin.top.is_auto() && !self.margin.bottom.is_auto() && self.align_self == AlignSelf::STRETCH {
                return grid_area_minus_margins.height;
            }
            None
        });
        Size { width, height }.maybe_apply_aspect_ratio(aspect_ratio).maybe_clamp(min_size, max_size)
    }

    /// The item's grid area: definite fixed tracks in `axis`, estimated tracks in the other.
    pub fn grid_area_size(
        &self,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        other_axis_tracks: &[GridTrack],
        available_space: Size<Option<f32>>,
        get_track_size_estimate: impl Fn(&GridTrack, Option<f32>) -> Option<f32>,
        calc: &impl Fn(CalcId, f32) -> f32,
    ) -> Size<Option<f32>> {
        let mut size = Size::NONE;
        size.set_abstract(
            axis,
            axis_tracks[self.track_range_excluding_lines(axis)]
                .iter()
                .map(|track| {
                    let min = track.min.definite_value(available_space.get_abstract(axis), calc)?;
                    let max = track.max.definite_value(available_space.get_abstract(axis), calc)?;
                    if min == max { Some(track.base_size) } else { None }
                })
                .sum::<Option<f32>>(),
        );
        size.set_abstract(
            axis.other(),
            other_axis_tracks[self.track_range_excluding_lines(axis.other())]
                .iter()
                .map(|track| {
                    get_track_size_estimate(track, available_space.get_abstract(axis.other()))
                        .map(|s| s + track.content_alignment_adjustment)
                })
                .sum::<Option<f32>>(),
        );
        size
    }

    pub fn grid_area_size_cached(
        &mut self,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        other_axis_tracks: &[GridTrack],
        available_space: Size<Option<f32>>,
        get_track_size_estimate: impl Fn(&GridTrack, Option<f32>) -> Option<f32>,
        calc: &impl Fn(CalcId, f32) -> f32,
    ) -> Size<Option<f32>> {
        match self.grid_area_size_cache {
            Some(s) => s,
            None => {
                let s = self.grid_area_size(axis, axis_tracks, other_axis_tracks, available_space, get_track_size_estimate, calc);
                self.grid_area_size_cache = Some(s);
                s
            }
        }
    }

    /// Margin sums; inline margins resolve against 0, block margins against the inner width.
    #[inline]
    pub fn margins_axis_sums_with_baseline_shims<T: LayoutTree + ?Sized>(&self, inner_node_width: Option<f32>, tree: &T) -> Size<f32> {
        let margin = match self.fixed_margin {
            Some(margin) => margin,
            None => self.resolve_margin(inner_node_width, &|id, basis| tree.resolve_calc(id, basis)),
        };
        Rect { top: margin.top + self.baseline_shim, ..margin }.sum_axes()
    }

    fn keyword_adjusted_available_space<T: LayoutTree + ?Sized>(
        &self,
        grid_area_size: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        tree: &T,
    ) -> Size<AvailableSpace> {
        if !self.size.width.is_sizing_keyword() && !self.size.height.is_sizing_keyword() {
            return available_space;
        }
        let margins = self.margins_axis_sums_with_baseline_shims(grid_area_size.width, tree);
        let mut adjusted = available_space;
        for axis in [AbstractAxis::Inline, AbstractAxis::Block] {
            let size_style = self.size.get_abstract(axis);
            if !size_style.is_sizing_keyword() {
                continue;
            }
            let stretch = grid_area_size.get_abstract(axis).maybe_sub(margins.get_abstract(axis)).maybe_max(0.0);
            if let Some(SizingKeywordResolution::Measure(a)) =
                resolve_sizing_keyword(size_style.raw(), stretch, grid_area_size.get_abstract(axis))
            {
                adjusted.set_abstract(axis, a);
            }
        }
        adjusted
    }

    /// The question [`Self::content_contribution`] would ask, for batching ahead of time.
    pub fn contribution_request<T: LayoutTree + ?Sized>(
        &self,
        axis: AbstractAxis,
        tree: &T,
        grid_area_size: Size<Option<f32>>,
        available_space: Size<Option<f32>>,
        fallback: AvailableSpace,
    ) -> crate::tree::ChildRequest {
        let known_dimensions = self.known_dimensions(tree, grid_area_size);
        let available = self.keyword_adjusted_available_space(
            grid_area_size,
            available_space.map(|o| o.map_or(fallback, AvailableSpace::Definite)),
            tree,
        );
        crate::compute::size_request(self.node, known_dimensions, grid_area_size, available, SizingMode::InherentSize, axis.as_abs().into())
    }

    /// What the style makes of the minimum contribution (css-grid-1 §11.5, css-sizing-3 §5.2):
    /// a definite preferred size clamped by the min and max sizes; the min-content contribution
    /// for a sizing keyword other than `stretch`; else the min size, cyclic percentages against
    /// zero; else zero for a scroll container; else (`None`) the automatic minimum.
    fn style_minimum(&self, axis: AbstractAxis, grid_area_size: Size<Option<f32>>, calc: &impl Fn(CalcId, f32) -> f32) -> Option<StyleMinimum> {
        let sizes = self.sizes(grid_area_size, calc);
        if let Some(size) = sizes.size.get_abstract(axis) {
            let (min, max) = (self.min_size.get_abstract(axis), self.max_size.get_abstract(axis));
            if min.is_auto() && max.is_auto() {
                return Some(StyleMinimum::Fixed(size));
            }
            let basis = grid_area_size.get_abstract(axis);
            // A compressible replaced element resolves an indefinite percentage max size against zero.
            let max_basis = if self.is_compressible_replaced { basis.or(Some(0.0)) } else { basis };
            let adjustment = if self.box_sizing == BoxSizing::ContentBox { sizes.padding_border.get_abstract(axis) } else { 0.0 };
            let min = min.resolve(basis, calc).map(|v| v + adjustment);
            let max = max.resolve(max_basis, calc).map(|v| v + adjustment);
            return Some(StyleMinimum::Fixed(size.maybe_clamp(min, max)));
        }
        let size = self.size.get_abstract(axis);
        if size.is_sizing_keyword() && !size.is_stretch() {
            return Some(StyleMinimum::MinContent);
        }
        let min_size = if self.fixed_sizes.is_some() || grid_area_size.width.is_some() && grid_area_size.height.is_some() {
            sizes.min_size.get_abstract(axis)
        } else {
            let adjustment = if self.box_sizing == BoxSizing::ContentBox { sizes.padding_border } else { Size::ZERO };
            crate::compute::block::resolve_size(self.min_size.map(|v| v.raw()), grid_area_size.map(|s| Some(s.unwrap_or(0.0))), calc)
                .maybe_apply_aspect_ratio(self.aspect_ratio)
                .maybe_add(adjustment)
                .get_abstract(axis)
        };
        min_size.or_else(|| self.overflow.get(axis.as_abs()).maybe_into_automatic_min_size()).map(StyleMinimum::Fixed)
    }

    /// Whether [`Self::minimum_contribution`] would measure min-content rather than read the style.
    pub fn minimum_needs_content(
        &self,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        grid_area_size: Size<Option<f32>>,
        inner_node_size: Size<Option<f32>>,
        calc: &impl Fn(crate::style::CalcId, f32) -> f32,
    ) -> bool {
        match self.style_minimum(axis, grid_area_size, calc) {
            Some(StyleMinimum::Fixed(_)) => false,
            Some(StyleMinimum::MinContent) => true,
            None => self.uses_content_based_minimum(axis, axis_tracks, inner_node_size.get_abstract(axis)),
        }
    }

    /// css-grid-1 §6.6: the automatic minimum is content-based when the item spans a track whose
    /// min sizing function behaves as `auto` and either spans one track or crosses no flexible one.
    #[inline]
    fn uses_content_based_minimum(&self, axis: AbstractAxis, axis_tracks: &[GridTrack], axis_inner_node_size: Option<f32>) -> bool {
        (self.span(axis) == 1 || !self.crosses_flexible_track(axis))
            && self.spans_track_matching(axis, axis_tracks, |t| t.min_behaves_as_auto(axis_inner_node_size))
    }

    /// Drops the "requested" markers a collect pass left in the contribution caches.
    pub fn clear_requested_markers(&mut self, axis: AbstractAxis) {
        if self.min_content_contribution_cache.get_abstract(axis).is_some_and(f32::is_nan) {
            self.min_content_contribution_cache.set_abstract(axis, None);
        }
        if self.max_content_contribution_cache.get_abstract(axis).is_some_and(f32::is_nan) {
            self.max_content_contribution_cache.set_abstract(axis, None);
        }
    }

    fn content_contribution<T: LayoutTree + CacheAccess + ?Sized>(
        &self,
        axis: AbstractAxis,
        tree: &mut T,
        grid_area_size: Size<Option<f32>>,
        available_space: Size<Option<f32>>,
        fallback: AvailableSpace,
    ) -> f32 {
        let known_dimensions = self.known_dimensions(tree, grid_area_size);
        let available = self.keyword_adjusted_available_space(
            grid_area_size,
            available_space.map(|o| o.map_or(fallback, AvailableSpace::Definite)),
            tree,
        );
        tree.measure_child_size(
            self.node,
            known_dimensions,
            grid_area_size,
            available,
            SizingMode::InherentSize,
            axis.as_abs(),
            Line::FALSE,
        )
    }

    pub fn min_content_contribution<T: LayoutTree + CacheAccess + ?Sized>(
        &self,
        axis: AbstractAxis,
        tree: &mut T,
        grid_area_size: Size<Option<f32>>,
        available_space: Size<Option<f32>>,
    ) -> f32 {
        self.content_contribution(axis, tree, grid_area_size, available_space, AvailableSpace::MinContent)
    }

    pub fn min_content_contribution_cached<T: LayoutTree + CacheAccess + ?Sized>(
        &mut self,
        axis: AbstractAxis,
        tree: &mut T,
        grid_area_size: Size<Option<f32>>,
        available_space: Size<Option<f32>>,
    ) -> f32 {
        match self.min_content_contribution_cache.get_abstract(axis) {
            Some(v) => v,
            None => {
                let v = self.min_content_contribution(axis, tree, grid_area_size, available_space);
                self.min_content_contribution_cache.set_abstract(axis, Some(v));
                v
            }
        }
    }

    pub fn max_content_contribution<T: LayoutTree + CacheAccess + ?Sized>(
        &self,
        axis: AbstractAxis,
        tree: &mut T,
        grid_area_size: Size<Option<f32>>,
        available_space: Size<Option<f32>>,
    ) -> f32 {
        self.content_contribution(axis, tree, grid_area_size, available_space, AvailableSpace::MaxContent)
    }

    pub fn max_content_contribution_cached<T: LayoutTree + CacheAccess + ?Sized>(
        &mut self,
        axis: AbstractAxis,
        tree: &mut T,
        grid_area_size: Size<Option<f32>>,
        available_space: Size<Option<f32>>,
    ) -> f32 {
        match self.max_content_contribution_cache.get_abstract(axis) {
            Some(v) => v,
            None => {
                let v = self.max_content_contribution(axis, tree, grid_area_size, available_space);
                self.max_content_contribution_cache.set_abstract(axis, Some(v));
                v
            }
        }
    }

    /// css-grid-1 §6.6 automatic minimum size.
    pub fn minimum_contribution<T: LayoutTree + CacheAccess + ?Sized>(
        &mut self,
        tree: &mut T,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        grid_area_size: Size<Option<f32>>,
        inner_node_size: Size<Option<f32>>,
    ) -> f32 {
        let calc = |id, basis| tree.resolve_calc(id, basis);
        match self.style_minimum(axis, grid_area_size, &calc) {
            Some(StyleMinimum::Fixed(v)) => return v,
            Some(StyleMinimum::MinContent) => return self.min_content_contribution_cached(axis, tree, grid_area_size, grid_area_size),
            None => {}
        }
        if !self.uses_content_based_minimum(axis, axis_tracks, inner_node_size.get_abstract(axis)) {
            return 0.0;
        }
        let mut minimum = self.min_content_contribution_cached(axis, tree, grid_area_size, grid_area_size);
        let calc = |id, basis| tree.resolve_calc(id, basis);
        if self.is_compressible_replaced {
            let size = self.size.get_abstract(axis).resolve(Some(0.0), &calc);
            let max_size = self.max_size.get_abstract(axis).resolve(Some(0.0), &calc);
            minimum = minimum.maybe_min(size).maybe_min(max_size);
        }
        // The clamp by fixed max track sizing functions is a stretch fit into them: it never
        // shrinks the item below its padding and border.
        let limit = self.spanned_fixed_track_limit(axis, axis_tracks, inner_node_size.get_abstract(axis), &calc);
        let padding_border = self.sizes(grid_area_size, &calc).padding_border.get_abstract(axis);
        minimum.maybe_min(limit.map(|l| l.max(padding_border)))
    }

    pub fn minimum_contribution_cached<T: LayoutTree + CacheAccess + ?Sized>(
        &mut self,
        tree: &mut T,
        axis: AbstractAxis,
        axis_tracks: &[GridTrack],
        grid_area_size: Size<Option<f32>>,
        inner_node_size: Size<Option<f32>>,
    ) -> f32 {
        match self.minimum_contribution_cache.get_abstract(axis) {
            Some(v) => v,
            None => {
                let v = self.minimum_contribution(tree, axis, axis_tracks, grid_area_size, inner_node_size);
                self.minimum_contribution_cache.set_abstract(axis, Some(v));
                v
            }
        }
    }
}
