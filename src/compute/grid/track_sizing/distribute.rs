//! §11.5.1 distributing extra space across tracks.

use super::super::track::GridTrack;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IntrinsicContributionType {
    Minimum,
    Maximum,
}

/// Distributes `space` to affected tracks' `item_incurred_increase`, respecting limits; returns the remainder.
pub(super) fn distribute_space_up_to_limits(
    space_to_distribute: f32,
    tracks: &mut [GridTrack],
    track_is_affected: impl Fn(&GridTrack) -> bool,
    track_distribution_proportion: impl Fn(&GridTrack) -> f32,
    track_affected_property: impl Fn(&GridTrack) -> f32,
    track_limit: impl Fn(&GridTrack) -> f32,
) -> f32 {
    const THRESHOLD: f32 = 0.01;
    let mut space = space_to_distribute;
    for _ in 0..tracks.len() + 1 {
        if space <= THRESHOLD {
            break;
        }
        let growable = |t: &GridTrack| track_affected_property(t) + t.item_incurred_increase < track_limit(t) && track_is_affected(t);
        let proportion_sum: f32 = tracks.iter().filter(|t| growable(t)).map(&track_distribution_proportion).sum();
        if proportion_sum == 0.0 {
            break;
        }
        let min_increase_limit = tracks
            .iter()
            .filter(|t| growable(t))
            .map(|t| (track_limit(t) - track_affected_property(t) - t.item_incurred_increase) / track_distribution_proportion(t))
            .fold(f32::INFINITY, f32::min);
        let iteration_increase = min_increase_limit.min(space / proportion_sum);
        for t in tracks.iter_mut().filter(|t| track_is_affected(t)) {
            let increase = iteration_increase * track_distribution_proportion(t);
            if increase > 0.0 && track_affected_property(t) + t.item_incurred_increase + increase <= track_limit(t) + THRESHOLD {
                t.item_incurred_increase += increase;
                space -= increase;
            }
        }
    }
    space
}

pub(super) fn distribute_item_space_to_base_size(
    is_flex: bool,
    space: f32,
    tracks: &mut [GridTrack],
    track_is_affected: impl Fn(&GridTrack) -> bool,
    track_limit: impl Fn(&GridTrack) -> f32,
    contribution_type: IntrinsicContributionType,
    axis_inner_node_size: Option<f32>,
) {
    if is_flex {
        let filter = |t: &GridTrack| t.is_flexible() && track_is_affected(t);
        let flex_factor_sum: f32 = tracks.iter().filter(|t| filter(t)).map(|t| t.flex_factor()).sum();
        if flex_factor_sum > 0.0 {
            inner(space, tracks, filter, |t| t.flex_factor(), track_limit, contribution_type, axis_inner_node_size)
        } else {
            inner(space, tracks, filter, |_| 1.0, track_limit, contribution_type, axis_inner_node_size)
        }
    } else {
        inner(space, tracks, track_is_affected, |_| 1.0, track_limit, contribution_type, axis_inner_node_size)
    }

    fn inner(
        space: f32,
        tracks: &mut [GridTrack],
        track_is_affected: impl Fn(&GridTrack) -> bool,
        proportion: impl Fn(&GridTrack) -> f32,
        track_limit: impl Fn(&GridTrack) -> f32,
        contribution_type: IntrinsicContributionType,
        axis_inner_node_size: Option<f32>,
    ) {
        if space == 0.0 || !tracks.iter().any(&track_is_affected) {
            return;
        }
        let track_sizes: f32 = tracks.iter().map(|t| t.base_size).sum();
        let extra = (space - track_sizes).max(0.0);
        const THRESHOLD: f32 = 0.000001;
        let extra = distribute_space_up_to_limits(extra, tracks, &track_is_affected, &proportion, |t| t.base_size, &track_limit);
        if extra > THRESHOLD {
            // Beyond limits, only intrinsic (or max/fit-content) tracks grow; failing that, all of them.
            let mut filter: fn(&GridTrack) -> bool = match contribution_type {
                IntrinsicContributionType::Minimum => |t| t.max.is_intrinsic(),
                IntrinsicContributionType::Maximum => |t| t.max.is_max_or_fit_content(),
            };
            if !tracks.iter().any(|t| track_is_affected(t) && filter(t)) {
                filter = |_| true;
            }
            distribute_space_up_to_limits(
                extra,
                tracks,
                |t| track_is_affected(t) && filter(t),
                &proportion,
                |t| t.base_size,
                |t| t.fit_content_limit(axis_inner_node_size),
            );
        }
        for t in tracks.iter_mut() {
            if t.item_incurred_increase > t.base_size_planned_increase {
                t.base_size_planned_increase = t.item_incurred_increase;
            }
            t.item_incurred_increase = 0.0;
        }
    }
}

pub(super) fn distribute_item_space_to_growth_limit(
    space: f32,
    tracks: &mut [GridTrack],
    track_is_affected: impl Fn(&GridTrack) -> bool,
    axis_inner_node_size: Option<f32>,
) {
    if space == 0.0 || !tracks.iter().any(&track_is_affected) {
        return;
    }
    let track_sizes: f32 = tracks.iter().map(|t| if t.growth_limit == f32::INFINITY { t.base_size } else { t.growth_limit }).sum();
    let extra = (space - track_sizes).max(0.0);
    let is_growable = |t: &GridTrack| t.infinitely_growable || t.fit_content_limited_growth_limit(axis_inner_node_size) == f32::INFINITY;
    let growable_count = tracks.iter().filter(|t| track_is_affected(t) && is_growable(t)).count();
    if growable_count > 0 {
        let increase = extra / growable_count as f32;
        for t in tracks.iter_mut().filter(|t| track_is_affected(t) && is_growable(t)) {
            t.item_incurred_increase = increase;
        }
    } else {
        distribute_space_up_to_limits(
            extra,
            tracks,
            track_is_affected,
            |_| 1.0,
            |t| if t.growth_limit == f32::INFINITY { t.base_size } else { t.growth_limit },
            move |t| t.fit_content_limit(axis_inner_node_size),
        );
    }
    for t in tracks.iter_mut() {
        if t.item_incurred_increase > t.growth_limit_planned_increase {
            t.growth_limit_planned_increase = t.item_incurred_increase;
        }
        t.item_incurred_increase = 0.0;
    }
}
