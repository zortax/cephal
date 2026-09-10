//! Alignment keywords with their safety.

use super::display::Direction;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum AlignItemsKeyword {
    Start,
    End,
    FlexStart,
    FlexEnd,
    SelfStart,
    SelfEnd,
    Center,
    Baseline,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum AlignContentKeyword {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
    SpaceBetween,
    SpaceEvenly,
    SpaceAround,
}

impl AlignContentKeyword {
    /// Physical reversal for `rtl` containers.
    #[inline]
    pub fn reversed(self) -> Self {
        match self {
            Self::Start | Self::Stretch => Self::End,
            Self::End => Self::Start,
            Self::FlexStart => Self::FlexEnd,
            Self::FlexEnd => Self::FlexStart,
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum AlignmentSafety {
    #[default]
    Unsafe,
    Safe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlignItems {
    pub keyword: AlignItemsKeyword,
    pub safety: AlignmentSafety,
}

pub type AlignSelf = AlignItems;
pub type JustifyItems = AlignItems;
pub type JustifySelf = AlignItems;

impl AlignItems {
    const fn new(keyword: AlignItemsKeyword) -> Self {
        Self { keyword, safety: AlignmentSafety::Unsafe }
    }
    const fn safe(keyword: AlignItemsKeyword) -> Self {
        Self { keyword, safety: AlignmentSafety::Safe }
    }
    pub const START: Self = Self::new(AlignItemsKeyword::Start);
    pub const END: Self = Self::new(AlignItemsKeyword::End);
    pub const FLEX_START: Self = Self::new(AlignItemsKeyword::FlexStart);
    pub const FLEX_END: Self = Self::new(AlignItemsKeyword::FlexEnd);
    pub const SELF_START: Self = Self::new(AlignItemsKeyword::SelfStart);
    pub const SELF_END: Self = Self::new(AlignItemsKeyword::SelfEnd);
    pub const CENTER: Self = Self::new(AlignItemsKeyword::Center);
    pub const BASELINE: Self = Self::new(AlignItemsKeyword::Baseline);
    pub const STRETCH: Self = Self::new(AlignItemsKeyword::Stretch);
    pub const SAFE_START: Self = Self::safe(AlignItemsKeyword::Start);
    pub const SAFE_END: Self = Self::safe(AlignItemsKeyword::End);
    pub const SAFE_FLEX_START: Self = Self::safe(AlignItemsKeyword::FlexStart);
    pub const SAFE_FLEX_END: Self = Self::safe(AlignItemsKeyword::FlexEnd);
    pub const SAFE_CENTER: Self = Self::safe(AlignItemsKeyword::Center);
    pub const SAFE_SELF_START: Self = Self::safe(AlignItemsKeyword::SelfStart);
    pub const SAFE_SELF_END: Self = Self::safe(AlignItemsKeyword::SelfEnd);

    #[inline]
    pub const fn is_safe(self) -> bool {
        matches!(self.safety, AlignmentSafety::Safe)
    }
    #[inline]
    pub const fn with_keyword(self, keyword: AlignItemsKeyword) -> Self {
        Self { keyword, safety: self.safety }
    }

    /// Resolves `self-start`/`self-end` against the item's and container's directions.
    #[inline]
    pub fn resolve_self_relative(self, item: Direction, container: Direction, axis_is_inline: bool) -> Self {
        let flip = axis_is_inline && item != container;
        match self.keyword {
            AlignItemsKeyword::SelfStart => {
                self.with_keyword(if flip { AlignItemsKeyword::End } else { AlignItemsKeyword::Start })
            }
            AlignItemsKeyword::SelfEnd => {
                self.with_keyword(if flip { AlignItemsKeyword::Start } else { AlignItemsKeyword::End })
            }
            _ => self,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlignContent {
    pub keyword: AlignContentKeyword,
    pub safety: AlignmentSafety,
}

pub type JustifyContent = AlignContent;

impl AlignContent {
    const fn new(keyword: AlignContentKeyword) -> Self {
        Self { keyword, safety: AlignmentSafety::Unsafe }
    }
    const fn safe(keyword: AlignContentKeyword) -> Self {
        Self { keyword, safety: AlignmentSafety::Safe }
    }
    pub const START: Self = Self::new(AlignContentKeyword::Start);
    pub const END: Self = Self::new(AlignContentKeyword::End);
    pub const FLEX_START: Self = Self::new(AlignContentKeyword::FlexStart);
    pub const FLEX_END: Self = Self::new(AlignContentKeyword::FlexEnd);
    pub const CENTER: Self = Self::new(AlignContentKeyword::Center);
    pub const STRETCH: Self = Self::new(AlignContentKeyword::Stretch);
    pub const SPACE_BETWEEN: Self = Self::new(AlignContentKeyword::SpaceBetween);
    pub const SPACE_EVENLY: Self = Self::new(AlignContentKeyword::SpaceEvenly);
    pub const SPACE_AROUND: Self = Self::new(AlignContentKeyword::SpaceAround);
    pub const SAFE_START: Self = Self::safe(AlignContentKeyword::Start);
    pub const SAFE_END: Self = Self::safe(AlignContentKeyword::End);
    pub const SAFE_FLEX_START: Self = Self::safe(AlignContentKeyword::FlexStart);
    pub const SAFE_FLEX_END: Self = Self::safe(AlignContentKeyword::FlexEnd);
    pub const SAFE_CENTER: Self = Self::safe(AlignContentKeyword::Center);

    #[inline]
    pub const fn is_safe(self) -> bool {
        matches!(self.safety, AlignmentSafety::Safe)
    }
    #[inline]
    pub const fn with_keyword(self, keyword: AlignContentKeyword) -> Self {
        Self { keyword, safety: self.safety }
    }
}
