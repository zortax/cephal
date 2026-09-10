//! Resolving named lines and areas to line numbers.

use super::coordinates::{MAX_GRID_TRACKS, NonNamedPlacement};
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

impl<T: LayoutTree + ?Sized> Axis<'_, T> {
    fn resolve(&self, line: Line<GridPlacement>) -> Line<NonNamedPlacement> {
        let start = match line.start {
            GridPlacement::NamedLine(name, idx) => {
                GridPlacement::Line(self.find_line_index(name, idx as i32, IdentSuffix::Start, &|l| l))
            }
            other => other,
        };
        let end = match line.end {
            GridPlacement::NamedLine(name, idx) => {
                GridPlacement::Line(self.find_line_index(name, idx as i32, IdentSuffix::End, &|l| l))
            }
            other => other,
        };
        match (start, end) {
            (GridPlacement::Line(start_line), GridPlacement::NamedSpan(name, idx)) => {
                let normalized = if start_line > 0 {
                    start_line as u32
                } else {
                    (self.explicit_track_count as i32 + 1 + start_line as i32).max(0) as u32
                };
                let end_line = self.find_line_index(name, idx as i32, IdentSuffix::End, &|lines| {
                    let p = lines.partition_point(|l| *l <= normalized);
                    &lines[p..]
                });
                Line { start: NonNamedPlacement::Line(start_line), end: NonNamedPlacement::Line(end_line) }
            }
            (GridPlacement::NamedSpan(name, idx), GridPlacement::Line(end_line)) => {
                let normalized = if end_line > 0 {
                    end_line as u32
                } else {
                    (self.explicit_track_count as i32 + 1 + end_line as i32).max(0) as u32
                };
                let start_line = self.find_line_index(name, idx as i32, IdentSuffix::Start, &|lines| {
                    let p = lines.partition_point(|l| *l < normalized);
                    &lines[..p]
                });
                Line { start: NonNamedPlacement::Line(start_line), end: NonNamedPlacement::Line(end_line) }
            }
            (s, e) => Line { start: NonNamedPlacement::from_style(s), end: NonNamedPlacement::from_style(e) },
        }
    }

    fn find_line_index(&self, name: Ident, idx: i32, end: IdentSuffix, filter: &dyn Fn(&[u32]) -> &[u32]) -> i16 {
        let idx = if idx == 0 { 1 } else { idx };
        let explicit = self.explicit_track_count as i32;
        fn get_line(lines: &[u32], explicit: i32, idx: i32) -> i16 {
            let abs = idx.unsigned_abs() as usize;
            let line = if abs <= lines.len() {
                if idx > 0 { lines[abs - 1] as i64 } else { lines[lines.len() - abs] as i64 }
            } else {
                let remaining = (abs - lines.len()) as i64 * idx.signum() as i64;
                if idx > 0 { explicit as i64 + 1 + remaining } else { -(explicit as i64 + 1 + remaining) }
            };
            line.clamp(i16::MIN as i64, i16::MAX as i64) as i16
        }
        if let Some(lines) = self.lines.get(&NameKey::plain(name)) {
            return get_line(filter(lines), explicit, idx);
        }
        let implicit_key =
            self.tree.suffixed_ident(name, end).map_or(NameKey::synthetic(name, end), NameKey::plain);
        if let Some(lines) = self.lines.get(&implicit_key) {
            return get_line(filter(lines), explicit, idx);
        }
        let line = if idx > 0 { explicit as i64 + 1 + idx as i64 } else { -(explicit as i64 + 1 + idx as i64) };
        line.clamp(i16::MIN as i64, i16::MAX as i64) as i16
    }
}
