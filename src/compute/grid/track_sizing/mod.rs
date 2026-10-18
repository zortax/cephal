//! css-grid-1 §11 track sizing.

mod distribute;
mod intrinsic;

use super::coordinates::TrackCounts;
use super::item::GridItem;
use super::track::GridTrack;
use crate::compute::LayoutTreeExt;
use crate::compute::scratch::Scratch;
use crate::geometry::{AbstractAxis, AvailableSpace, Line, Size};
use crate::style::{AlignContent, AlignContentKeyword, CalcId, OriginZeroLine};
use crate::tree::{CacheAccess, LayoutTree, SizingMode};
use distribute::distribute_space_up_to_limits;

/// Size estimate for a track in the other axis during this axis' sizing.
pub(super) type TrackSizeEstimate = fn(&GridTrack, Option<f32>, &dyn Fn(CalcId, f32) -> f32) -> Option<f32>;

pub(super) fn estimate_from_max_sizing_function(track: &GridTrack, basis: Option<f32>, calc: &dyn Fn(CalcId, f32) -> f32) -> Option<f32> {
    track.max.definite_value(basis, &calc)
}

pub(super) fn estimate_from_base_size(track: &GridTrack, _: Option<f32>, _: &dyn Fn(CalcId, f32) -> f32) -> Option<f32> {
    Some(track.base_size)
}

/// Stably sorts `order`, a permutation of `items`, by `key`: the order `sort_by_key` would leave
/// the items in. Only the indices move.
pub(super) fn sort_order_by_key(order: &mut [u32], items: &[GridItem], key: impl Fn(&GridItem) -> u64) {
    if order.is_sorted_by_key(|&i| key(&items[i as usize])) {
        return;
    }
    let mut keyed: Scratch<(u64, u32)> = Scratch::collect(order.iter().map(|&i| (key(&items[i as usize]), i)));
    if keyed.len() <= 64 {
        keyed.sort_by_key(|&(k, _)| k);
    } else {
        // Least significant digit first radix sort, skipping the bytes every key shares; each
        // pass is stable, so equal keys keep their order.
        let (and, or) = keyed.iter().fold((u64::MAX, 0), |(and, or), &(k, _)| (and & k, or | k));
        let varying = and ^ or;
        let mut buffer: Scratch<(u64, u32)> = Scratch::collect(keyed.iter().copied());
        for shift in (0..64).step_by(8) {
            if (varying >> shift) & 0xFF == 0 {
                continue;
            }
            let mut starts = [0u32; 256];
            for &(k, _) in keyed.iter() {
                starts[(k >> shift) as usize & 0xFF] += 1;
            }
            let mut sum = 0;
            for start in starts.iter_mut() {
                (*start, sum) = (sum, sum + *start);
            }
            for &(k, i) in keyed.iter() {
                let start = &mut starts[(k >> shift) as usize & 0xFF];
                buffer[*start as usize] = (k, i);
                *start += 1;
            }
            core::mem::swap(&mut keyed, &mut buffer);
        }
    }
    for (slot, &(_, i)) in order.iter_mut().zip(keyed.iter()) {
        *slot = i;
    }
}

/// Sort key in the order of origin-zero lines.
#[inline]
pub(super) fn line_key(line: OriginZeroLine) -> u64 {
    (line.0 as u16 ^ 0x8000) as u64
}

/// Items without flexible tracks first, then by span, then by start line.
#[inline]
pub(super) fn cross_flex_then_span_then_start_key(item: &GridItem, axis: AbstractAxis) -> u64 {
    let placement = item.placement(axis);
    ((item.crosses_flexible_track(axis) as u64) << 32)
        | ((super::coordinates::span_of(placement) as u64) << 16)
        | line_key(placement.start)
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
    /// The order to visit `items` in, as indices; sorting reorders only these.
    pub order: &'a mut [u32],
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
        order,
        get_track_size_estimate,
        has_baseline_aligned_item,
        run_mode,
    } = p;
    let percentage_basis = inner_node_size.get_abstract(axis).or(axis_min_size);
    initialize_track_sizes(tree, axis_tracks, percentage_basis);
    if has_baseline_aligned_item {
        if run_mode == crate::tree::RunMode::ComputeSize {
            crate::compute::speculate(tree, |tree| resolve_item_baselines(tree, axis, items, order, inner_node_size));
        } else {
            resolve_item_baselines(tree, axis, items, order, inner_node_size);
        }
    }
    // Every track fixed: nothing to size.
    {
        let calc = |id, basis| tree.resolve_calc(id, basis);
        if axis_tracks.iter().all(|t| t.base_size == t.growth_limit && t.min.definite_value(percentage_basis, &calc).is_some()) {
            return;
        }
    }
    // Sizing measures items many times over; resolve what needs no basis once.
    for item in items.iter_mut() {
        item.resolve_fixed_lengths();
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
        order,
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
        order,
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
    order: &mut [u32],
    inner_node_size: Size<Option<f32>>,
) {
    let other_axis = axis.other();
    sort_order_by_key(order, items, |item| line_key(item.placement(other_axis).start));
    let mut rest = &order[..];
    while !rest.is_empty() {
        let current_row = items[rest[0] as usize].placement(other_axis).start;
        let split = rest.iter().position(|&i| items[i as usize].placement(other_axis).start != current_row).unwrap_or(rest.len());
        let (row_items, tail) = rest.split_at(split);
        rest = tail;
        if row_items.iter().filter(|&&i| items[i as usize].participates_in_baseline_alignment()).count() <= 1 {
            continue;
        }
        for &i in row_items {
            let item = &mut items[i as usize];
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
            .map(|&i| &items[i as usize])
            .filter(|i| i.participates_in_baseline_alignment())
            .map(|i| i.baseline.unwrap_or(0.0))
            .fold(f32::NEG_INFINITY, f32::max);
        for &i in row_items {
            let item = &mut items[i as usize];
            if item.participates_in_baseline_alignment() {
                item.baseline_shim = row_max - item.baseline.unwrap_or(0.0);
            }
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
    order: &[u32],
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
            let mut from_items = 0.0f32;
            for &i in order {
                let item = &mut items[i as usize];
                if !item.crosses_flexible_track(axis) {
                    continue;
                }
                let range = item.track_range_excluding_lines(axis);
                let max_content = m.max_content(item, axis_tracks);
                from_items = f32::max(from_items, find_size_of_fr(&axis_tracks[range], max_content));
            }
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
