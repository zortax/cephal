//! Which cells of the (implicit) grid are taken, as a dense row-major matrix.

use super::coordinates::TrackCounts;
use crate::geometry::{AbsoluteAxis, Line};
use crate::compute::scratch::Scratch;
use crate::style::OriginZeroLine;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum CellOccupancyState {
    #[default]
    Unoccupied,
    DefinitelyPlaced,
    AutoPlaced,
}

pub(super) struct CellOccupancyMatrix {
    columns: TrackCounts,
    rows: TrackCounts,
    /// `rows.len() * columns.len()` cells, row-major.
    cells: Scratch<CellOccupancyState>,
}

impl CellOccupancyMatrix {
    pub fn with_track_counts(columns: TrackCounts, rows: TrackCounts) -> Self {
        let mut cells = Scratch::take();
        cells.resize(rows.len() * columns.len(), CellOccupancyState::Unoccupied);
        Self { cells, rows, columns }
    }

    #[inline]
    pub fn track_counts(&self, axis: AbsoluteAxis) -> &TrackCounts {
        match axis {
            AbsoluteAxis::Horizontal => &self.columns,
            AbsoluteAxis::Vertical => &self.rows,
        }
    }

    #[inline]
    fn cell(&self, row: usize, column: usize) -> CellOccupancyState {
        self.cells[row * self.columns.len() + column]
    }

    fn expand_to_fit_range(&mut self, row_span: Line<OriginZeroLine>, col_span: Line<OriginZeroLine>) {
        let req_neg_rows = (-(self.rows.negative_implicit as i16) - row_span.start.0).max(0) as usize;
        let req_pos_rows = (row_span.end.0 - self.rows.implicit_end_line().0).max(0) as usize;
        let req_neg_cols = (-(self.columns.negative_implicit as i16) - col_span.start.0).max(0) as usize;
        let req_pos_cols = (col_span.end.0 - self.columns.implicit_end_line().0).max(0) as usize;
        if req_neg_rows + req_pos_rows + req_neg_cols + req_pos_cols == 0 {
            return;
        }
        let (old_rows, old_cols) = (self.rows.len(), self.columns.len());
        let new_cols = old_cols + req_neg_cols + req_pos_cols;
        let new_rows = old_rows + req_neg_rows + req_pos_rows;
        let mut cells: Scratch<CellOccupancyState> = Scratch::take();
        cells.resize(new_rows * new_cols, CellOccupancyState::Unoccupied);
        for r in 0..old_rows {
            let src = &self.cells[r * old_cols..(r + 1) * old_cols];
            let dst = (r + req_neg_rows) * new_cols + req_neg_cols;
            cells[dst..dst + old_cols].copy_from_slice(src);
        }
        self.cells = cells;
        self.rows.negative_implicit += req_neg_rows as u16;
        self.rows.positive_implicit += req_pos_rows as u16;
        self.columns.negative_implicit += req_neg_cols as u16;
        self.columns.positive_implicit += req_pos_cols as u16;
    }

    pub fn mark_area_as(
        &mut self,
        primary_axis: AbsoluteAxis,
        primary_span: Line<OriginZeroLine>,
        secondary_span: Line<OriginZeroLine>,
        value: CellOccupancyState,
    ) {
        let (row_span, column_span) = match primary_axis {
            AbsoluteAxis::Horizontal => (secondary_span, primary_span),
            AbsoluteAxis::Vertical => (primary_span, secondary_span),
        };
        self.expand_to_fit_range(row_span, column_span);
        let rows = self.rows.oz_line_range_to_track_range(row_span);
        let cols = self.columns.oz_line_range_to_track_range(column_span);
        let width = self.columns.len();
        for r in rows {
            let base = r as usize * width;
            for c in cols.clone() {
                self.cells[base + c as usize] = value;
            }
        }
    }

    /// Next primary position past any occupied cell in the area, or `None` when the area is free.
    pub fn line_area_collision_jump(
        &self,
        primary_axis: AbsoluteAxis,
        primary_span: Line<OriginZeroLine>,
        secondary_span: Line<OriginZeroLine>,
    ) -> Option<OriginZeroLine> {
        let primary_counts = self.track_counts(primary_axis);
        let secondary_counts = self.track_counts(primary_axis.other());
        let p = primary_counts.oz_line_range_to_track_range(primary_span);
        let s = secondary_counts.oz_line_range_to_track_range(secondary_span);
        let p = p.start.max(0)..p.end.min(primary_counts.len() as i16);
        let s = s.start.max(0)..s.end.min(secondary_counts.len() as i16);
        let occupied = |si: i16, pi: i16| {
            let (row, col) = match primary_axis {
                AbsoluteAxis::Horizontal => (si, pi),
                AbsoluteAxis::Vertical => (pi, si),
            };
            self.cell(row as usize, col as usize) != CellOccupancyState::Unoccupied
        };
        // Every position before the end of an occupying run still overlaps it; jump past the
        // furthest run touching the area.
        let mut furthest: Option<i16> = None;
        for si in s {
            if let Some(pi) = p.clone().rev().find(|&pi| occupied(si, pi)) {
                let mut end = pi + 1;
                while end < primary_counts.len() as i16 && occupied(si, end) {
                    end += 1;
                }
                furthest = Some(furthest.map_or(end, |f| f.max(end)));
            }
        }
        furthest.map(|end| primary_counts.track_to_prev_oz_line(end as u16))
    }

    /// Line after the last non-empty track within `span`.
    pub fn occupied_track_jump(&self, axis: AbsoluteAxis, span: Line<OriginZeroLine>) -> Option<OriginZeroLine> {
        let counts = self.track_counts(axis);
        let range = counts.oz_line_range_to_track_range(span);
        let start = range.start.max(0);
        let end = range.end.min(counts.len() as i16);
        (start..end).rev().find(|&i| self.track_is_occupied(axis, i as usize)).map(|i| counts.track_to_prev_oz_line(i as u16) + 1)
    }

    fn track_is_occupied(&self, axis: AbsoluteAxis, index: usize) -> bool {
        let width = self.columns.len();
        match axis {
            AbsoluteAxis::Vertical => self.cells[index * width..(index + 1) * width].iter().any(|c| *c != CellOccupancyState::Unoccupied),
            AbsoluteAxis::Horizontal => (0..self.rows.len()).any(|r| self.cell(r, index) != CellOccupancyState::Unoccupied),
        }
    }

    #[inline]
    pub fn row_is_occupied(&self, row_index: usize) -> bool {
        row_index < self.rows.len() && self.track_is_occupied(AbsoluteAxis::Vertical, row_index)
    }
    #[inline]
    pub fn column_is_occupied(&self, column_index: usize) -> bool {
        column_index < self.columns.len() && self.track_is_occupied(AbsoluteAxis::Horizontal, column_index)
    }

    /// Line after the last cell of `kind` in the track starting at `start_at` on the other axis.
    pub fn last_of_type(
        &self,
        track_type: AbsoluteAxis,
        start_at: OriginZeroLine,
        kind: CellOccupancyState,
    ) -> Option<OriginZeroLine> {
        let counts = self.track_counts(track_type.other());
        let index = counts.oz_line_to_next_track(start_at);
        if index < 0 || index >= counts.len() as i16 {
            return None;
        }
        let along = self.track_counts(track_type);
        let last = (0..along.len()).rev().find(|&i| {
            let (row, col) = match track_type {
                AbsoluteAxis::Horizontal => (index as usize, i),
                AbsoluteAxis::Vertical => (i, index as usize),
            };
            self.cell(row, col) == kind
        });
        last.map(|i| along.track_to_prev_oz_line(i as u16))
    }
}

crate::compute::scratch::pooled!(CellOccupancyState);
