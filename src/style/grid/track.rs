//! Track sizing functions and template components.

use crate::style::Ident;
use crate::style::calc::CalcId;
use crate::style::length::Length;

macro_rules! track_fn {
    ($(#[$m:meta])* $name:ident { $($ctor:ident => $inner:ident),* $(,)? } consts { $($cname:ident => $cval:ident),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[repr(transparent)]
        pub struct $name(pub Length);

        impl $name {
            $( pub const $cname: Self = Self(Length::$cval); )*
            $(
                #[inline]
                pub const fn $ctor(v: f32) -> Self { Self(Length::$inner(v)) }
            )*
            #[inline]
            pub const fn calc(id: CalcId) -> Self { Self(Length::calc(id)) }
        }

        impl core::ops::Deref for $name {
            type Target = Length;
            #[inline]
            fn deref(&self) -> &Length { &self.0 }
        }

        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { self.0.fmt(f) }
        }

        impl Default for $name {
            fn default() -> Self { Self::AUTO }
        }
    };
}

track_fn! {
    /// The `min` half of a track sizing function.
    MinTrackSizingFunction { length => length, percent => percent }
    consts { AUTO => AUTO, ZERO => ZERO, MIN_CONTENT => MIN_CONTENT, MAX_CONTENT => MAX_CONTENT }
}

track_fn! {
    /// The `max` half of a track sizing function.
    MaxTrackSizingFunction { length => length, percent => percent, fr => fr, fit_content_px => fit_content_px, fit_content_percent => fit_content_percent }
    consts { AUTO => AUTO, ZERO => ZERO, MIN_CONTENT => MIN_CONTENT, MAX_CONTENT => MAX_CONTENT }
}

macro_rules! track_predicates {
    ($name:ident) => {
        impl $name {
            /// `auto`, `min-content`, `max-content` or `fit-content()`.
            #[inline]
            pub fn is_intrinsic(self) -> bool {
                self.0.is_auto() || self.0.is_min_content() || self.0.is_max_content() || self.0.is_fit_content()
            }
            /// `auto`, `max-content` or `fit-content()`.
            #[inline]
            pub fn is_max_content_alike(self) -> bool {
                self.0.is_auto() || self.0.is_max_content() || self.0.is_fit_content()
            }
            /// `max-content` or `fit-content()`.
            #[inline]
            pub fn is_max_or_fit_content(self) -> bool {
                self.0.is_max_content() || self.0.is_fit_content()
            }
            #[inline]
            pub fn is_min_or_max_content(self) -> bool {
                self.0.is_min_content() || self.0.is_max_content()
            }
            #[inline]
            pub fn has_definite_value(self, basis: Option<f32>) -> bool {
                self.0.is_length() || (self.0.uses_percentage() && !self.0.is_fit_content() && basis.is_some())
            }
        }
    };
}
track_predicates!(MinTrackSizingFunction);
track_predicates!(MaxTrackSizingFunction);

impl MinTrackSizingFunction {
    /// Definite value when the function is a length or resolvable percentage.
    #[inline]
    pub fn definite_value(self, basis: Option<f32>, calc: &impl Fn(CalcId, f32) -> f32) -> Option<f32> {
        self.0.resolve(basis, calc)
    }
}

impl MaxTrackSizingFunction {
    /// Definite value when the function is a length or resolvable percentage.
    #[inline]
    pub fn definite_value(self, basis: Option<f32>, calc: &impl Fn(CalcId, f32) -> f32) -> Option<f32> {
        self.0.resolve(basis, calc)
    }
    /// The limit of `fit-content()`, or the definite value.
    #[inline]
    pub fn definite_limit(self, basis: Option<f32>, calc: &impl Fn(CalcId, f32) -> f32) -> Option<f32> {
        use crate::style::length::LengthKind;
        match self.0.kind() {
            LengthKind::FitContentPx(v) => Some(v),
            LengthKind::FitContentPercent(f) => basis.map(|b| b * f),
            _ => self.definite_value(basis, calc),
        }
    }
}

/// `minmax(min, max)`, or a single value applied to both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackSizingFunction {
    pub min: MinTrackSizingFunction,
    pub max: MaxTrackSizingFunction,
}

impl TrackSizingFunction {
    pub const AUTO: Self = Self { min: MinTrackSizingFunction::AUTO, max: MaxTrackSizingFunction::AUTO };
    pub const MIN_CONTENT: Self =
        Self { min: MinTrackSizingFunction::MIN_CONTENT, max: MaxTrackSizingFunction::MIN_CONTENT };
    pub const MAX_CONTENT: Self =
        Self { min: MinTrackSizingFunction::MAX_CONTENT, max: MaxTrackSizingFunction::MAX_CONTENT };

    #[inline]
    pub const fn length(px: f32) -> Self {
        Self { min: MinTrackSizingFunction::length(px), max: MaxTrackSizingFunction::length(px) }
    }
    #[inline]
    pub const fn percent(f: f32) -> Self {
        Self { min: MinTrackSizingFunction::percent(f), max: MaxTrackSizingFunction::percent(f) }
    }
    /// `<flex>` alone means `minmax(auto, <flex>)`.
    #[inline]
    pub const fn fr(f: f32) -> Self {
        Self { min: MinTrackSizingFunction::AUTO, max: MaxTrackSizingFunction::fr(f) }
    }
    /// `fit-content()` alone means `minmax(auto, fit-content())`.
    #[inline]
    pub const fn fit_content_px(px: f32) -> Self {
        Self { min: MinTrackSizingFunction::AUTO, max: MaxTrackSizingFunction::fit_content_px(px) }
    }
    #[inline]
    pub const fn fit_content_percent(f: f32) -> Self {
        Self { min: MinTrackSizingFunction::AUTO, max: MaxTrackSizingFunction::fit_content_percent(f) }
    }
    #[inline]
    pub const fn minmax(min: MinTrackSizingFunction, max: MaxTrackSizingFunction) -> Self {
        Self { min, max }
    }
    /// Whether either half is a length or percentage.
    #[inline]
    pub fn has_fixed_component(self) -> bool {
        self.min.is_length() || self.min.is_percent() || self.max.is_length() || self.max.is_percent()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RepetitionCount {
    AutoFill,
    AutoFit,
    Count(u16),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GridTemplateRepetition {
    pub count: RepetitionCount,
    pub tracks: Vec<TrackSizingFunction>,
    /// Empty, or `tracks.len() + 1` name sets.
    pub line_names: Vec<Vec<Ident>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GridTemplateComponent {
    Single(TrackSizingFunction),
    Repeat(GridTemplateRepetition),
}

impl GridTemplateComponent {
    #[inline]
    pub fn is_auto_repetition(&self) -> bool {
        matches!(
            self,
            Self::Repeat(GridTemplateRepetition { count: RepetitionCount::AutoFill | RepetitionCount::AutoFit, .. })
        )
    }
}

impl From<TrackSizingFunction> for GridTemplateComponent {
    fn from(t: TrackSizingFunction) -> Self {
        Self::Single(t)
    }
}
