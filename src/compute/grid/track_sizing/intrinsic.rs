//! §11.5 resolving intrinsic track sizes from item contributions.

use super::super::item::GridItem;
use super::super::track::GridTrack;
use super::distribute::{IntrinsicContributionType, distribute_item_space_to_base_size, distribute_item_space_to_growth_limit};
use super::{TrackSizeEstimate, cmp_by_cross_flex_then_span_then_start};
use crate::geometry::{AbstractAxis, AvailableSpace, MaybeMath, Size};
use crate::style::LengthKind;
use crate::compute::scratch::Scratch;
use crate::tree::{CacheAccess, ChildRequest, LayoutTree};

/// Groups items by span (single-track first) with flexible-track items last.
struct ItemBatcher {
    axis: AbstractAxis,
    index_offset: usize,
    current_is_flex: bool,
}

impl ItemBatcher {
    fn next(&mut self, items: &[GridItem]) -> Option<(core::ops::Range<usize>, bool)> {
        if self.current_is_flex || self.index_offset >= items.len() {
            return None;
        }
        let item = &items[self.index_offset];
        let span = item.span(self.axis);
        self.current_is_flex = item.crosses_flexible_track(self.axis);
        let next = if self.current_is_flex {
            items.len()
        } else {
            items.iter().position(|i| i.crosses_flexible_track(self.axis) || i.span(self.axis) > span).unwrap_or(items.len())
        };
        let range = self.index_offset..next;
        self.index_offset = next;
        Some((range, self.current_is_flex))
    }
}

/// Measures items with their grid-area sizes and margins folded in.
pub(super) struct Measurer<'a, T: ?Sized> {
    tree: &'a mut T,
    other_axis_tracks: &'a [GridTrack],
    estimate: TrackSizeEstimate,
    axis: AbstractAxis,
    inner_node_size: Size<Option<f32>>,
    /// While set, measurements are recorded as questions instead of asked; `NaN` markers in the
    /// item caches keep the questions to the ones the real pass will ask first.
    prefetch: Option<Scratch<ChildRequest>>,
    /// Set when the first collected question was already cached: the frame is warm and the
    /// collect pass records nothing further.
    warm: bool,
}

impl<'a, T: LayoutTree + CacheAccess + ?Sized> Measurer<'a, T> {
    pub(super) fn new(
        tree: &'a mut T,
        other_axis_tracks: &'a [GridTrack],
        estimate: TrackSizeEstimate,
        axis: AbstractAxis,
        inner_node_size: Size<Option<f32>>,
    ) -> Self {
        Self { tree, other_axis_tracks, estimate, axis, inner_node_size, prefetch: None, warm: false }
    }
    fn grid_area_size(&self, item: &mut GridItem, axis_tracks: &[GridTrack]) -> Size<Option<f32>> {
        let calc = |id, basis| self.tree.resolve_calc(id, basis);
        let estimate = self.estimate;
        item.grid_area_size_cached(self.axis, axis_tracks, self.other_axis_tracks, self.inner_node_size, |t, b| estimate(t, b, &calc), &calc)
    }
    /// Records a question in collect mode; the first one decides whether the frame is warm.
    fn collect(&mut self, request: ChildRequest) {
        if self.warm {
            return;
        }
        let Some(requests) = &mut self.prefetch else { return };
        if requests.is_empty() && crate::compute::is_cached(self.tree, &request) {
            self.warm = true;
            return;
        }
        requests.push(request);
    }

    fn min_content(&mut self, item: &mut GridItem, axis_tracks: &[GridTrack]) -> f32 {
        if self.prefetch.is_some() {
            if self.warm {
                return 0.0;
            }
            let area = self.grid_area_size(item, axis_tracks);
            let available = area.with_abstract(self.axis, None);
            if item.min_content_contribution_cache.get_abstract(self.axis).is_none() {
                item.min_content_contribution_cache.set_abstract(self.axis, Some(f32::NAN));
                let request = item.contribution_request(self.axis, self.tree, area, available, AvailableSpace::MinContent);
                self.collect(request);
            }
            return 0.0;
        }
        let area = self.grid_area_size(item, axis_tracks);
        let available = area.with_abstract(self.axis, None);
        let margins = item.margins_axis_sums_with_baseline_shims(available.width, self.tree);
        item.min_content_contribution_cached(self.axis, self.tree, area, available) + margins.get_abstract(self.axis)
    }
    pub(super) fn max_content(&mut self, item: &mut GridItem, axis_tracks: &[GridTrack]) -> f32 {
        if self.prefetch.is_some() {
            if self.warm {
                return 0.0;
            }
            let area = self.grid_area_size(item, axis_tracks);
            let available = area.with_abstract(self.axis, None);
            if item.max_content_contribution_cache.get_abstract(self.axis).is_none() {
                item.max_content_contribution_cache.set_abstract(self.axis, Some(f32::NAN));
                let request = item.contribution_request(self.axis, self.tree, area, available, AvailableSpace::MaxContent);
                self.collect(request);
            }
            return 0.0;
        }
        let area = self.grid_area_size(item, axis_tracks);
        let available = area.with_abstract(self.axis, None);
        let margins = item.margins_axis_sums_with_baseline_shims(available.width, self.tree);
        item.max_content_contribution_cached(self.axis, self.tree, area, available) + margins.get_abstract(self.axis)
    }
    fn minimum(&mut self, item: &mut GridItem, axis_tracks: &[GridTrack]) -> f32 {
        if self.prefetch.is_some() {
            if self.warm {
                return 0.0;
            }
            let area = self.grid_area_size(item, axis_tracks);
            let needs = {
                let calc = |id, basis| self.tree.resolve_calc(id, basis);
                item.min_content_contribution_cache.get_abstract(self.axis).is_none()
                    && item.minimum_contribution_cache.get_abstract(self.axis).is_none()
                    && item.minimum_needs_content(self.axis, axis_tracks, area, &calc)
            };
            if needs {
                item.min_content_contribution_cache.set_abstract(self.axis, Some(f32::NAN));
                let request = item.contribution_request(self.axis, self.tree, area, area, AvailableSpace::MinContent);
                self.collect(request);
            }
            return 0.0;
        }
        let area = self.grid_area_size(item, axis_tracks);
        let available = area.with_abstract(self.axis, None);
        let margins = item.margins_axis_sums_with_baseline_shims(available.width, self.tree);
        item.minimum_contribution_cached(self.tree, self.axis, axis_tracks, area, self.inner_node_size) + margins.get_abstract(self.axis)
    }
}

#[inline]
fn flush_planned_base_size_increases(tracks: &mut [GridTrack]) {
    for t in tracks {
        t.base_size += t.base_size_planned_increase;
        t.base_size_planned_increase = 0.0;
    }
}

#[inline]
fn flush_planned_growth_limit_increases(tracks: &mut [GridTrack], set_infinitely_growable: bool) {
    for t in tracks {
        if t.growth_limit_planned_increase > 0.0 {
            t.growth_limit = if t.growth_limit == f32::INFINITY {
                t.base_size + t.growth_limit_planned_increase
            } else {
                t.growth_limit + t.growth_limit_planned_increase
            };
            t.infinitely_growable = set_infinitely_growable;
        } else {
            t.infinitely_growable = false;
        }
        t.growth_limit_planned_increase = 0.0;
    }
}

#[inline]
fn crossed_flex_factor_sum(tracks: &[GridTrack]) -> f32 {
    tracks.iter().filter(|t| t.is_flexible()).map(|t| t.flex_factor()).sum()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_intrinsic_track_sizes<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    axis: AbstractAxis,
    axis_tracks: &mut [GridTrack],
    other_axis_tracks: &[GridTrack],
    items: &mut [GridItem],
    axis_available_grid_space: AvailableSpace,
    inner_node_size: Size<Option<f32>>,
    estimate: TrackSizeEstimate,
) {
    items.sort_by(cmp_by_cross_flex_then_span_then_start(axis));
    let axis_inner_node_size = inner_node_size.get_abstract(axis);
    let mut m = Measurer { tree, other_axis_tracks, estimate, axis, inner_node_size, prefetch: None, warm: false };
    let mut batcher = ItemBatcher { axis, index_offset: 0, current_is_flex: false };

    while let Some((range, is_flex)) = batcher.next(items) {
        let batch = &mut items[range];
        // Every item's contributions are independent: collect the questions this batch will ask
        // against a scratch copy of the tracks, answer them together, then size for real.
        // Collecting costs a second pass, so only do it when the first real item is cold: a warm
        // frame answers from the cache faster than any batch. The test reads only cache state,
        // so every executor decides alike.
        let worth_collecting = batch.len() >= 2
            && m.tree.batches_help()
            && m.tree.speculation() == 0
            && batch.iter().any(|i| {
                let meta = m.tree.query_meta(i.node);
                !meta.cheap_leaf && !meta.hidden
            });
        if worth_collecting {
            m.prefetch = Some(Scratch::take());
            m.warm = false;
            let mut copy: Scratch<GridTrack> = Scratch::collect(axis_tracks.iter().cloned());
            size_batch(&mut m, &mut copy, batch, is_flex, axis, axis_inner_node_size, axis_available_grid_space);
            for item in batch.iter_mut() {
                item.clear_requested_markers(axis);
            }
            let mut requests = m.prefetch.take().expect("collect pass set");
            // Leaves with nothing to measure are never cached, so asking early would only repeat them.
            requests.retain(|r| {
                let meta = m.tree.query_meta(r.node);
                !meta.cheap_leaf && !meta.hidden
            });
            let mut outputs: Scratch<crate::tree::LayoutOutput> = Scratch::with_capacity(requests.len());
            m.tree.compute_child_layouts(&requests, &mut outputs);
        }
        size_batch(&mut m, axis_tracks, batch, is_flex, axis, axis_inner_node_size, axis_available_grid_space);
    }

    for t in axis_tracks.iter_mut().filter(|t| t.growth_limit == f32::INFINITY) {
        t.growth_limit = t.base_size;
    }
}

/// Sizes the tracks for one batch of equal-span items.
#[allow(clippy::too_many_arguments)]
fn size_batch<T: LayoutTree + CacheAccess + ?Sized>(
    m: &mut Measurer<'_, T>,
    axis_tracks: &mut [GridTrack],
    batch: &mut [GridItem],
    is_flex: bool,
    axis: AbstractAxis,
    axis_inner_node_size: Option<f32>,
    axis_available_grid_space: AvailableSpace,
) {
    let batch_span = batch[0].span(axis);

    // Single-span items size their track directly.
    if !is_flex && batch_span == 1 {
        for item in batch.iter_mut() {
            let track_index = item.placement_indexes(axis).start as usize + 1;
            let track = &axis_tracks[track_index];
            let new_base_size = match track.min.kind() {
                LengthKind::MinContent => track.base_size.max(m.min_content(item, axis_tracks)),
                LengthKind::Percent(_) | LengthKind::Calc(_) => {
                    if axis_inner_node_size.is_none() { track.base_size.max(m.min_content(item, axis_tracks)) } else { track.base_size }
                }
                LengthKind::MaxContent => track.base_size.max(m.max_content(item, axis_tracks)),
                LengthKind::Auto => {
                    let space = match axis_available_grid_space {
                        AvailableSpace::MinContent | AvailableSpace::MaxContent
                            if !item.overflow.get(axis.as_abs()).is_scroll_container() =>
                        {
                            let minimum = m.minimum(item, axis_tracks);
                            let min_content = m.min_content(item, axis_tracks);
                            let calc = |id, basis| m.tree.resolve_calc(id, basis);
                            let limit = axis_tracks[track_index].max.definite_limit(axis_inner_node_size, &calc);
                            min_content.maybe_min(limit).max(minimum)
                        }
                        _ => m.minimum(item, axis_tracks),
                    };
                    axis_tracks[track_index].base_size.max(space)
                }
                LengthKind::Length(_) => track.base_size,
                _ => unreachable!("invalid min track sizing function"),
            };
            let growth_limit_min_content =
                if !item.overflow.get(axis.as_abs()).is_scroll_container() { Some(m.min_content(item, axis_tracks)) } else { None };
            let growth_limit_max_content = m.max_content(item, axis_tracks);
            let growth_limit_intrinsic_min_content = m.min_content(item, axis_tracks);
            let track = &mut axis_tracks[track_index];
            track.base_size = new_base_size;
            if track.max.is_fit_content() {
                if let Some(mc) = growth_limit_min_content {
                    track.growth_limit_planned_increase = track.growth_limit_planned_increase.max(mc);
                }
                let limit = track.fit_content_limit(axis_inner_node_size);
                track.growth_limit_planned_increase = track.growth_limit_planned_increase.max(growth_limit_max_content.min(limit));
            } else if track.max.is_max_content_alike() || (track.max.uses_percentage() && axis_inner_node_size.is_none()) {
                track.growth_limit_planned_increase = track.growth_limit_planned_increase.max(growth_limit_max_content);
            } else if track.max.is_intrinsic() {
                track.growth_limit_planned_increase = track.growth_limit_planned_increase.max(growth_limit_intrinsic_min_content);
            }
        }
        for t in axis_tracks.iter_mut() {
            if t.growth_limit_planned_increase > 0.0 {
                t.growth_limit = if t.growth_limit == f32::INFINITY {
                    t.growth_limit_planned_increase
                } else {
                    t.growth_limit.max(t.growth_limit_planned_increase)
                };
            }
            t.infinitely_growable = false;
            t.growth_limit_planned_increase = 0.0;
            if t.growth_limit < t.base_size {
                t.growth_limit = t.base_size;
            }
        }
        return;
    }

    // Spanning items: minimums to intrinsic-min tracks.
    for item in batch.iter_mut().filter(|i| i.crosses_intrinsic_track(axis)) {
        let space = match axis_available_grid_space {
            AvailableSpace::MinContent | AvailableSpace::MaxContent if !item.overflow.get(axis.as_abs()).is_scroll_container() => {
                let minimum = m.minimum(item, axis_tracks);
                let min_content = m.min_content(item, axis_tracks);
                let calc = |id, basis| m.tree.resolve_calc(id, basis);
                let limit = item.spanned_track_limit(axis, axis_tracks, axis_inner_node_size, &calc);
                let limited = min_content.maybe_min(limit).max(minimum);
                if is_flex {
                    let spanned = &axis_tracks[item.track_range_excluding_lines(axis)];
                    let inflexible: f32 = spanned.iter().filter(|t| !t.is_flexible()).map(|t| t.base_size).sum();
                    let scale = crossed_flex_factor_sum(spanned).min(1.0);
                    minimum.max(inflexible + (limited - inflexible).max(0.0) * scale)
                } else {
                    limited
                }
            }
            _ => m.minimum(item, axis_tracks),
        };
        let tracks = &mut axis_tracks[item.track_range_excluding_lines(axis)];
        if space > 0.0 {
            let calc = |id, basis| m.tree.resolve_calc(id, basis);
            let affected = |t: &GridTrack| t.min.definite_value(axis_inner_node_size, &calc).is_none();
            if item.overflow.get(axis.as_abs()).is_scroll_container() {
                distribute_item_space_to_base_size(
                    is_flex,
                    space,
                    tracks,
                    affected,
                    move |t| t.fit_content_limited_growth_limit(axis_inner_node_size),
                    IntrinsicContributionType::Minimum,
                    axis_inner_node_size,
                );
            } else {
                distribute_item_space_to_base_size(
                    is_flex,
                    space,
                    tracks,
                    affected,
                    |t| t.growth_limit,
                    IntrinsicContributionType::Minimum,
                    axis_inner_node_size,
                );
            }
        }
    }
    flush_planned_base_size_increases(axis_tracks);

    // Min-content contributions to min/max-content-min tracks.
    let affected = |t: &GridTrack| t.min.is_min_content() || t.min.is_max_content();
    for item in batch.iter_mut() {
        if !item.spans_track_matching(axis, axis_tracks, affected) {
            continue;
        }
        let space = m.min_content(item, axis_tracks);
        let tracks = &mut axis_tracks[item.track_range_excluding_lines(axis)];
        if space > 0.0 {
            if item.overflow.get(axis.as_abs()).is_scroll_container() {
                distribute_item_space_to_base_size(
                    is_flex,
                    space,
                    tracks,
                    affected,
                    move |t| t.fit_content_limited_growth_limit(axis_inner_node_size),
                    IntrinsicContributionType::Minimum,
                    axis_inner_node_size,
                );
            } else {
                distribute_item_space_to_base_size(
                    is_flex,
                    space,
                    tracks,
                    affected,
                    |t| t.growth_limit,
                    IntrinsicContributionType::Minimum,
                    axis_inner_node_size,
                );
            }
        }
    }
    flush_planned_base_size_increases(axis_tracks);

    // Under max-content constraints, max-content contributions to auto/max-content-min tracks.
    if axis_available_grid_space == AvailableSpace::MaxContent {
        let has_auto_min = |t: &GridTrack| t.min.is_auto() && !t.max.is_min_content();
        let has_max_content_min = |t: &GridTrack| t.min.is_max_content();
        for item in batch.iter_mut() {
            if !item.spans_track_matching(axis, axis_tracks, |t| has_auto_min(t) || has_max_content_min(t)) {
                continue;
            }
            let max_content = m.max_content(item, axis_tracks);
            let calc = |id, basis| m.tree.resolve_calc(id, basis);
            let limit = item.spanned_track_limit(axis, axis_tracks, axis_inner_node_size, &calc);
            let mut space = max_content.maybe_min(limit);
            if is_flex {
                let spanned = &axis_tracks[item.track_range_excluding_lines(axis)];
                let inflexible: f32 = spanned.iter().filter(|t| !t.is_flexible()).map(|t| t.base_size).sum();
                let scale = crossed_flex_factor_sum(spanned).min(1.0);
                space = inflexible + (space - inflexible).max(0.0) * scale;
            }
            let tracks = &mut axis_tracks[item.track_range_excluding_lines(axis)];
            if space > 0.0 {
                if tracks.iter().any(has_max_content_min) {
                    distribute_item_space_to_base_size(
                        is_flex,
                        space,
                        tracks,
                        has_max_content_min,
                        |_| f32::INFINITY,
                        IntrinsicContributionType::Maximum,
                        axis_inner_node_size,
                    );
                } else {
                    distribute_item_space_to_base_size(
                        is_flex,
                        space,
                        tracks,
                        has_auto_min,
                        move |t| t.fit_content_limited_growth_limit(axis_inner_node_size),
                        IntrinsicContributionType::Maximum,
                        axis_inner_node_size,
                    );
                }
            }
        }
        flush_planned_base_size_increases(axis_tracks);
    }

    // Max-content contributions to max-content-min tracks.
    let has_max_content_min = |t: &GridTrack| t.min.is_max_content();
    for item in batch.iter_mut() {
        if !item.spans_track_matching(axis, axis_tracks, has_max_content_min) {
            continue;
        }
        let space = m.max_content(item, axis_tracks);
        let tracks = &mut axis_tracks[item.track_range_excluding_lines(axis)];
        if space > 0.0 {
            distribute_item_space_to_base_size(
                is_flex,
                space,
                tracks,
                has_max_content_min,
                |t| t.growth_limit,
                IntrinsicContributionType::Maximum,
                axis_inner_node_size,
            );
        }
    }
    flush_planned_base_size_increases(axis_tracks);
    for t in axis_tracks.iter_mut() {
        if t.growth_limit < t.base_size {
            t.growth_limit = t.base_size;
        }
    }

    // Growth limits from contributions, for non-flex batches.
    if !is_flex {
        for item in batch.iter_mut() {
            {
                let calc = |id, basis| m.tree.resolve_calc(id, basis);
                if !item.spans_track_matching(axis, axis_tracks, |t| t.max.definite_value(axis_inner_node_size, &calc).is_none()) {
                    continue;
                }
            }
            let space = m.min_content(item, axis_tracks);
            let tracks = &mut axis_tracks[item.track_range_excluding_lines(axis)];
            if space > 0.0 {
                let calc = |id, basis| m.tree.resolve_calc(id, basis);
                distribute_item_space_to_growth_limit(
                    space,
                    tracks,
                    |t| t.max.definite_value(axis_inner_node_size, &calc).is_none(),
                    axis_inner_node_size,
                );
            }
        }
        flush_planned_growth_limit_increases(axis_tracks, true);
        let has_max_content_max =
            |t: &GridTrack| t.max.is_max_content_alike() || (t.max.uses_percentage() && axis_inner_node_size.is_none());
        for item in batch.iter_mut() {
            if !item.spans_track_matching(axis, axis_tracks, has_max_content_max) {
                continue;
            }
            let space = m.max_content(item, axis_tracks);
            let tracks = &mut axis_tracks[item.track_range_excluding_lines(axis)];
            if space > 0.0 {
                distribute_item_space_to_growth_limit(space, tracks, has_max_content_max, axis_inner_node_size);
            }
        }
        flush_planned_growth_limit_increases(axis_tracks, false);
    }
}
