//! css-grid-1 §8.5 auto placement.

use super::coordinates::{MAX_OZ_LINE, MIN_OZ_LINE, NonNamedPlacement, OriginZeroPlacement, PlacementLine};
use super::item::GridItem;
use super::named::NamedLineResolver;
use super::occupancy::{CellOccupancyMatrix, CellOccupancyState};
use crate::compute::scratch::Scratch;
use crate::geometry::{AbsoluteAxis, Line};
use crate::style::{AlignItems, GridAutoFlow, OriginZeroLine, Position};
use crate::tree::{LayoutTree, NodeId};

#[inline]
fn advance(p: OriginZeroLine) -> OriginZeroLine {
    OriginZeroLine(p.0.saturating_add(1))
}

#[inline]
fn resolve_indefinite_grid_span(position: OriginZeroLine, span: u16) -> Line<OriginZeroLine> {
    let line = |v: i32| OriginZeroLine(v.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    Line { start: line(position.0 as i32), end: line(position.0 as i32 + span as i32) }
}

/// A child's placement in both axes.
#[derive(Clone, Copy)]
struct Placement {
    horizontal: Line<OriginZeroPlacement>,
    vertical: Line<OriginZeroPlacement>,
}

impl Placement {
    #[inline]
    fn get(&self, axis: AbsoluteAxis) -> Line<OriginZeroPlacement> {
        match axis {
            AbsoluteAxis::Horizontal => self.horizontal,
            AbsoluteAxis::Vertical => self.vertical,
        }
    }
}

pub(super) fn place_grid_items<T: LayoutTree + ?Sized>(
    tree: &T,
    node: NodeId,
    matrix: &mut CellOccupancyMatrix,
    items: &mut Vec<GridItem>,
    auto_flow: GridAutoFlow,
    align_items: AlignItems,
    justify_items: AlignItems,
    resolver: &NamedLineResolver,
) {
    let primary_axis = auto_flow.primary_axis();
    let secondary_axis = primary_axis.other();
    let explicit_col_count = matrix.track_counts(AbsoluteAxis::Horizontal).explicit;
    let explicit_row_count = matrix.track_counts(AbsoluteAxis::Vertical).explicit;

    // In-flow children with their origin-zero placements.
    let children: Scratch<(usize, NodeId, Placement)> = Scratch::collect(
        tree.children(node)
        .enumerate()
        .filter(|(_, child)| {
            let s = tree.style(*child);
            !s.generates_no_box() && s.position != Position::Absolute
        })
        .map(|(index, child)| {
            let s = tree.style(child);
            let horizontal = resolver.resolve_column_names(tree, s.grid_column).map(|p| p.into_origin_zero(explicit_col_count));
            let vertical = resolver.resolve_row_names(tree, s.grid_row).map(|p| p.into_origin_zero(explicit_row_count));
            (index, child, Placement { horizontal, vertical })
        }),
    );
    let _ = NonNamedPlacement::Auto;

    let record = |matrix: &mut CellOccupancyMatrix,
                      items: &mut Vec<GridItem>,
                      child: NodeId,
                      index: usize,
                      primary_span: Line<OriginZeroLine>,
                      secondary_span: Line<OriginZeroLine>,
                      state: CellOccupancyState| {
        let primary_span = clamp_span(primary_span);
        let secondary_span = clamp_span(secondary_span);
        matrix.mark_area_as(primary_axis, primary_span, secondary_span, state);
        let (col_span, row_span) = match primary_axis {
            AbsoluteAxis::Horizontal => (primary_span, secondary_span),
            AbsoluteAxis::Vertical => (secondary_span, primary_span),
        };
        items.push(GridItem::new(child, col_span, row_span, tree.style(child), align_items, justify_items, index as u16));
    };

    // 1. Definite in both axes.
    for &(index, child, p) in children.iter().filter(|(_, _, p)| p.horizontal.is_definite() && p.vertical.is_definite()) {
        let primary_span = p.get(primary_axis).resolve_definite_grid_lines();
        let secondary_span = p.get(secondary_axis).resolve_definite_grid_lines();
        record(matrix, items, child, index, primary_span, secondary_span, CellOccupancyState::DefinitelyPlaced);
    }

    // 2. Definite in the secondary axis only.
    for &(index, child, p) in
        children.iter().filter(|(_, _, p)| p.get(secondary_axis).is_definite() && !p.get(primary_axis).is_definite())
    {
        let (primary_span, secondary_span) = place_definite_secondary_axis_item(matrix, p, auto_flow);
        record(matrix, items, child, index, primary_span, secondary_span, CellOccupancyState::AutoPlaced);
    }

    // 3. Everything else, walking the cursor.
    let start = (matrix.track_counts(primary_axis).implicit_start_line(), matrix.track_counts(secondary_axis).implicit_start_line());
    let mut cursor = start;
    for &(index, child, p) in children.iter().filter(|(_, _, p)| !p.get(secondary_axis).is_definite()) {
        let (primary_span, secondary_span) = place_indefinitely_positioned_item(matrix, p, auto_flow, cursor);
        record(matrix, items, child, index, primary_span, secondary_span, CellOccupancyState::AutoPlaced);
        cursor = if auto_flow.is_dense() { start } else { (primary_span.end, secondary_span.start) };
    }
}

fn place_definite_secondary_axis_item(
    matrix: &CellOccupancyMatrix,
    p: Placement,
    auto_flow: GridAutoFlow,
) -> (Line<OriginZeroLine>, Line<OriginZeroLine>) {
    let primary_axis = auto_flow.primary_axis();
    let secondary_axis = primary_axis.other();
    let primary_start = matrix.track_counts(primary_axis).implicit_start_line();
    let secondary_span = p.get(secondary_axis).resolve_definite_grid_lines();
    let mut position = if auto_flow.is_dense() {
        primary_start
    } else {
        matrix.last_of_type(primary_axis, secondary_span.start, CellOccupancyState::AutoPlaced).unwrap_or(primary_start)
    };
    let primary_span_len = p.get(primary_axis).indefinite_span();
    loop {
        let primary_span = resolve_indefinite_grid_span(position, primary_span_len);
        match matrix.line_area_collision_jump(primary_axis, primary_span, secondary_span) {
            None => return (primary_span, secondary_span),
            Some(next) => position = next,
        }
    }
}

fn place_indefinitely_positioned_item(
    matrix: &CellOccupancyMatrix,
    p: Placement,
    auto_flow: GridAutoFlow,
    cursor: (OriginZeroLine, OriginZeroLine),
) -> (Line<OriginZeroLine>, Line<OriginZeroLine>) {
    let primary_axis = auto_flow.primary_axis();
    let secondary_axis = primary_axis.other();
    let primary_style = p.get(primary_axis);
    let secondary_style = p.get(secondary_axis);
    let secondary_span_len = secondary_style.indefinite_span();
    let primary_start = matrix.track_counts(primary_axis).implicit_start_line();
    let primary_end = matrix.track_counts(primary_axis).implicit_end_line();
    let secondary_start = matrix.track_counts(secondary_axis).implicit_start_line();
    let (mut primary_idx, mut secondary_idx) = cursor;

    if primary_style.is_definite() {
        let primary_span = primary_style.resolve_definite_grid_lines();
        secondary_idx = if auto_flow.is_dense() {
            secondary_start
        } else if primary_span.start < primary_idx {
            advance(secondary_idx)
        } else {
            secondary_idx
        };
        loop {
            let secondary_span = resolve_indefinite_grid_span(secondary_idx, secondary_span_len);
            if let Some(next) = matrix.line_area_collision_jump(secondary_axis, secondary_span, primary_span) {
                secondary_idx = next;
                continue;
            }
            return (primary_span, secondary_span);
        }
    }

    let primary_span_len = primary_style.indefinite_span();
    let spans_all_primary_tracks = primary_span_len as usize >= matrix.track_counts(primary_axis).len();
    loop {
        let primary_span = resolve_indefinite_grid_span(primary_idx, primary_span_len);
        let secondary_span = resolve_indefinite_grid_span(secondary_idx, secondary_span_len);
        if primary_span.end > primary_end {
            if primary_idx == primary_start {
                return (primary_span, secondary_span);
            }
            secondary_idx = advance(secondary_idx);
            primary_idx = primary_start;
            continue;
        }
        if spans_all_primary_tracks {
            match matrix.occupied_track_jump(secondary_axis, secondary_span) {
                Some(next) => {
                    secondary_idx = next;
                    primary_idx = primary_start;
                    continue;
                }
                None => return (primary_span, secondary_span),
            }
        }
        if let Some(next) = matrix.line_area_collision_jump(primary_axis, primary_span, secondary_span) {
            primary_idx = next;
            continue;
        }
        return (primary_span, secondary_span);
    }
}

#[inline]
fn clamp_span(span: Line<OriginZeroLine>) -> Line<OriginZeroLine> {
    let start = span.start.0.clamp(MIN_OZ_LINE, MAX_OZ_LINE - 1);
    let end = span.end.0.clamp(start + 1, MAX_OZ_LINE);
    Line { start: OriginZeroLine(start), end: OriginZeroLine(end) }
}

crate::compute::scratch::pooled!((usize, NodeId, Placement));
