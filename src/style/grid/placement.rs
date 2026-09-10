//! Grid line placement.

use crate::style::Ident;

/// A 1-based grid line; negative counts from the end.
pub type GridLine = i16;

/// A line index where the first explicit line is 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OriginZeroLine(pub i16);

impl OriginZeroLine {
    /// Converts to a 0-based track index once the implicit grid's negative extent is known.
    #[inline]
    pub fn into_track_vec_index(self, negative_implicit_tracks: u16) -> usize {
        (self.0 + negative_implicit_tracks as i16) as usize
    }
    /// Lines above the explicit grid count as implicit tracks past its end.
    #[inline]
    pub fn implied_explicit_track_count(self, explicit_track_count: u16) -> u16 {
        if self.0 > 0 { (self.0 as u16).saturating_sub(explicit_track_count) } else { 0 }
    }
    #[inline]
    pub fn implied_negative_implicit_tracks(self) -> u16 {
        if self.0 < 0 { (-self.0) as u16 } else { 0 }
    }
    #[inline]
    pub fn implied_positive_implicit_tracks(self, explicit_track_count: u16) -> u16 {
        if self.0 > explicit_track_count as i16 { self.0 as u16 - explicit_track_count } else { 0 }
    }
}

impl core::ops::Add<u16> for OriginZeroLine {
    type Output = Self;
    fn add(self, rhs: u16) -> Self {
        Self(self.0 + rhs as i16)
    }
}
impl core::ops::Sub<u16> for OriginZeroLine {
    type Output = Self;
    fn sub(self, rhs: u16) -> Self {
        Self(self.0 - rhs as i16)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GridPlacement {
    #[default]
    Auto,
    Line(GridLine),
    NamedLine(Ident, GridLine),
    Span(u16),
    NamedSpan(Ident, u16),
}

impl GridPlacement {
    pub const AUTO: Self = Self::Auto;
    #[inline]
    pub const fn from_line_index(i: GridLine) -> Self {
        Self::Line(i)
    }
    #[inline]
    pub const fn from_span(n: u16) -> Self {
        Self::Span(n)
    }
    #[inline]
    pub fn is_definite(self) -> bool {
        matches!(self, Self::Line(_) | Self::NamedLine(..))
    }
}
