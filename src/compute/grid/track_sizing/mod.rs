//! css-grid-1 §11 track sizing.

mod distribute;
mod intrinsic;

use super::coordinates::TrackCounts;
use super::item::GridItem;
use super::track::GridTrack;
use crate::compute::LayoutTreeExt;
use crate::geometry::{AbstractAxis, AvailableSpace, Line, Size};
use crate::style::{AlignContent, AlignContentKeyword, CalcId};
use crate::tree::{CacheAccess, LayoutTree, SizingMode};
use core::cmp::Ordering;
use distribute::distribute_space_up_to_limits;

/// Size estimate for a track in the other axis during this axis' sizing.
pub(super) type TrackSizeEstimate = fn(&GridTrack, Option<f32>, &dyn Fn(CalcId, f32) -> f32) -> Option<f32>;

pub(super) fn estimate_from_max_sizing_function(track: &GridTrack, basis: Option<f32>, calc: &dyn Fn(CalcId, f32) -> f32) -> Option<f32> {
    track.max.definite_value(basis, &calc)
}

pub(super) fn estimate_from_base_size(track: &GridTrack, _: Option<f32>, _: &dyn Fn(CalcId, f32) -> f32) -> Option<f32> {
    Some(track.base_size)
}

/// Items without flexible tracks first, then by span, then by start line.
pub(super) fn cmp_by_cross_flex_then_span_then_start(axis: AbstractAxis) -> impl FnMut(&GridItem, &GridItem) -> Ordering {
    move |a, b| match (a.crosses_flexible_track(axis), b.crosses_flexible_track(axis)) {
        (false, true) => Ordering::Less,
        (true, false) => Ordering::Greater,
        _ => {
            let (pa, pb) = (a.placement(axis), b.placement(axis));
            match super::coordinates::span_of(pa).cmp(&super::coordinates::span_of(pb)) {
                Ordering::Equal => pa.start.cmp(&pb.start),
                o => o,
            }
        }
    }
}

/// Extra gutter space from distributing `align-content` in the other axis.
pub(super) fn compute_alignment_gutter_adjustment(
    alignment: AlignContent,
    axis_inner_node_size: Option<f32>,
    get_track_size_estimate: impl Fn(&GridTrack, Option<f32>) -> Option<f32>,
    tracks: &[GridTrack],
) -> f32 {
    if tracks.len() <= 1 {
        return 0.0;
    }
    let outer_gutter_weight = match alignment.keyword {
        AlignContentKeyword::Start
        | AlignContentKeyword::FlexStart
        | AlignContentKeyword::End
        | AlignContentKeyword::FlexEnd
        | AlignContentKeyword::Center
        | AlignContentKeyword::SpaceAround
        | AlignContentKeyword::SpaceEvenly => 1,
        AlignContentKeyword::Stretch | AlignContentKeyword::SpaceBetween => 0,
    };
    let inner_gutter_weight = match alignment.keyword {
        AlignContentKeyword::SpaceBetween | AlignContentKeyword::SpaceEvenly => 1,
        AlignContentKeyword::SpaceAround => 2,
        _ => 0,
    };
    if inner_gutter_weight == 0 {
        return 0.0;
    }
    let Some(inner) = axis_inner_node_size else { return 0.0 };
    let free_space = tracks
        .iter()
        .map(|t| get_track_size_estimate(t, Some(inner)))
        .sum::<Option<f32>>()
        .map(|sum| (inner - sum).max(0.0))
        .unwrap_or(0.0);
    let weighted = ((tracks.len() - 3) / 2) * inner_gutter_weight + 2 * outer_gutter_weight;
    (free_space / weighted as f32) * inner_gutter_weight as f32
}

pub(super) fn resolve_item_track_indexes(items: &mut [GridItem], column_counts: TrackCounts, row_counts: TrackCounts) {
    for item in items {
        item.column_indexes = item.column.map(|l| column_counts.track_vec_index(l) as u16);
        item.row_indexes = item.row.map(|l| row_counts.track_vec_index(l) as u16);
    }
}

pub(super) fn determine_if_item_crosses_flexible_or_intrinsic_tracks(items: &mut [GridItem], columns: &[GridTrack], rows: &[GridTrack]) {
    for item in items {
        item.crosses_flexible_column = item.track_range_excluding_lines(AbstractAxis::Inline).any(|i| columns[i].is_flexible());
        item.crosses_intrinsic_column =
            item.track_range_excluding_lines(AbstractAxis::Inline).any(|i| columns[i].has_intrinsic_sizing_function());
        item.crosses_flexible_row = item.track_range_excluding_lines(AbstractAxis::Block).any(|i| rows[i].is_flexible());
        item.crosses_intrinsic_row = item.track_range_excluding_lines(AbstractAxis::Block).any(|i| rows[i].has_intrinsic_sizing_function());
    }
}

pub(super) struct TrackSizingParams<'a> {
    pub axis: AbstractAxis,
    pub axis_min_size: Option<f32>,
    pub axis_max_size: Option<f32>,
    pub axis_alignment: AlignContent,
    pub other_axis_alignment: AlignContent,
    pub available_grid_space: Size<AvailableSpace>,
    pub inner_node_size: Size<Option<f32>>,
    pub axis_tracks: &'a mut [GridTrack],
    pub other_axis_tracks: &'a mut [GridTrack],
    pub items: &'a mut [GridItem],
    pub get_track_size_estimate: TrackSizeEstimate,
    pub has_baseline_aligned_item: bool,
    pub run_mode: crate::tree::RunMode,
}

pub(super) fn track_sizing_algorithm<T: LayoutTree + CacheAccess + ?Sized>(tree: &mut T, p: TrackSizingParams<'_>) {
    let TrackSizingParams {
        axis,
        axis_min_size,
        axis_max_size,
        axis_alignment,
        other_axis_alignment,
        available_grid_space,
        inner_node_size,
        axis_tracks,
        other_axis_tracks,
        items,
        get_track_size_estimate,
        has_baseline_aligned_item,
        run_mode,
    } = p;
    let percentage_basis = inner_node_size.get_abstract(axis).or(axis_min_size);
    initialize_track_sizes(tree, axis_tracks, percentage_basis);
    if has_baseline_aligned_item {
        if run_mode == crate::tree::RunMode::ComputeSize {
            crate::compute::speculate(tree, |tree| resolve_item_baselines(tree, axis, items, inner_node_size));
        } else {
            resolve_item_baselines(tree, axis, items, inner_node_size);
        }
    }
    // Every track fixed: nothing to size.
    {
        let calc = |id, basis| tree.resolve_calc(id, basis);
        if axis_tracks.iter().all(|t| t.base_size == t.growth_limit && t.min.definite_value(percentage_basis, &calc).is_some()) {
            return;
        }
    }
    let gutter_adjustment = {
        let calc = |id, basis| tree.resolve_calc(id, basis);
        compute_alignment_gutter_adjustment(
            other_axis_alignment,
            inner_node_size.get_abstract(axis.other()),
            |t, basis| get_track_size_estimate(t, basis, &calc),
            other_axis_tracks,
        )
    };
    if other_axis_tracks.len() > 3 {
        let len = other_axis_tracks.len();
        for track in other_axis_tracks[2..len].iter_mut().step_by(2) {
            track.content_alignment_adjustment = gutter_adjustment;
        }
    }
    intrinsic::resolve_intrinsic_track_sizes(
        tree,
        axis,
        axis_tracks,
        other_axis_tracks,
        items,
        available_grid_space.get_abstract(axis),
        inner_node_size,
        get_track_size_estimate,
    );
    maximise_tracks(axis_tracks, inner_node_size.get_abstract(axis), available_grid_space.get_abstract(axis));
    let available_for_expansion = match inner_node_size.get_abstract(axis) {
        Some(s) => AvailableSpace::Definite(s),
        None => match available_grid_space.get_abstract(axis) {
            AvailableSpace::MinContent => AvailableSpace::MinContent,
            _ => AvailableSpace::MaxContent,
        },
    };
    expand_flexible_tracks(
        tree,
        axis,
        axis_tracks,
        other_axis_tracks,
        items,
        axis_min_size,
        axis_max_size,
        available_for_expansion,
        inner_node_size,
        get_track_size_estimate,
    );
    if axis_alignment == AlignContent::STRETCH {
        stretch_auto_tracks(axis_tracks, axis_min_size, available_for_expansion);
    }
}

fn initialize_track_sizes<T: LayoutTree + ?Sized>(tree: &T, axis_tracks: &mut [GridTrack], basis: Option<f32>) {
    let calc = |id, basis| tree.resolve_calc(id, basis);
    for track in axis_tracks.iter_mut() {
        track.base_size = track.min.definite_value(basis, &calc).unwrap_or(0.0);
        track.growth_limit = track.max.definite_value(basis, &calc).unwrap_or(f32::INFINITY);
        if track.growth_limit < track.base_size {
            track.growth_limit = track.base_size;
        }
    }
}

/// Baseline shims for rows with more than one baseline-aligned item.
fn resolve_item_baselines<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    axis: AbstractAxis,
    items: &mut [GridItem],
    inner_node_size: Size<Option<f32>>,
) {
    let other_axis = axis.other();
    items.sort_by_key(|item| item.placement(other_axis).start);
    let mut rest = &mut items[..];
    while !rest.is_empty() {
        let current_row = rest[0].placement(other_axis).start;
        let split = rest.iter().position(|i| i.placement(other_axis).start != current_row).unwrap_or(rest.len());
        let (row_items, tail) = rest.split_at_mut(split);
        rest = tail;
        if row_items.iter().filter(|i| i.participates_in_baseline_alignment()).count() <= 1 {
            continue;
        }
        for item in row_items.iter_mut() {
            if !item.participates_in_baseline_alignment() {
                continue;
            }
            let out = tree.perform_child_layout(item.node, Size::NONE, inner_node_size, Size::MIN_CONTENT, SizingMode::InherentSize, Line::FALSE);
            let height = out.size.height;
            let baseline = if item.overflow.y.is_scroll_container() {
                out.baselines.first.unwrap_or(height).min(height).max(0.0)
            } else {
                out.baselines.first.unwrap_or(height)
            };
            let calc = |id, basis| tree.resolve_calc(id, basis);
            item.baseline = Some(baseline + item.margin.top.resolve_or_zero(inner_node_size.width, &calc));
        }
        let row_max = row_items
            .iter()
            .filter(|i| i.participates_in_baseline_alignment())
            .map(|i| i.baseline.unwrap_or(0.0))
            .fold(f32::NEG_INFINITY, f32::max);
        for item in row_items.iter_mut().filter(|i| i.participates_in_baseline_alignment()) {
            item.baseline_shim = row_max - item.baseline.unwrap_or(0.0);
        }
    }
}

/// §11.6: grow tracks up to their growth limits with the free space.
fn maximise_tracks(axis_tracks: &mut [GridTrack], axis_inner_node_size: Option<f32>, axis_available_grid_space: AvailableSpace) {
    let used_space: f32 = axis_tracks.iter().map(|t| t.base_size).sum();
    let free_space = axis_available_grid_space.compute_free_space(used_space);
    if free_space == f32::INFINITY {
        for t in axis_tracks.iter_mut() {
            t.base_size = t.growth_limit;
        }
    } else if free_space > 0.0 {
        distribute_space_up_to_limits(free_space, axis_tracks, |_| true, |_| 1.0, |t| t.base_size, move |t| {
            t.fit_content_limited_growth_limit(axis_inner_node_size)
        });
        for t in axis_tracks.iter_mut() {
            t.base_size += t.item_incurred_increase;
            t.item_incurred_increase = 0.0;
        }
    }
}

/// §11.7: size flexible tracks.
#[allow(clippy::too_many_arguments)]
fn expand_flexible_tracks<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    axis: AbstractAxis,
    axis_tracks: &mut [GridTrack],
    other_axis_tracks: &[GridTrack],
    items: &mut [GridItem],
    axis_min_size: Option<f32>,
    axis_max_size: Option<f32>,
    available: AvailableSpace,
    inner_node_size: Size<Option<f32>>,
    estimate: TrackSizeEstimate,
) {
    let flex_fraction = match available {
        AvailableSpace::Definite(space) => {
            let used: f32 = axis_tracks.iter().map(|t| t.base_size).sum();
            if space - used <= 0.0 { 0.0 } else { find_size_of_fr(axis_tracks, space) }
        }
        AvailableSpace::MinContent => 0.0,
        AvailableSpace::MaxContent => {
            let from_tracks = axis_tracks
                .iter()
                .filter(|t| t.max.is_fr())
                .map(|t| {
                    let f = t.flex_factor();
                    if f > 1.0 { t.base_size / f } else { t.base_size }
                })
                .fold(0.0f32, f32::max);
            let mut m = intrinsic::Measurer::new(tree, other_axis_tracks, estimate, axis, inner_node_size);
            let from_items = items
                .iter_mut()
                .filter(|i| i.crosses_flexible_track(axis))
                .map(|i| {
                    let range = i.track_range_excluding_lines(axis);
                    let max_content = m.max_content(i, axis_tracks);
                    find_size_of_fr(&axis_tracks[range], max_content)
                })
                .fold(0.0f32, f32::max);
            let flex_fraction = from_tracks.max(from_items);
            let hypothetical: f32 = axis_tracks
                .iter()
                .map(|t| if t.max.is_fr() { t.base_size.max(t.max.value() * flex_fraction) } else { t.base_size })
                .sum();
            let min = axis_min_size.unwrap_or(0.0);
            let max = axis_max_size.unwrap_or(f32::INFINITY);
            if hypothetical < min {
                find_size_of_fr(axis_tracks, min)
            } else if hypothetical > max {
                find_size_of_fr(axis_tracks, max)
            } else {
                flex_fraction
            }
        }
    };
    for t in axis_tracks.iter_mut().filter(|t| t.max.is_fr()) {
        t.base_size = t.base_size.max(t.max.value() * flex_fraction);
    }
}

/// §11.7.1 find the size of an `fr`.
fn find_size_of_fr(tracks: &[GridTrack], space_to_fill: f32) -> f32 {
    if space_to_fill == 0.0 {
        return 0.0;
    }
    let mut hypothetical = f32::INFINITY;
    for _ in 0..tracks.len() + 1 {
        let mut used = 0.0;
        let mut naive_sum = 0.0;
        for t in tracks {
            if t.max.is_fr() && t.max.value() * hypothetical >= t.base_size {
                naive_sum += t.max.value();
            } else {
                used += t.base_size;
            }
        }
        let leftover = space_to_fill - used;
        let previous = hypothetical;
        hypothetical = leftover / naive_sum.max(1.0);
        let valid = tracks.iter().all(|t| {
            if t.max.is_fr() {
                let f = t.max.value();
                f * hypothetical >= t.base_size || f * previous < t.base_size
            } else {
                true
            }
        });
        if valid {
            break;
        }
    }
    hypothetical
}

/// §11.8: `align-content: stretch` grows auto tracks.
fn stretch_auto_tracks(axis_tracks: &mut [GridTrack], axis_min_size: Option<f32>, available: AvailableSpace) {
    let n = axis_tracks.iter().filter(|t| t.max.is_auto()).count();
    if n == 0 {
        return;
    }
    let used: f32 = axis_tracks.iter().map(|t| t.base_size).sum();
    let free = if available.is_definite() { available.compute_free_space(used) } else { axis_min_size.map_or(0.0, |s| s - used) };
    if free > 0.0 {
        let extra = free / n as f32;
        for t in axis_tracks.iter_mut().filter(|t| t.max.is_auto()) {
            t.base_size += extra;
        }
    }
}
