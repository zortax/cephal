//! Resolving named lines and areas to line numbers.

use super::coordinates::{MAX_GRID_TRACKS, MAX_OZ_LINE, MIN_OZ_LINE, NonNamedPlacement};
use crate::geometry::{AbsoluteAxis, Line};
use crate::style::{GridContainerStyle, GridPlacement, GridTemplateComponent, Ident, RepetitionCount};
use crate::tree::{IdentSuffix, LayoutTree};
use std::collections::HashMap;

/// A line name key: a real ident, or a synthetic `<area>-start`/`-end` when none was interned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct NameKey(u64);

impl NameKey {
    #[inline]
    fn plain(id: Ident) -> Self {
        Self((id.0 as u64) << 2)
    }
    #[inline]
    fn synthetic(id: Ident, suffix: IdentSuffix) -> Self {
        Self(((id.0 as u64) << 2) | match suffix {
            IdentSuffix::Start => 1,
            IdentSuffix::End => 2,
        })
    }
}

type LineMap = HashMap<NameKey, Vec<u32>>;

pub(super) struct NamedLineResolver {
    row_lines: LineMap,
    column_lines: LineMap,
    area_column_count: u16,
    area_row_count: u16,
    explicit_column_count: u16,
    explicit_row_count: u16,
    column_line_name_pairs: Vec<(u32, Ident)>,
    row_line_name_pairs: Vec<(u32, Ident)>,
}

fn upsert(map: &mut LineMap, key: NameKey, line: u32) {
    map.entry(key).or_default().push(line);
}

fn collect_axis(
    tracks: &[GridTemplateComponent],
    names: &[Vec<Ident>],
    auto_repetitions: u16,
    lines: &mut LineMap,
    pairs: &mut Vec<(u32, Ident)>,
) {
    let mut current_line = 0u32;
    let mut track_iter = tracks.iter();
    for i in 0..=tracks.len() {
        current_line += 1;
        if let Some(name_set) = names.get(i) {
            for &name in name_set {
                pairs.push((current_line, name));
                upsert(lines, NameKey::plain(name), current_line);
            }
        }
        if let Some(GridTemplateComponent::Repeat(repeat)) = track_iter.next() {
            let count = match repeat.count {
                RepetitionCount::Count(c) => c,
                RepetitionCount::AutoFill | RepetitionCount::AutoFit => auto_repetitions,
            };
            let lines_per_repetition = repeat.tracks.len() as u32;
            assert!(
                repeat.line_names.is_empty() || repeat.line_names.len() as u32 == lines_per_repetition + 1,
                "repeat() needs no line name sets or track count + 1 of them"
            );
            for _ in 0..count {
                for (line, set) in (current_line..).zip(repeat.line_names.iter()) {
                    for &name in set {
                        pairs.push((line, name));
                        upsert(lines, NameKey::plain(name), line);
                    }
                }
                current_line += lines_per_repetition;
                if current_line > MAX_GRID_TRACKS as u32 {
                    break;
                }
            }
            if count > 0 {
                current_line = current_line.saturating_sub(1);
            }
        }
    }
}

impl NamedLineResolver {
    pub fn new<T: LayoutTree + ?Sized>(
        tree: &T,
        style: &GridContainerStyle,
        column_auto_repetitions: u16,
        row_auto_repetitions: u16,
    ) -> Self {
        let mut column_lines = LineMap::new();
        let mut row_lines = LineMap::new();
        let mut column_line_name_pairs = Vec::new();
        let mut row_line_name_pairs = Vec::new();
        collect_axis(
            &style.template_columns,
            &style.template_column_names,
            column_auto_repetitions,
            &mut column_lines,
            &mut column_line_name_pairs,
        );
        collect_axis(&style.template_rows, &style.template_row_names, row_auto_repetitions, &mut row_lines, &mut row_line_name_pairs);

        let (mut area_column_count, mut area_row_count) = (0, 0);
        if let Some(areas) = &style.template_areas {
            area_column_count = areas.column_count;
            area_row_count = areas.row_count;
            for area in &areas.areas {
                let key = |suffix| tree.suffixed_ident(area.name, suffix).map_or(NameKey::synthetic(area.name, suffix), NameKey::plain);
                let (start_key, end_key) = (key(IdentSuffix::Start), key(IdentSuffix::End));
                // Detailed info only lists real idents.
                if let Some(id) = tree.suffixed_ident(area.name, IdentSuffix::Start) {
                    column_line_name_pairs.push((area.column_start as u32, id));
                    row_line_name_pairs.push((area.row_start as u32, id));
                }
                if let Some(id) = tree.suffixed_ident(area.name, IdentSuffix::End) {
                    column_line_name_pairs.push((area.column_end as u32, id));
                    row_line_name_pairs.push((area.row_end as u32, id));
                }
                upsert(&mut column_lines, start_key, area.column_start as u32);
                upsert(&mut column_lines, end_key, area.column_end as u32);
                upsert(&mut row_lines, start_key, area.row_start as u32);
                upsert(&mut row_lines, end_key, area.row_end as u32);
            }
        }
        for lines in column_lines.values_mut().chain(row_lines.values_mut()) {
            lines.sort_unstable();
            lines.dedup();
        }
        Self {
            row_lines,
            column_lines,
            area_column_count,
            area_row_count,
            explicit_column_count: 0,
            explicit_row_count: 0,
            column_line_name_pairs,
            row_line_name_pairs,
        }
    }

    #[inline]
    pub fn area_column_count(&self) -> u16 {
        self.area_column_count
    }
    #[inline]
    pub fn area_row_count(&self) -> u16 {
        self.area_row_count
    }
    #[inline]
    pub fn set_explicit_column_count(&mut self, n: u16) {
        self.explicit_column_count = n;
    }
    #[inline]
    pub fn set_explicit_row_count(&mut self, n: u16) {
        self.explicit_row_count = n;
    }

    /// Names per explicit line (line 1 first), for detailed grid info.
    pub fn detailed_line_names(&self, axis: AbsoluteAxis) -> Vec<Vec<Ident>> {
        let (pairs, explicit) = match axis {
            AbsoluteAxis::Horizontal => (&self.column_line_name_pairs, self.explicit_column_count),
            AbsoluteAxis::Vertical => (&self.row_line_name_pairs, self.explicit_row_count),
        };
        if pairs.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<Vec<Ident>> = vec![Vec::new(); explicit as usize + 1];
        let mut sorted: Vec<&(u32, Ident)> = pairs.iter().collect();
        sorted.sort_by_key(|(line, _)| *line);
        for &&(line, name) in &sorted {
            if line >= 1
                && let Some(set) = out.get_mut(line as usize - 1)
                && !set.contains(&name)
            {
                set.push(name);
            }
        }
        out
    }

    pub fn resolve_column_names<T: LayoutTree + ?Sized>(&self, tree: &T, line: Line<GridPlacement>) -> Line<NonNamedPlacement> {
        Axis { tree, lines: &self.column_lines, explicit_track_count: self.explicit_column_count }.resolve(line)
    }
    pub fn resolve_row_names<T: LayoutTree + ?Sized>(&self, tree: &T, line: Line<GridPlacement>) -> Line<NonNamedPlacement> {
        Axis { tree, lines: &self.row_lines, explicit_track_count: self.explicit_row_count }.resolve(line)
    }
}

struct Axis<'a, T: ?Sized> {
    tree: &'a T,
    lines: &'a LineMap,
    explicit_track_count: u16,
}

/// A 1-based line from the name maps, as an origin-zero line.
#[inline]
fn oz_line(line: u32) -> i32 {
    line as i32 - 1
}

impl<T: LayoutTree + ?Sized> Axis<'_, T> {
    /// css-grid-2 §8.3: names resolve to lines; a named span counts lines from the other, definite edge.
    fn resolve(&self, line: Line<GridPlacement>) -> Line<NonNamedPlacement> {
        let start = match line.start {
            GridPlacement::NamedLine(name, idx) => GridPlacement::Line(self.find_named_line(name, idx, IdentSuffix::Start)),
            other => other,
        };
        let end = match line.end {
            GridPlacement::NamedLine(name, idx) => GridPlacement::Line(self.find_named_line(name, idx, IdentSuffix::End)),
            other => other,
        };
        match (start, end) {
            (GridPlacement::Line(start_line), GridPlacement::NamedSpan(name, idx)) if start_line != 0 => {
                let after_start = self.into_origin_zero(start_line) + 1;
                let end_line = self.nth_line_forwards(self.lines_named(name), after_start, idx.max(1) as i32);
                Line { start: NonNamedPlacement::Line(start_line), end: NonNamedPlacement::Line(self.into_grid_line(end_line)) }
            }
            (GridPlacement::NamedSpan(name, idx), GridPlacement::Line(end_line)) if end_line != 0 => {
                let before_end = self.into_origin_zero(end_line) - 1;
                let start_line = self.nth_line_backwards(self.lines_named(name), before_end, idx.max(1) as i32);
                Line { start: NonNamedPlacement::Line(self.into_grid_line(start_line)), end: NonNamedPlacement::Line(end_line) }
            }
            (s, e) => Line { start: NonNamedPlacement::from_style(s), end: NonNamedPlacement::from_style(e) },
        }
    }

    /// The line `<integer>? <custom-ident>` names; an index of 0 means none was given, in which case
    /// the edge of a named area (`<custom-ident>-start` / `-end`) takes priority.
    fn find_named_line(&self, name: Ident, idx: i16, edge: IdentSuffix) -> i16 {
        if idx == 0
            && let Some(&first) = self.lines.get(&self.edge_key(name, edge)).and_then(|lines| lines.first())
        {
            return self.into_grid_line(oz_line(first));
        }
        let lines = self.lines_named(name);
        let line = if idx >= 0 {
            self.nth_line_forwards(lines, 0, (idx as i32).max(1))
        } else {
            self.nth_line_backwards(lines, self.explicit_track_count as i32, -(idx as i32))
        };
        self.into_grid_line(line)
    }

    /// The key of an area's `<name>-start` / `-end` lines: those named so explicitly plus the area's own edge.
    #[inline]
    fn edge_key(&self, name: Ident, edge: IdentSuffix) -> NameKey {
        self.tree.suffixed_ident(name, edge).map_or(NameKey::synthetic(name, edge), NameKey::plain)
    }

    /// The sorted 1-based lines called `name` (a `<area>-start` / `-end` name includes the area's edge).
    #[inline]
    fn lines_named(&self, name: Ident) -> &[u32] {
        self.lines.get(&NameKey::plain(name)).map_or(&[], Vec::as_slice)
    }

    /// The `n`th of `lines` at or after origin-zero `first`; when too few, every implicit line after
    /// the explicit grid counts as named.
    fn nth_line_forwards(&self, lines: &[u32], first: i32, n: i32) -> i32 {
        let first = first.max(0);
        let lines = &lines[lines.partition_point(|&l| oz_line(l) < first)..];
        match lines.get(n as usize - 1) {
            Some(&l) => oz_line(l),
            None => first.max(self.explicit_track_count as i32 + 1) + (n - lines.len() as i32) - 1,
        }
    }

    /// The `n`th of `lines` at or before origin-zero `last`, counting backwards; when too few, every
    /// implicit line before the explicit grid counts as named.
    fn nth_line_backwards(&self, lines: &[u32], last: i32, n: i32) -> i32 {
        let last = last.min(self.explicit_track_count as i32);
        let lines = &lines[..lines.partition_point(|&l| oz_line(l) <= last)];
        match lines.len().checked_sub(n as usize) {
            Some(index) => oz_line(lines[index]),
            None => last.min(-1) - (n - lines.len() as i32) + 1,
        }
    }

    #[inline]
    fn into_origin_zero(&self, line: i16) -> i32 {
        super::coordinates::line_into_origin_zero(line, self.explicit_track_count).0 as i32
    }

    /// A (clamped) origin-zero line as a 1-based or negative CSS line.
    #[inline]
    fn into_grid_line(&self, oz: i32) -> i16 {
        let oz = oz.clamp(MIN_OZ_LINE as i32, MAX_OZ_LINE as i32) as i16;
        if oz >= 0 { oz + 1 } else { oz - (self.explicit_track_count as i16 + 1) }
    }
}
