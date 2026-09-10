//! Estimating implicit track counts from item placements.

use super::coordinates::{MAX_OZ_LINE, MIN_OZ_LINE, NonNamedPlacement, OriginZeroPlacement, PlacementLine, TrackCounts};
use crate::geometry::Line;
use crate::style::{OriginZeroLine, Style};

pub(super) fn compute_grid_size_estimate<'a>(
    explicit_col_count: u16,
    explicit_row_count: u16,
    child_styles: impl Iterator<Item = &'a Style>,
) -> (TrackCounts, TrackCounts) {
    let (mut col_min, mut col_max, mut col_max_span) = (OriginZeroLine(0), OriginZeroLine(0), 0u16);
    let (mut row_min, mut row_max, mut row_max_span) = (OriginZeroLine(0), OriginZeroLine(0), 0u16);
    for style in child_styles {
        let (cmin, cmax, cspan) = child_min_line_max_line_span(style.grid_column, explicit_col_count);
        let (rmin, rmax, rspan) = child_min_line_max_line_span(style.grid_row, explicit_row_count);
        col_min = col_min.min(cmin);
        col_max = col_max.max(cmax);
        col_max_span = col_max_span.max(cspan);
        row_min = row_min.min(rmin);
        row_max = row_max.max(rmax);
        row_max_span = row_max_span.max(rspan);
    }
    let neg_cols = col_min.implied_negative_implicit_tracks();
    let mut pos_cols = col_max.implied_positive_implicit_tracks(explicit_col_count);
    let neg_rows = row_min.implied_negative_implicit_tracks();
    let mut pos_rows = row_max.implied_positive_implicit_tracks(explicit_row_count);
    if neg_cols + explicit_col_count + pos_cols < col_max_span {
        pos_cols = col_max_span - explicit_col_count - neg_cols;
    }
    if neg_rows + explicit_row_count + pos_rows < row_max_span {
        pos_rows = row_max_span - explicit_row_count - neg_rows;
    }
    (
        TrackCounts { negative_implicit: neg_cols, explicit: explicit_col_count, positive_implicit: pos_cols },
        TrackCounts { negative_implicit: neg_rows, explicit: explicit_row_count, positive_implicit: pos_rows },
    )
}

fn child_min_line_max_line_span(
    line: Line<crate::style::GridPlacement>,
    explicit_track_count: u16,
) -> (OriginZeroLine, OriginZeroLine, u16) {
    use OriginZeroPlacement as P;
    let oz: Line<OriginZeroPlacement> = Line {
        start: NonNamedPlacement::from_style(line.start).into_origin_zero(explicit_track_count),
        end: NonNamedPlacement::from_style(line.end).into_origin_zero(explicit_track_count),
    };
    let min = match (oz.start, oz.end) {
        (P::Line(a), P::Line(b)) => a.min(b),
        (P::Line(t), P::Auto) | (P::Line(t), P::Span(_)) => t,
        (P::Auto, P::Line(t)) => t - 1,
        (P::Span(span), P::Line(t)) => t - span,
        _ => OriginZeroLine(0),
    };
    let max = match (oz.start, oz.end) {
        (P::Line(a), P::Line(b)) => {
            if a == b { a + 1 } else { a.max(b) }
        }
        (P::Line(t), P::Auto) => t + 1,
        (P::Line(t), P::Span(span)) => t + span,
        (P::Auto, P::Line(t)) | (P::Span(_), P::Line(t)) => t,
        _ => OriginZeroLine(0),
    };
    let span = match (oz.start, oz.end) {
        (P::Auto | P::Span(_), P::Auto | P::Span(_)) => oz.indefinite_span(),
        _ => 1,
    };
    (OriginZeroLine(min.0.max(MIN_OZ_LINE)), OriginZeroLine(max.0.min(MAX_OZ_LINE)), span)
}
