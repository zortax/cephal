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

/// Each word is built in a register and stored whole, so comparing a key just built reads back
/// whole stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheKey {
    /// Known dimension bits, width low and height high; zero when unknown (see the flags).
    known: u64,
    /// Available space encoding, width low: definite bits, `+inf` for max-content, `-inf` for
    /// min-content.
    avail: u64,
    /// Parent size bits, width low; `+inf` for `None`.
    parent: u64,
    /// The context key low, the flags high.
    rest: u64,
}

#[inline(always)]
fn pack(low: u32, high: u32) -> u64 {
    low as u64 | (high as u64) << 32
}

#[inline(always)]
fn low(w: u64) -> u32 {
    w as u32
}

#[inline(always)]
fn high(w: u64) -> u32 {
    (w >> 32) as u32
}

const REST_AXIS_MASK: u64 = (AXIS_MASK as u64) << 32;

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
    /// Whether the question was asked inside a shared block formatting context with floats, whose
    /// answer depends on float state the key only versions within one layout pass.
    #[inline(always)]
    pub fn depends_on_floats(&self) -> bool {
        low(self.rest) != 0
    }

    /// A placeholder for unused slots.
    pub const EMPTY: Self = Self { known: 0, avail: 0, parent: 0, rest: 0 };

    #[inline]
    pub fn from_input(i: &LayoutInput) -> Self {
        let mut flags = 0u16;
        let known = pack(
            i.known_dimensions.width.map_or(0, |v| {
                flags |= KD_KNOWN_W;
                v.to_bits()
            }),
            i.known_dimensions.height.map_or(0, |v| {
                flags |= KD_KNOWN_H;
                v.to_bits()
            }),
        );
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
            avail: pack(avail_bits(i.available_space.width), avail_bits(i.available_space.height)),
            parent: pack(
                i.parent_size.width.map_or(f32::INFINITY.to_bits(), f32::to_bits),
                i.parent_size.height.map_or(f32::INFINITY.to_bits(), f32::to_bits),
            ),
            rest: pack(i.context_key, flags as u32),
        }
    }

    #[inline(always)]
    fn flags(&self) -> u16 {
        high(self.rest) as u16
    }

    /// Whether the context and flags match, apart from the axis, where a `Both` entry covers
    /// single-axis requests.
    #[inline(always)]
    fn rest_answers(&self, query: &Self) -> bool {
        (self.rest ^ query.rest) & !REST_AXIS_MASK == 0
            && (self.rest & REST_AXIS_MASK == REST_AXIS_MASK || self.rest & REST_AXIS_MASK == query.rest & REST_AXIS_MASK)
    }

    /// Whether an entry stored under `self` answers `query`: identical apart from the axis,
    /// where a `Both` entry covers single-axis requests.
    #[inline]
    pub fn answers(&self, query: &Self) -> bool {
        self.known == query.known && self.avail == query.avail && self.parent == query.parent && self.rest_answers(query)
    }

    /// Like [`Self::answers`], also when the available spaces differ only between min-content and
    /// max-content on axes outside `observed` (bit 0 width, bit 1 height).
    #[inline]
    pub fn answers_unobserved(&self, query: &Self, observed: u8) -> bool {
        let intrinsic = |b: u32| b == f32::INFINITY.to_bits() || b == f32::NEG_INFINITY.to_bits();
        let avail = |i: u32, a: u32, b: u32| a == b || (observed & (1 << i) == 0 && intrinsic(a) && intrinsic(b));
        self.known == query.known
            && (self.avail == query.avail
                || (avail(0, low(self.avail), low(query.avail)) && avail(1, high(self.avail), high(query.avail))))
            && self.parent == query.parent
            && self.rest_answers(query)
    }

    /// Like [`Self::answers`], ignoring available space on axes the query fixes.
    #[cfg(feature = "stats")]
    pub fn answers_relaxed(&self, query: &Self) -> bool {
        let f = query.flags();
        self.known == query.known
            && (f & KD_KNOWN_W != 0 || low(self.avail) == low(query.avail))
            && (f & KD_KNOWN_H != 0 || high(self.avail) == high(query.avail))
            && self.parent == query.parent
            && self.rest_answers(query)
    }

    pub fn into_input(self, run_mode: RunMode) -> LayoutInput {
        let f = self.flags();
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
                width: (f & KD_KNOWN_W != 0).then(|| f32::from_bits(low(self.known))),
                height: (f & KD_KNOWN_H != 0).then(|| f32::from_bits(high(self.known))),
            },
            known_dimensions_are_definite: Size { width: f & KD_DEFINITE_W != 0, height: f & KD_DEFINITE_H != 0 },
            parent_size: Size {
                width: (low(self.parent) != inf).then(|| f32::from_bits(low(self.parent))),
                height: (high(self.parent) != inf).then(|| f32::from_bits(high(self.parent))),
            },
            available_space: Size { width: avail_from_bits(low(self.avail)), height: avail_from_bits(high(self.avail)) },
            vertical_margins_are_collapsible: Line { start: f & MARGIN_START != 0, end: f & MARGIN_END != 0 },
            context_key: low(self.rest),
        }
    }
}
