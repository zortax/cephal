//! §9.3 collecting items into lines.

use crate::compute::scratch::Scratch;
use super::axis::FlexAxisSize;
use super::{AlgoConstants, FlexItem, FlexLine};
use crate::geometry::{AvailableSpace, MaybeMath, Size};

fn single_line(items: &[FlexItem]) -> Scratch<FlexLine> {
    Scratch::collect([FlexLine { start: 0, end: items.len(), cross_size: 0.0, offset_cross: 0.0 }])
}

fn one_item_per_line(items: &[FlexItem]) -> Scratch<FlexLine> {
    Scratch::collect((0..items.len()).map(|i| FlexLine { start: i, end: i + 1, cross_size: 0.0, offset_cross: 0.0 }))
}

/// Available main space for line breaking, honouring the container's min/max size.
fn main_axis_available_space(c: &AlgoConstants, available_space: Size<AvailableSpace>) -> AvailableSpace {
    match c.max_size.main(c.dir) {
        Some(max_size) => AvailableSpace::Definite({
            let available = available_space.main(c.dir).into_option().unwrap_or(max_size);
            let available = if c.has_definite_main_size { available } else { available.min(max_size) };
            available.maybe_max(c.min_size.main(c.dir))
        }),
        // A column's automatic main size is content based, so ancestor space does not wrap it.
        None if !c.dir.is_row() && !c.has_definite_main_size && available_space.main(c.dir).is_definite() => {
            AvailableSpace::MaxContent
        }
        None => available_space.main(c.dir),
    }
}

pub(super) fn collect_flex_lines(c: &AlgoConstants, available_space: Size<AvailableSpace>, items: &[FlexItem]) -> Scratch<FlexLine> {
    if !c.is_wrap || !c.known_main_size_is_definite {
        return single_line(items);
    }
    match main_axis_available_space(c, available_space) {
        AvailableSpace::MaxContent => single_line(items),
        AvailableSpace::MinContent => one_item_per_line(items),
        AvailableSpace::Definite(limit) => {
            let mut lines = Scratch::with_capacity(1);
            let gap = c.gap.main(c.dir);
            let mut start = 0;
            while start < items.len() {
                let mut line_length = 0.0;
                let mut end = items.len();
                for (idx, child) in items[start..].iter().enumerate() {
                    line_length += child.hypothetical_outer_size.main(c.dir) + if idx == 0 { 0.0 } else { gap };
                    if line_length > limit && idx != 0 {
                        end = start + idx;
                        break;
                    }
                }
                lines.push(FlexLine { start, end, cross_size: 0.0, offset_cross: 0.0 });
                start = end;
            }
            lines
        }
    }
}

pub(super) fn collect_balanced_flex_lines(
    c: &AlgoConstants,
    available_space: Size<AvailableSpace>,
    items: &[FlexItem],
) -> Scratch<FlexLine> {
    if items.is_empty() {
        return Scratch::take();
    }
    let space = if c.known_main_size_is_definite {
        main_axis_available_space(c, available_space)
    } else {
        AvailableSpace::MaxContent
    };
    if space == AvailableSpace::MinContent {
        return one_item_per_line(items);
    }
    let limit = space.into_option().unwrap_or(f32::INFINITY);
    let min_line_count = c.line_count.unwrap_or(1) as usize;
    let counts = super::balance::balanced_line_item_counts(
        items.iter().map(|item| item.hypothetical_outer_size.main(c.dir)),
        limit,
        c.gap.main(c.dir),
        min_line_count,
    );
    let mut lines = Scratch::with_capacity(counts.len());
    let mut start = 0;
    for count in counts {
        lines.push(FlexLine { start, end: start + count, cross_size: 0.0, offset_cross: 0.0 });
        start += count;
    }
    debug_assert_eq!(start, items.len());
    lines
}
