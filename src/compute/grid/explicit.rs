//! Explicit grid sizing and track list initialisation.

use super::coordinates::{MAX_GRID_TRACKS, TrackCounts};
use super::track::{GridTrack, GridTrackKind};
use crate::geometry::{AbsoluteAxis, MaybeMath};
use crate::geometry::Size;
use crate::style::{CalcId, GridContainerStyle, GridTemplateComponent, LengthPercentage, RepetitionCount, TrackSizingFunction};

pub(super) enum AutoRepeatStrategy {
    MaxRepetitionsThatDoNotOverflow,
    MinRepetitionsThatDoOverflow,
}

/// Returns `(auto repetition count, explicit track count)`.
pub(super) fn compute_explicit_grid_size_in_axis(
    style: &GridContainerStyle,
    gap: Size<LengthPercentage>,
    auto_fit_container_size: Option<f32>,
    strategy: AutoRepeatStrategy,
    calc: &impl Fn(CalcId, f32) -> f32,
    axis: AbsoluteAxis,
) -> (u16, u16) {
    let template = style.template_tracks(axis);
    if template.is_empty() {
        return (0, 0);
    }
    if template.iter().any(|c| matches!(c, GridTemplateComponent::Repeat(r) if r.tracks.is_empty())) {
        return (0, 0);
    }
    let non_auto_repeating_track_count = template
        .iter()
        .fold(0u32, |count, c| {
            count.saturating_add(match c {
                GridTemplateComponent::Single(_) => 1,
                GridTemplateComponent::Repeat(r) => match r.count {
                    RepetitionCount::Count(n) => n as u32 * r.tracks.len() as u32,
                    _ => 0,
                },
            })
        })
        .min(MAX_GRID_TRACKS as u32) as u16;
    let auto_repetition_count = template.iter().filter(|c| c.is_auto_repetition()).count() as u16;
    let all_fixed = template.iter().all(|c| match c {
        GridTemplateComponent::Single(t) => t.has_fixed_component(),
        GridTemplateComponent::Repeat(r) => r.tracks.iter().all(|t| t.has_fixed_component()),
    });
    let template_is_valid = auto_repetition_count == 0 || (auto_repetition_count == 1 && all_fixed);
    if !template_is_valid {
        return (0, 0);
    }
    if auto_repetition_count == 0 {
        return (0, non_auto_repeating_track_count);
    }

    let mut auto_repeat_insertion_point = 0u32;
    let repetition = template
        .iter()
        .find_map(|c| match c {
            GridTemplateComponent::Single(_) => {
                auto_repeat_insertion_point = auto_repeat_insertion_point.saturating_add(1);
                None
            }
            GridTemplateComponent::Repeat(r) => match r.count {
                RepetitionCount::Count(n) => {
                    auto_repeat_insertion_point = auto_repeat_insertion_point.saturating_add(n as u32 * r.tracks.len() as u32);
                    None
                }
                _ => Some(r),
            },
        })
        .expect("one auto repetition");
    let repetition_track_count = repetition.tracks.len().min(u16::MAX as usize) as u16;

    let num_repetitions: u32 = match auto_fit_container_size {
        None => 1,
        Some(inner) => {
            let parent_size = Some(inner);
            let definite = |t: TrackSizingFunction| {
                let max = t.max.definite_value(parent_size, calc);
                let min = t.min.definite_value(parent_size, calc);
                max.map(|m| m.maybe_max(min)).or(min).unwrap_or(0.0)
            };
            let non_repeating_used: f32 = template
                .iter()
                .map(|c| match c {
                    GridTemplateComponent::Single(t) => definite(*t),
                    GridTemplateComponent::Repeat(r) => match r.count {
                        RepetitionCount::Count(n) => r.tracks.iter().map(|t| definite(*t)).sum::<f32>() * n as f32,
                        _ => 0.0,
                    },
                })
                .sum();
            let gap_size = gap.get(axis).resolve_or_zero(Some(inner), calc);
            let per_repetition_track_used: f32 = repetition.tracks.iter().map(|t| definite(*t)).sum();
            let first_and_non_repeating = non_repeating_used
                + per_repetition_track_used
                + ((non_auto_repeating_track_count as u32 + repetition_track_count as u32).saturating_sub(1) as f32 * gap_size);
            if first_and_non_repeating > inner {
                1
            } else {
                let per_repetition_used = per_repetition_track_used + repetition_track_count as f32 * gap_size;
                let fit = (inner - first_and_non_repeating) / per_repetition_used;
                match strategy {
                    AutoRepeatStrategy::MaxRepetitionsThatDoNotOverflow => (fit.floor() as u32).saturating_add(1),
                    AutoRepeatStrategy::MinRepetitionsThatDoOverflow => (fit.ceil() as u32).saturating_add(1),
                }
            }
        }
    };
    let remaining = (MAX_GRID_TRACKS as u32).saturating_sub(auto_repeat_insertion_point);
    let num_repetitions = if remaining == 0 {
        0
    } else {
        let max_repetitions = remaining.div_ceil(repetition_track_count as u32);
        num_repetitions.clamp(1, max_repetitions) as u16
    };
    let count = (non_auto_repeating_track_count as u32 + repetition_track_count as u32 * num_repetitions as u32)
        .min(MAX_GRID_TRACKS as u32) as u16;
    (num_repetitions, count)
}

/// Builds the interleaved gutter/track list for one axis.
pub(super) fn initialize_grid_tracks(
    tracks: &mut Vec<GridTrack>,
    counts: TrackCounts,
    style: &GridContainerStyle,
    gap: LengthPercentage,
    axis: AbsoluteAxis,
    auto_repetition_count: u16,
    track_has_items: impl Fn(usize) -> bool,
) {
    let template = style.template_tracks(axis);
    let auto_tracks = style.auto_tracks(axis);
    tracks.clear();
    tracks.reserve(counts.len() * 2 + 1);
    tracks.push(GridTrack::gutter(gap));

    let auto_track_count = auto_tracks.len();
    if counts.negative_implicit > 0 {
        if auto_track_count == 0 {
            create_implicit_tracks(tracks, counts.negative_implicit, core::iter::repeat(TrackSizingFunction::AUTO), gap);
        } else {
            let offset = auto_track_count - (counts.negative_implicit as usize % auto_track_count);
            create_implicit_tracks(tracks, counts.negative_implicit, auto_tracks.iter().copied().cycle().skip(offset), gap);
        }
    }

    let mut current = counts.negative_implicit as usize;
    let explicit_limit = (counts.negative_implicit + counts.explicit) as usize;
    if counts.explicit > 0 {
        for component in template {
            match component {
                GridTemplateComponent::Single(t) => {
                    if current < explicit_limit {
                        tracks.push(GridTrack::new(t.min, t.max));
                        tracks.push(GridTrack::gutter(gap));
                        current += 1;
                    }
                }
                GridTemplateComponent::Repeat(r) => match r.count {
                    RepetitionCount::Count(n) => {
                        let repeated = (r.tracks.len() * n as usize).min(explicit_limit - current);
                        for t in r.tracks.iter().cycle().take(repeated) {
                            tracks.push(GridTrack::new(t.min, t.max));
                            tracks.push(GridTrack::gutter(gap));
                            current += 1;
                        }
                    }
                    RepetitionCount::AutoFit | RepetitionCount::AutoFill => {
                        let repeated = (r.tracks.len() * auto_repetition_count as usize).min(explicit_limit - current);
                        for t in r.tracks.iter().cycle().take(repeated) {
                            let mut track = GridTrack::new(t.min, t.max);
                            let mut gutter = GridTrack::gutter(gap);
                            if r.count == RepetitionCount::AutoFit && !track_has_items(current) {
                                track.collapse();
                                gutter.collapse();
                            }
                            tracks.push(track);
                            tracks.push(gutter);
                            current += 1;
                        }
                        // Collapsed tracks at the end also collapse the gutter before them.
                        if r.count == RepetitionCount::AutoFit && current == counts.len() {
                            for prev in tracks.iter_mut().rev() {
                                if prev.kind == GridTrackKind::Track && !prev.is_collapsed {
                                    break;
                                }
                                prev.collapse();
                            }
                        }
                    }
                },
            }
        }
    }

    let grid_area_tracks = (counts.negative_implicit + counts.explicit) - current as u16;
    if auto_track_count == 0 {
        create_implicit_tracks(tracks, counts.positive_implicit + grid_area_tracks, core::iter::repeat(TrackSizingFunction::AUTO), gap);
    } else {
        create_implicit_tracks(tracks, counts.positive_implicit + grid_area_tracks, auto_tracks.iter().copied().cycle(), gap);
    }
    tracks.first_mut().unwrap().collapse();
    tracks.last_mut().unwrap().collapse();
}

fn create_implicit_tracks(
    tracks: &mut Vec<GridTrack>,
    count: u16,
    mut auto_tracks: impl Iterator<Item = TrackSizingFunction>,
    gap: LengthPercentage,
) {
    for _ in 0..count {
        let t = auto_tracks.next().expect("cycled iterator");
        tracks.push(GridTrack::new(t.min, t.max));
        tracks.push(GridTrack::gutter(gap));
    }
}
