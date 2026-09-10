//! Grid lines in origin-zero form and track counts.

use crate::geometry::Line;
use crate::style::{GridPlacement, OriginZeroLine};

pub(super) const MAX_GRID_TRACKS: u16 = 10_000;
pub(super) const MIN_OZ_LINE: i16 = -(MAX_GRID_TRACKS as i16);
pub(super) const MAX_OZ_LINE: i16 = MAX_GRID_TRACKS as i16;

/// Converts a 1-based (negative from end) CSS line to origin-zero.
#[inline]
pub(super) fn line_into_origin_zero(line: i16, explicit_track_count: u16) -> OriginZeroLine {
    let explicit_line_count = explicit_track_count as i16 + 1;
    let oz = match line.cmp(&0) {
        core::cmp::Ordering::Greater => line - 1,
        core::cmp::Ordering::Less => line + explicit_line_count,
        core::cmp::Ordering::Equal => 0,
    };
    OriginZeroLine(oz.clamp(MIN_OZ_LINE, MAX_OZ_LINE))
}

/// Placement with names resolved away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NonNamedPlacement {
    Auto,
    Line(i16),
    Span(u16),
}

impl NonNamedPlacement {
    #[inline]
    pub fn from_style(p: GridPlacement) -> Self {
        match p {
            GridPlacement::Auto | GridPlacement::NamedLine(..) => Self::Auto,
            GridPlacement::Line(l) => Self::Line(l),
            GridPlacement::Span(s) => Self::Span(s),
            GridPlacement::NamedSpan(..) => Self::Span(1),
        }
    }
    #[inline]
    pub fn into_origin_zero(self, explicit_track_count: u16) -> OriginZeroPlacement {
        match self {
            Self::Auto => OriginZeroPlacement::Auto,
            Self::Span(span) => OriginZeroPlacement::Span(span.clamp(1, MAX_GRID_TRACKS)),
            Self::Line(0) => OriginZeroPlacement::Auto,
            Self::Line(line) => OriginZeroPlacement::Line(line_into_origin_zero(line, explicit_track_count)),
        }
    }
}

/// Placement in origin-zero coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OriginZeroPlacement {
    Auto,
    Line(OriginZeroLine),
    Span(u16),
}

pub(super) trait PlacementLine {
    fn is_definite(&self) -> bool;
    fn indefinite_span(&self) -> u16;
    fn resolve_definite_grid_lines(&self) -> Line<OriginZeroLine>;
    fn resolve_absolutely_positioned_grid_tracks(&self) -> Line<Option<OriginZeroLine>>;
}

impl PlacementLine for Line<OriginZeroPlacement> {
    #[inline]
    fn is_definite(&self) -> bool {
        matches!((self.start, self.end), (OriginZeroPlacement::Line(_), _) | (_, OriginZeroPlacement::Line(_)))
    }
    #[inline]
    fn indefinite_span(&self) -> u16 {
        use OriginZeroPlacement as P;
        match (self.start, self.end) {
            (P::Line(_), P::Span(span)) | (P::Span(span), P::Line(_)) => span,
            (P::Span(span), P::Auto) | (P::Auto, P::Span(span)) | (P::Span(span), P::Span(_)) => span,
            _ => 1,
        }
    }
    fn resolve_definite_grid_lines(&self) -> Line<OriginZeroLine> {
        use OriginZeroPlacement as P;
        match (self.start, self.end) {
            (P::Line(a), P::Line(b)) => {
                if a == b { Line { start: a, end: a + 1 } } else { Line { start: a.min(b), end: a.max(b) } }
            }
            (P::Line(line), P::Span(span)) => Line { start: line, end: line + span },
            (P::Line(line), P::Auto) => Line { start: line, end: line + 1 },
            (P::Span(span), P::Line(line)) => Line { start: line - span, end: line },
            (P::Auto, P::Line(line)) => Line { start: line - 1, end: line },
            _ => unreachable!("indefinite placement"),
        }
    }
    fn resolve_absolutely_positioned_grid_tracks(&self) -> Line<Option<OriginZeroLine>> {
        use OriginZeroPlacement as P;
        match (self.start, self.end) {
            (P::Line(a), P::Line(b)) => {
                if a == b {
                    Line { start: Some(a), end: Some(a + 1) }
                } else {
                    Line { start: Some(a.min(b)), end: Some(a.max(b)) }
                }
            }
            (P::Line(line), P::Span(span)) => Line { start: Some(line), end: Some(line + span) },
            (P::Line(line), P::Auto) => Line { start: Some(line), end: None },
            (P::Span(span), P::Line(line)) => Line { start: Some(line - span), end: Some(line) },
            (P::Auto, P::Line(line)) => Line { start: None, end: Some(line) },
            _ => Line { start: None, end: None },
        }
    }
}

#[inline]
pub(super) fn span_of(line: Line<OriginZeroLine>) -> u16 {
    (line.end.0 - line.start.0).max(0) as u16
}

/// Implicit tracks before, explicit tracks, implicit tracks after.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct TrackCounts {
    pub negative_implicit: u16,
    pub explicit: u16,
    pub positive_implicit: u16,
}

impl TrackCounts {
    #[inline]
    pub const fn len(&self) -> usize {
        (self.negative_implicit + self.explicit + self.positive_implicit) as usize
    }
    #[inline]
    pub const fn implicit_start_line(&self) -> OriginZeroLine {
        OriginZeroLine(-(self.negative_implicit as i16))
    }
    #[inline]
    pub const fn implicit_end_line(&self) -> OriginZeroLine {
        OriginZeroLine((self.explicit + self.positive_implicit) as i16)
    }
    #[inline]
    pub const fn oz_line_to_next_track(&self, line: OriginZeroLine) -> i16 {
        line.0 + self.negative_implicit as i16
    }
    #[inline]
    pub const fn oz_line_range_to_track_range(&self, l: Line<OriginZeroLine>) -> core::ops::Range<i16> {
        self.oz_line_to_next_track(l.start)..self.oz_line_to_next_track(l.end)
    }
    #[inline]
    pub const fn track_to_prev_oz_line(&self, index: u16) -> OriginZeroLine {
        OriginZeroLine(index as i16 - self.negative_implicit as i16)
    }
    /// Index into the interleaved gutter/track list for a line.
    #[inline]
    pub fn try_into_track_vec_index(&self, line: OriginZeroLine) -> Option<usize> {
        if line.0 < -(self.negative_implicit as i16) || line.0 > (self.explicit + self.positive_implicit) as i16 {
            return None;
        }
        Some(2 * (line.0 + self.negative_implicit as i16) as usize)
    }
    #[inline]
    pub fn track_vec_index(&self, line: OriginZeroLine) -> usize {
        self.try_into_track_vec_index(line).expect("line within the implicit grid")
    }
}
