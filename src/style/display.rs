//! Box-model keywords.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Display {
    #[default]
    Block,
    FlowRoot,
    Flex,
    Grid,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BoxSizing {
    #[default]
    BorderBox,
    ContentBox,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Position {
    /// In flow; insets are ignored. The initial value.
    #[default]
    Static,
    /// In flow, offset by its insets; the containing block of absolutely positioned descendants.
    Relative,
    Absolute,
    /// Placed exactly like [`Position::Absolute`], against the parent; the host anchors the box to
    /// the viewport, through where it parents it or how it composes scroll offsets. A fixed box
    /// scrolls with nothing, so it adds no scrollable overflow to its container.
    Fixed,
    /// Laid out as [`Position::Static`] (sticky offsets need scroll positions, which layout does not
    /// have), but the containing block of absolutely positioned descendants.
    Sticky,
}

impl Position {
    /// Whether the box is positioned out of flow (`absolute` or `fixed`).
    #[inline]
    pub fn is_out_of_flow(self) -> bool {
        matches!(self, Position::Absolute | Position::Fixed)
    }

    /// Whether the box is positioned, that is anything but `static`.
    #[inline]
    pub fn is_positioned(self) -> bool {
        self != Position::Static
    }

    /// Whether an in-flow box is offset by its insets.
    #[inline]
    pub fn offsets_in_flow(self) -> bool {
        self == Position::Relative
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Overflow {
    #[default]
    Visible,
    Clip,
    Hidden,
    Scroll,
}

impl Overflow {
    /// `hidden` and `scroll` establish a scroll container, which zeroes the automatic minimum size.
    #[inline]
    pub fn is_scroll_container(self) -> bool {
        matches!(self, Self::Hidden | Self::Scroll)
    }
    #[inline]
    pub fn maybe_into_automatic_min_size(self) -> Option<f32> {
        if self.is_scroll_container() { Some(0.0) } else { None }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Float {
    #[default]
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Clear {
    #[default]
    None,
    Left,
    Right,
    Both,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextAlign {
    #[default]
    Auto,
    LegacyLeft,
    LegacyRight,
    LegacyCenter,
}

/// `contain` bit set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Contain(u8);

impl Contain {
    pub const NONE: Self = Self(0);
    pub const LAYOUT: Self = Self(1);
    pub const PAINT: Self = Self(2);
    pub const CONTENT: Self = Self(3);

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    /// `layout` or `paint` containment establishes an independent formatting context.
    #[inline]
    pub const fn establishes_independent_formatting_context(self) -> bool {
        self.0 != 0
    }
    /// `layout` containment suppresses the box's baseline.
    #[inline]
    pub const fn suppresses_baseline(self) -> bool {
        self.contains(Self::LAYOUT)
    }
    /// `paint` containment clips overflow.
    #[inline]
    pub const fn contains_scrollable_overflow(self) -> bool {
        self.contains(Self::PAINT)
    }
}
