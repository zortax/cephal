//! Inline/block axes, mapped to horizontal/vertical for horizontal writing modes.

use super::{AbsoluteAxis, Size};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AbstractAxis {
    Inline,
    Block,
}

impl AbstractAxis {
    #[inline]
    pub const fn other(self) -> Self {
        match self {
            Self::Inline => Self::Block,
            Self::Block => Self::Inline,
        }
    }
    #[inline]
    pub const fn as_abs(self) -> AbsoluteAxis {
        match self {
            Self::Inline => AbsoluteAxis::Horizontal,
            Self::Block => AbsoluteAxis::Vertical,
        }
    }
}

impl<T> Size<T> {
    #[inline]
    pub fn get_abstract(self, axis: AbstractAxis) -> T {
        self.get(axis.as_abs())
    }
    #[inline]
    pub fn set_abstract(&mut self, axis: AbstractAxis, v: T) {
        self.set(axis.as_abs(), v)
    }
    #[inline]
    pub fn with_abstract(self, axis: AbstractAxis, v: T) -> Self {
        self.with(axis.as_abs(), v)
    }
}
