//! One 8-byte word for every CSS length-like value.
//!
//! Low 8 bits hold the tag; the high 32 bits hold an `f32` payload for numeric variants and a
//! calc handle for `calc()`. Plain data: hashable, comparable, thread-safe.

use super::calc::CalcId;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(transparent)]
pub struct Length(u64);

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tag {
    Auto = 0,
    Length = 1,
    Percent = 2,
    Calc = 3,
    Fr = 4,
    MinContent = 5,
    MaxContent = 6,
    FitContent = 7,
    FitContentPx = 8,
    FitContentPercent = 9,
    Stretch = 10,
    Content = 11,
}

/// A decoded [`Length`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LengthKind {
    Auto,
    Length(f32),
    /// Fraction in `0..=1`.
    Percent(f32),
    Calc(CalcId),
    Fr(f32),
    MinContent,
    MaxContent,
    FitContent,
    FitContentPx(f32),
    FitContentPercent(f32),
    Stretch,
    Content,
}

impl Length {
    #[inline]
    const fn keyword(tag: Tag) -> Self {
        Self(tag as u64)
    }
    #[inline]
    const fn numeric(tag: Tag, value: f32) -> Self {
        Self(((value.to_bits() as u64) << 32) | tag as u64)
    }

    pub const AUTO: Self = Self::keyword(Tag::Auto);
    pub const ZERO: Self = Self::numeric(Tag::Length, 0.0);
    pub const MIN_CONTENT: Self = Self::keyword(Tag::MinContent);
    pub const MAX_CONTENT: Self = Self::keyword(Tag::MaxContent);
    pub const FIT_CONTENT: Self = Self::keyword(Tag::FitContent);
    pub const STRETCH: Self = Self::keyword(Tag::Stretch);
    pub const CONTENT: Self = Self::keyword(Tag::Content);

    #[inline]
    pub const fn length(px: f32) -> Self {
        Self::numeric(Tag::Length, px)
    }
    #[inline]
    pub const fn percent(fraction: f32) -> Self {
        Self::numeric(Tag::Percent, fraction)
    }
    #[inline]
    pub const fn fr(fraction: f32) -> Self {
        Self::numeric(Tag::Fr, fraction)
    }
    #[inline]
    pub const fn fit_content_px(px: f32) -> Self {
        Self::numeric(Tag::FitContentPx, px)
    }
    #[inline]
    pub const fn fit_content_percent(fraction: f32) -> Self {
        Self::numeric(Tag::FitContentPercent, fraction)
    }
    #[inline]
    pub const fn calc(id: CalcId) -> Self {
        Self(((id.0 as u64) << 32) | Tag::Calc as u64)
    }

    #[inline]
    pub(crate) fn tag(self) -> Tag {
        // Every constructor writes a valid tag.
        unsafe { core::mem::transmute((self.0 & 0xFF) as u8) }
    }
    #[inline]
    pub fn value(self) -> f32 {
        f32::from_bits((self.0 >> 32) as u32)
    }
    #[inline]
    pub fn calc_id(self) -> CalcId {
        CalcId((self.0 >> 32) as u32)
    }

    #[inline]
    pub fn kind(self) -> LengthKind {
        match self.tag() {
            Tag::Auto => LengthKind::Auto,
            Tag::Length => LengthKind::Length(self.value()),
            Tag::Percent => LengthKind::Percent(self.value()),
            Tag::Calc => LengthKind::Calc(self.calc_id()),
            Tag::Fr => LengthKind::Fr(self.value()),
            Tag::MinContent => LengthKind::MinContent,
            Tag::MaxContent => LengthKind::MaxContent,
            Tag::FitContent => LengthKind::FitContent,
            Tag::FitContentPx => LengthKind::FitContentPx(self.value()),
            Tag::FitContentPercent => LengthKind::FitContentPercent(self.value()),
            Tag::Stretch => LengthKind::Stretch,
            Tag::Content => LengthKind::Content,
        }
    }

    #[inline]
    pub fn is_auto(self) -> bool {
        self.tag() == Tag::Auto
    }
    #[inline]
    pub fn is_length(self) -> bool {
        self.tag() == Tag::Length
    }
    #[inline]
    pub fn is_percent(self) -> bool {
        self.tag() == Tag::Percent
    }
    #[inline]
    pub fn is_calc(self) -> bool {
        self.tag() == Tag::Calc
    }
    #[inline]
    pub fn is_fr(self) -> bool {
        self.tag() == Tag::Fr
    }
    #[inline]
    pub fn is_min_content(self) -> bool {
        self.tag() == Tag::MinContent
    }
    #[inline]
    pub fn is_max_content(self) -> bool {
        self.tag() == Tag::MaxContent
    }
    #[inline]
    pub fn is_stretch(self) -> bool {
        self.tag() == Tag::Stretch
    }
    #[inline]
    pub fn is_content(self) -> bool {
        self.tag() == Tag::Content
    }
    /// `min-content`, `max-content`, `fit-content*` or `stretch`.
    #[inline]
    pub fn is_sizing_keyword(self) -> bool {
        matches!(
            self.tag(),
            Tag::MinContent | Tag::MaxContent | Tag::FitContent | Tag::FitContentPx | Tag::FitContentPercent | Tag::Stretch
        )
    }
    /// `min-content`, `max-content` or `fit-content*`.
    #[inline]
    pub fn is_intrinsic(self) -> bool {
        matches!(self.tag(), Tag::MinContent | Tag::MaxContent | Tag::FitContent | Tag::FitContentPx | Tag::FitContentPercent)
    }
    /// `max-content` or `fit-content*`, which behave like `max-content` for track sizing.
    #[inline]
    pub fn is_max_content_alike(self) -> bool {
        matches!(self.tag(), Tag::MaxContent | Tag::FitContent | Tag::FitContentPx | Tag::FitContentPercent)
    }
    #[inline]
    pub fn is_fit_content(self) -> bool {
        matches!(self.tag(), Tag::FitContent | Tag::FitContentPx | Tag::FitContentPercent)
    }
    /// Depends on the percentage basis (`%` or `calc()`).
    #[inline]
    pub fn uses_percentage(self) -> bool {
        matches!(self.tag(), Tag::Percent | Tag::Calc | Tag::FitContentPercent)
    }

    /// Resolves a length or percentage; anything else is `None`.
    #[inline]
    pub fn resolve(self, basis: Option<f32>, calc: &impl Fn(CalcId, f32) -> f32) -> Option<f32> {
        match self.tag() {
            Tag::Length => Some(self.value()),
            Tag::Percent => basis.map(|b| b * self.value()),
            Tag::Calc => basis.map(|b| calc(self.calc_id(), b)),
            _ => None,
        }
    }

    /// Like [`Length::resolve`], with `0` for unresolvable values.
    #[inline]
    pub fn resolve_or_zero(self, basis: Option<f32>, calc: &impl Fn(CalcId, f32) -> f32) -> f32 {
        self.resolve(basis, calc).unwrap_or(0.0)
    }

    /// Resolves against a definite basis; `%` with no basis is `None`.
    #[inline]
    pub fn resolved_percentage_size(self, basis: f32, calc: &impl Fn(CalcId, f32) -> f32) -> Option<f32> {
        match self.tag() {
            Tag::Percent => Some(basis * self.value()),
            Tag::Calc => Some(calc(self.calc_id(), basis)),
            _ => None,
        }
    }
}

impl core::fmt::Debug for Length {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.kind().fmt(f)
    }
}

impl Default for Length {
    fn default() -> Self {
        Self::AUTO
    }
}
