//! Float placement within a block formatting context (CSS 2.2 §9.5).

use crate::geometry::{AvailableSpace, Point, Size};
use crate::style::{Clear, Direction, Float};
use core::ops::Range;

pub(crate) const FIT_TOLERANCE: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FloatDirection {
    Left = 0,
    Right = 1,
}

impl Float {
    #[inline]
    pub fn direction(self) -> Option<FloatDirection> {
        match self {
            Float::Left => Some(FloatDirection::Left),
            Float::Right => Some(FloatDirection::Right),
            Float::None => None,
        }
    }
}

/// Space a block box can occupy beside floats.
#[derive(Clone, Copy, Debug, Default)]
pub struct BfcSlot {
    pub segment_id: Option<usize>,
    pub x: f32,
    pub y: f32,
    pub border_width: f32,
    pub stretch_width: f32,
}

#[derive(Clone, Debug, Default)]
pub struct PlacedFloatedBox {
    pub width: f32,
    pub height: f32,
    pub x_inset: f32,
    pub y: f32,
}

/// A vertical band with constant float insets.
#[derive(Clone, Debug)]
struct Segment {
    y: Range<f32>,
    insets: [f32; 2],
    has_float: [bool; 2],
}

fn float_fits_horizontally(width: f32, direction: FloatDirection, bfc_width: f32, float_insets: [f32; 2], cb_insets: [f32; 2]) -> bool {
    let lead = direction as usize;
    let trail = 1 - lead;
    let x_inset = float_insets[lead].max(cb_insets[lead]);
    let fits_opposite = float_insets[trail] == 0.0 || x_inset + width <= bfc_width - float_insets[trail] + FIT_TOLERANCE;
    let fits_cb = float_insets[lead] == 0.0 || x_inset + width <= bfc_width - cb_insets[trail] + FIT_TOLERANCE;
    fits_opposite && fits_cb
}

struct FloatFitter {
    bfc_width: f32,
    slot_height: f64,
    float_insets: [f32; 2],
    cb_insets: [f32; 2],
}

impl FloatFitter {
    fn union_insets(&mut self, insets: [f32; 2]) {
        self.float_insets[0] = self.float_insets[0].max(insets[0]);
        self.float_insets[1] = self.float_insets[1].max(insets[1]);
    }
    fn placed_inset(&self, direction: FloatDirection) -> f32 {
        let lead = direction as usize;
        self.float_insets[lead].max(self.cb_insets[lead])
    }
    fn fits_horizontally(&self, width: f32, direction: FloatDirection) -> bool {
        float_fits_horizontally(width, direction, self.bfc_width, self.float_insets, self.cb_insets)
    }
}

/// All floats of one BFC, in BFC coordinates.
#[derive(Clone, Debug, Default)]
pub struct FloatContext {
    available_width: f32,
    has_floats: bool,
    segments: Vec<Segment>,
    last_placed_floats: [Range<usize>; 2],
    clear_bottoms: [Option<f32>; 2],
    float_ceiling: Option<f32>,
    /// Bumped on every placement; keys cache entries that depend on float state.
    version: u32,
}

impl FloatContext {
    #[inline]
    pub fn has_floats(&self) -> bool {
        self.has_floats
    }
    #[inline]
    pub fn version(&self) -> u32 {
        self.version
    }
    #[inline]
    pub fn has_active_floats(&self, min_y: f32) -> bool {
        self.has_floats && self.segments.last().is_some_and(|s| s.y.end > min_y)
    }
    #[inline]
    pub fn set_width(&mut self, available_width: f32) {
        self.available_width = available_width;
    }

    fn subdivide_segment(&mut self, idx: usize, divide_at_y: f32) {
        let old = &mut self.segments[idx];
        debug_assert!(old.y.contains(&divide_at_y) && old.y.start != divide_at_y);
        let new = Segment { insets: old.insets, has_float: old.has_float, y: divide_at_y..old.y.end };
        old.y.end = divide_at_y;
        self.segments.insert(idx + 1, new);
    }

    fn update_last_placed_float(&mut self, direction: FloatDirection, placement: Range<usize>) {
        let slot = direction as usize;
        self.last_placed_floats[slot].start = self.last_placed_floats[slot].start.max(placement.start);
        self.last_placed_floats[slot].end = self.last_placed_floats[slot].end.max(placement.end);
    }

    /// Places a margin box; returns its margin-box position.
    pub fn place_floated_box(
        &mut self,
        floated_box: Size<f32>,
        min_y: f32,
        cb_insets: [f32; 2],
        direction: FloatDirection,
        clear: Clear,
    ) -> Point<f32> {
        self.has_floats = true;
        self.version += 1;
        let placed = self.place_inner(floated_box, min_y, cb_insets, direction, clear);
        let slot = direction as usize;
        let bottom = placed.y + placed.height;
        self.clear_bottoms[slot] = Some(self.clear_bottoms[slot].map_or(bottom, |b| b.max(bottom)));
        self.float_ceiling = Some(self.float_ceiling.map_or(placed.y, |c| c.max(placed.y)));
        match direction {
            FloatDirection::Left => Point { x: placed.x_inset, y: placed.y },
            FloatDirection::Right => Point { x: self.available_width - placed.x_inset - floated_box.width, y: placed.y },
        }
    }

    fn place_inner(&mut self, floated_box: Size<f32>, min_y: f32, cb_insets: [f32; 2], direction: FloatDirection, clear: Clear) -> PlacedFloatedBox {
        let slot = direction as usize;
        let min_y = min_y
            .max(self.float_ceiling.unwrap_or(f32::NEG_INFINITY))
            .max(self.cleared_threshold(clear).unwrap_or(f32::NEG_INFINITY));
        let float_start = self.last_placed_floats[0].start.max(self.last_placed_floats[1].start);
        let hwm = match clear {
            Clear::Left => float_start.max(self.last_placed_floats[0].end + 1),
            Clear::Right => float_start.max(self.last_placed_floats[1].end + 1),
            Clear::Both => self.last_placed_floats[0].end.max(self.last_placed_floats[1].end) + 1,
            Clear::None => float_start,
        };
        let mut start_idx = self
            .segments
            .get(hwm..)
            .and_then(|s| s.iter().position(|seg| seg.y.end > min_y).map(|i| i + hwm))
            .unwrap_or(self.segments.len());
        let mut start_y = min_y;
        let mut end_idx = start_idx;
        let (start, end, placed_inset) = 'outer: loop {
            let Some(start_segment) = self.segments.get(start_idx) else {
                break (None, None, cb_insets[slot]);
            };
            if !float_fits_horizontally(floated_box.width, direction, self.available_width, start_segment.insets, cb_insets) {
                start_idx += 1;
                end_idx = end_idx.max(start_idx);
                continue;
            }
            start_y = start_y.max(start_segment.y.start);
            let mut fitter = FloatFitter {
                bfc_width: self.available_width,
                slot_height: (start_segment.y.end - start_y) as f64,
                float_insets: [0.0, 0.0],
                cb_insets,
            };
            fitter.union_insets(start_segment.insets);
            loop {
                let Some(end_segment) = self.segments.get(end_idx) else {
                    break 'outer (Some(start_idx), None, fitter.placed_inset(direction));
                };
                fitter.union_insets(end_segment.insets);
                if !fitter.fits_horizontally(floated_box.width, direction) {
                    start_idx += 1;
                    end_idx = end_idx.max(start_idx);
                    continue 'outer;
                }
                if end_idx != start_idx {
                    fitter.slot_height += (end_segment.y.end - end_segment.y.start) as f64;
                }
                if fitter.slot_height < floated_box.height as f64 {
                    end_idx += 1;
                    continue;
                }
                break 'outer (Some(start_idx), Some(end_idx), fitter.placed_inset(direction));
            }
        };
        if floated_box.height == 0.0 {
            return PlacedFloatedBox { width: floated_box.width, height: 0.0, y: start_y, x_inset: placed_inset };
        }
        let Some(mut start_idx) = start else {
            let last_y_end = self.segments.last().map_or(0.0, |s| s.y.end);
            if start_y > last_y_end {
                self.segments.push(Segment { y: last_y_end..start_y, insets: [0.0, 0.0], has_float: [false; 2] });
            }
            let start_y = last_y_end.max(start_y);
            let mut insets = cb_insets;
            insets[slot] += floated_box.width;
            let mut has_float = [false; 2];
            has_float[slot] = true;
            self.segments.push(Segment { y: start_y..(start_y + floated_box.height), insets, has_float });
            let idx = self.segments.len() - 1;
            self.update_last_placed_float(direction, idx..idx + 1);
            return PlacedFloatedBox { width: floated_box.width, height: floated_box.height, y: start_y, x_inset: cb_insets[slot] };
        };
        let mut end = end;
        if start_y != self.segments[start_idx].y.start {
            self.subdivide_segment(start_idx, start_y);
            start_idx += 1;
            if let Some(e) = end.as_mut() {
                *e += 1;
            }
        }
        let end_idx = match end {
            None => {
                let last_y_end = self.segments.last().map_or(0.0, |s| s.y.end);
                if min_y > last_y_end {
                    self.segments.push(Segment { y: last_y_end..min_y, insets: [0.0, 0.0], has_float: [false; 2] });
                }
                self.segments.len() - 1
            }
            Some(mut end_idx) => {
                let end_y = start_y + floated_box.height;
                while end_idx > start_idx && end_y <= self.segments[end_idx].y.start {
                    end_idx -= 1;
                }
                if self.segments[end_idx].y.start < end_y && end_y < self.segments[end_idx].y.end {
                    self.subdivide_segment(end_idx, end_y);
                }
                end_idx
            }
        };
        let inset_plus_width = placed_inset + floated_box.width;
        for seg in &mut self.segments[start_idx..=end_idx] {
            seg.insets[slot] = inset_plus_width;
            seg.has_float[slot] = true;
        }
        self.update_last_placed_float(direction, start_idx..end_idx + 1);
        PlacedFloatedBox { width: floated_box.width, height: floated_box.height, y: start_y, x_inset: placed_inset }
    }

    fn cleared_segment(&self, clear: Clear) -> Option<usize> {
        let left_end = self.last_placed_floats[0].end;
        let right_end = self.last_placed_floats[1].end;
        match clear {
            Clear::Left if left_end > 0 => Some(left_end),
            Clear::Right if right_end > 0 => Some(right_end),
            Clear::Both if left_end > 0 || right_end > 0 => Some(left_end.max(right_end)),
            _ => None,
        }
    }

    /// Lowest float bottom a cleared box must be placed below.
    pub fn cleared_threshold(&self, clear: Clear) -> Option<f32> {
        match clear {
            Clear::Left => self.clear_bottoms[0],
            Clear::Right => self.clear_bottoms[1],
            Clear::Both => match self.clear_bottoms {
                [Some(l), Some(r)] => Some(l.max(r)),
                [l, r] => l.or(r),
            },
            Clear::None => None,
        }
    }

    /// The highest slot at or below `min_y` (past `after`) where a block box can sit beside floats.
    pub fn find_bfc_slot(
        &self,
        min_y: f32,
        cb_insets: [f32; 2],
        margins: [f32; 2],
        direction: Direction,
        clear: Clear,
        after: Option<usize>,
    ) -> BfcSlot {
        let margin_insets = [cb_insets[0] + margins[0], cb_insets[1] + margins[1]];
        let no_float_width = self.available_width - margin_insets[0] - margin_insets[1];
        let no_float_slot = BfcSlot { segment_id: None, x: margin_insets[0], y: min_y, border_width: no_float_width, stretch_width: no_float_width };
        if !self.has_active_floats(min_y) {
            return no_float_slot;
        }
        let min_y = min_y.max(self.cleared_threshold(clear).unwrap_or(f32::NEG_INFINITY));
        let at_least = after.map_or(0, |i| i + 1);
        let hwm = at_least.max(self.cleared_segment(clear).map_or(0, |i| i + 1));
        let start_idx = self
            .segments
            .get(hwm..)
            .and_then(|s| s.iter().position(|seg| seg.y.end > min_y).map(|i| i + hwm))
            .unwrap_or(self.segments.len());
        match self.segments.get(start_idx) {
            Some(segment) => {
                let lead = if direction == Direction::Ltr { 0 } else { 1 };
                let trail = 1 - lead;
                let mut fit = [0.0; 2];
                let mut stretch = [0.0; 2];
                fit[lead] = if segment.has_float[lead] { segment.insets[lead].max(margin_insets[lead]) } else { margin_insets[lead] };
                stretch[lead] = fit[lead];
                fit[trail] = if segment.has_float[trail] {
                    segment.insets[trail].max(cb_insets[trail])
                } else {
                    margin_insets[trail].min(cb_insets[trail])
                };
                stretch[trail] = if segment.has_float[trail] { segment.insets[trail].max(margin_insets[trail]) } else { margin_insets[trail] };
                BfcSlot {
                    segment_id: Some(start_idx),
                    x: fit[0],
                    y: segment.y.start.max(min_y),
                    border_width: self.available_width - fit[0] - fit[1],
                    stretch_width: self.available_width - stretch[0] - stretch[1],
                }
            }
            None => BfcSlot { y: self.segments.last().map_or(min_y, |s| s.y.end).max(min_y), ..no_float_slot },
        }
    }
}

/// Intrinsic width contribution of floats, which stack horizontally until cleared.
pub struct FloatIntrinsicWidthCalculator {
    available_width: AvailableSpace,
    side_sums: [f32; 2],
    contribution: f32,
    widest: f32,
}

impl FloatIntrinsicWidthCalculator {
    pub fn new(available_width: AvailableSpace) -> Self {
        Self { available_width, side_sums: [0.0; 2], contribution: 0.0, widest: 0.0 }
    }
    pub fn add_float(&mut self, width: f32, direction: FloatDirection, clear: Clear) {
        match self.available_width {
            AvailableSpace::Definite(_) | AvailableSpace::MaxContent => {
                if matches!(clear, Clear::Left | Clear::Both) {
                    self.side_sums[0] = 0.0;
                }
                if matches!(clear, Clear::Right | Clear::Both) {
                    self.side_sums[1] = 0.0;
                }
                self.side_sums[direction as usize] += width;
                self.contribution = self.contribution.max(self.side_sums[0] + self.side_sums[1]);
            }
            AvailableSpace::MinContent => self.contribution = self.contribution.max(width),
        }
        self.widest = self.widest.max(width);
    }
    pub fn result(&self) -> f32 {
        match self.available_width {
            AvailableSpace::Definite(w) => self.contribution.min(w).max(self.widest),
            _ => self.contribution,
        }
    }
}
