//! Typed views over [`Length`] restricting which keywords a property accepts.

use super::calc::CalcId;
use super::length::Length;

macro_rules! length_newtype {
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
            #[inline]
            pub const fn raw(self) -> Length { self.0 }
        }

        impl core::ops::Deref for $name {
            type Target = Length;
            #[inline]
            fn deref(&self) -> &Length { &self.0 }
        }

        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { self.0.fmt(f) }
        }

        impl From<$name> for Length {
            fn from(v: $name) -> Length { v.0 }
        }
    };
}

length_newtype! {
    /// A length or a percentage.
    LengthPercentage { length => length, percent => percent }
    consts { ZERO => ZERO }
}

length_newtype! {
    /// A length, a percentage or `auto`.
    LengthPercentageAuto { length => length, percent => percent }
    consts { ZERO => ZERO, AUTO => AUTO }
}

length_newtype! {
    /// A size: length, percentage, `auto`, sizing keyword or `content`.
    Dimension { length => length, percent => percent, fit_content_px => fit_content_px, fit_content_percent => fit_content_percent }
    consts { ZERO => ZERO, AUTO => AUTO, MIN_CONTENT => MIN_CONTENT, MAX_CONTENT => MAX_CONTENT, FIT_CONTENT => FIT_CONTENT, STRETCH => STRETCH, CONTENT => CONTENT }
}

impl Default for Dimension {
    fn default() -> Self {
        Self::AUTO
    }
}
impl Default for LengthPercentageAuto {
    fn default() -> Self {
        Self::AUTO
    }
}
impl Default for LengthPercentage {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<LengthPercentage> for LengthPercentageAuto {
    fn from(v: LengthPercentage) -> Self {
        Self(v.0)
    }
}
impl From<LengthPercentage> for Dimension {
    fn from(v: LengthPercentage) -> Self {
        Self(v.0)
    }
}
impl From<LengthPercentageAuto> for Dimension {
    fn from(v: LengthPercentageAuto) -> Self {
        Self(v.0)
    }
}

impl Dimension {
    /// Drops sizing keywords, which only `auto`-like properties accept.
    #[inline]
    pub fn into_length_percentage_auto(self) -> LengthPercentageAuto {
        if self.0.is_sizing_keyword() || self.0.is_content() { LengthPercentageAuto::AUTO } else { LengthPercentageAuto(self.0) }
    }
}
