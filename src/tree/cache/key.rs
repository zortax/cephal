//! A packed, reversible encoding of [`LayoutInput`].

use crate::geometry::{AvailableSpace, Line, Size};
use crate::tree::{LayoutInput, RequestedAxis, RunMode, SizingMode};

const KD_KNOWN_W: u16 = 1 << 0;
const KD_KNOWN_H: u16 = 1 << 1;
const KD_DEFINITE_W: u16 = 1 << 2;
const KD_DEFINITE_H: u16 = 1 << 3;
const MARGIN_START: u16 = 1 << 4;
const MARGIN_END: u16 = 1 << 5;
const SIZING_CONTENT: u16 = 1 << 6;
const AXIS_SHIFT: u16 = 7;
const AXIS_MASK: u16 = 0b11 << AXIS_SHIFT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheKey {
    /// Known dimension bits; zero when unknown (see the flags).
    known: [u32; 2],
    /// Available space encoding: definite bits, `+inf` for max-content, `-inf` for min-content.
    avail: [u32; 2],
    /// Parent size bits; `+inf` for `None`.
    parent: [u32; 2],
    context: u32,
    flags: u16,
}

#[inline]
fn avail_bits(a: AvailableSpace) -> u32 {
    match a {
        AvailableSpace::Definite(v) => v.to_bits(),
        AvailableSpace::MinContent => f32::NEG_INFINITY.to_bits(),
        AvailableSpace::MaxContent => f32::INFINITY.to_bits(),
    }
}

#[inline]
fn avail_from_bits(b: u32) -> AvailableSpace {
    let v = f32::from_bits(b);
    if v == f32::INFINITY {
        AvailableSpace::MaxContent
    } else if v == f32::NEG_INFINITY {
        AvailableSpace::MinContent
    } else {
        AvailableSpace::Definite(v)
    }
}

impl CacheKey {
    pub fn from_input(i: &LayoutInput) -> Self {
        let mut flags = 0u16;
        let known = [
            i.known_dimensions.width.map_or(0, |v| {
                flags |= KD_KNOWN_W;
                v.to_bits()
            }),
            i.known_dimensions.height.map_or(0, |v| {
                flags |= KD_KNOWN_H;
                v.to_bits()
            }),
        ];
        // Definiteness only matters for known dimensions.
        if i.known_dimensions_are_definite.width || i.known_dimensions.width.is_none() {
            flags |= KD_DEFINITE_W;
        }
        if i.known_dimensions_are_definite.height || i.known_dimensions.height.is_none() {
            flags |= KD_DEFINITE_H;
        }
        if i.vertical_margins_are_collapsible.start {
            flags |= MARGIN_START;
        }
        if i.vertical_margins_are_collapsible.end {
            flags |= MARGIN_END;
        }
        if i.sizing_mode == SizingMode::ContentSize {
            flags |= SIZING_CONTENT;
        }
        let axis = match i.axis {
            RequestedAxis::Horizontal => 1,
            RequestedAxis::Vertical => 2,
            RequestedAxis::Both => 3,
        };
        flags |= axis << AXIS_SHIFT;
        Self {
            known,
            avail: [avail_bits(i.available_space.width), avail_bits(i.available_space.height)],
            parent: [
                i.parent_size.width.map_or(f32::INFINITY.to_bits(), f32::to_bits),
                i.parent_size.height.map_or(f32::INFINITY.to_bits(), f32::to_bits),
            ],
            context: i.context_key,
            flags,
        }
    }

    /// Whether an entry stored under `self` answers `query`: identical apart from the axis,
    /// where a `Both` entry covers single-axis requests.
    #[inline]
    pub fn answers(&self, query: &Self) -> bool {
        self.known == query.known
            && self.avail == query.avail
            && self.parent == query.parent
            && self.context == query.context
            && (self.flags & !AXIS_MASK) == (query.flags & !AXIS_MASK)
            && (self.flags & AXIS_MASK == AXIS_MASK || self.flags & AXIS_MASK == query.flags & AXIS_MASK)
    }

    /// Like [`Self::answers`], ignoring available space on axes the query fixes.
    #[cfg(feature = "stats")]
    pub fn answers_relaxed(&self, query: &Self) -> bool {
        let f = query.flags;
        self.known == query.known
            && (f & KD_KNOWN_W != 0 || self.avail[0] == query.avail[0])
            && (f & KD_KNOWN_H != 0 || self.avail[1] == query.avail[1])
            && self.parent == query.parent
            && self.context == query.context
            && (self.flags & !AXIS_MASK) == (query.flags & !AXIS_MASK)
            && (self.flags & AXIS_MASK == AXIS_MASK || self.flags & AXIS_MASK == query.flags & AXIS_MASK)
    }

    pub fn into_input(self, run_mode: RunMode) -> LayoutInput {
        let f = self.flags;
        let inf = f32::INFINITY.to_bits();
        LayoutInput {
            run_mode,
            sizing_mode: if f & SIZING_CONTENT != 0 { SizingMode::ContentSize } else { SizingMode::InherentSize },
            axis: match (f & AXIS_MASK) >> AXIS_SHIFT {
                1 => RequestedAxis::Horizontal,
                2 => RequestedAxis::Vertical,
                _ => RequestedAxis::Both,
            },
            known_dimensions: Size {
                width: (f & KD_KNOWN_W != 0).then(|| f32::from_bits(self.known[0])),
                height: (f & KD_KNOWN_H != 0).then(|| f32::from_bits(self.known[1])),
            },
            known_dimensions_are_definite: Size { width: f & KD_DEFINITE_W != 0, height: f & KD_DEFINITE_H != 0 },
            parent_size: Size {
                width: (self.parent[0] != inf).then(|| f32::from_bits(self.parent[0])),
                height: (self.parent[1] != inf).then(|| f32::from_bits(self.parent[1])),
            },
            available_space: Size { width: avail_from_bits(self.avail[0]), height: avail_from_bits(self.avail[1]) },
            vertical_margins_are_collapsible: Line { start: f & MARGIN_START != 0, end: f & MARGIN_END != 0 },
            context_key: self.context,
        }
    }
}
