//! `min-content`, `max-content`, `fit-content()` and `stretch` resolution.

use crate::geometry::AvailableSpace;
use crate::style::{Length, LengthKind};

/// How a sizing keyword resolves for one axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SizingKeywordResolution {
    /// Measure the box under this available space.
    Measure(AvailableSpace),
    /// Use this exact size.
    Exact(f32),
}

/// `None` when the keyword behaves like `auto` (no keyword, or a missing basis).
#[inline]
pub fn resolve_sizing_keyword(
    value: Length,
    stretch_size: Option<f32>,
    percent_basis: Option<f32>,
) -> Option<SizingKeywordResolution> {
    use SizingKeywordResolution::*;
    match value.kind() {
        LengthKind::MinContent => Some(Measure(AvailableSpace::MinContent)),
        LengthKind::MaxContent => Some(Measure(AvailableSpace::MaxContent)),
        LengthKind::FitContentPx(px) => Some(Measure(AvailableSpace::Definite(px))),
        LengthKind::FitContentPercent(f) => percent_basis.map(|b| Measure(AvailableSpace::Definite(b * f))),
        LengthKind::FitContent => stretch_size.map(|s| Measure(AvailableSpace::Definite(s))),
        LengthKind::Stretch => stretch_size.map(Exact),
        _ => None,
    }
}
