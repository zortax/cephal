//! Flexbox keywords.

use crate::geometry::AbsoluteAxis;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FlexDirection {
    #[default]
    Row,
    Column,
    RowReverse,
    ColumnReverse,
}

impl FlexDirection {
    #[inline]
    pub const fn is_row(self) -> bool {
        matches!(self, Self::Row | Self::RowReverse)
    }
    #[inline]
    pub const fn is_column(self) -> bool {
        !self.is_row()
    }
    #[inline]
    pub const fn is_reverse(self) -> bool {
        matches!(self, Self::RowReverse | Self::ColumnReverse)
    }
    #[inline]
    pub const fn main_axis(self) -> AbsoluteAxis {
        if self.is_row() { AbsoluteAxis::Horizontal } else { AbsoluteAxis::Vertical }
    }
    #[inline]
    pub const fn cross_axis(self) -> AbsoluteAxis {
        if self.is_row() { AbsoluteAxis::Vertical } else { AbsoluteAxis::Horizontal }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FlexWrap {
    #[default]
    NoWrap,
    Wrap,
    WrapReverse,
    /// `wrap balance` (css-flexbox-2).
    Balance,
    /// `wrap-reverse balance`.
    BalanceReverse,
}

impl FlexWrap {
    #[inline]
    pub const fn is_multi_line(self) -> bool {
        !matches!(self, Self::NoWrap)
    }
    #[inline]
    pub const fn is_reverse(self) -> bool {
        matches!(self, Self::WrapReverse | Self::BalanceReverse)
    }
    #[inline]
    pub const fn is_balance(self) -> bool {
        matches!(self, Self::Balance | Self::BalanceReverse)
    }
}
