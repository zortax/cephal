//! Main/cross axis views over geometry.

use crate::geometry::{AbsoluteAxis, Point, Rect, Size};
use crate::style::FlexDirection;

/// A flex direction. [`Row`] and [`Column`] fix the main axis at compile time, so the algorithm
/// is instantiated once per axis instead of choosing an axis at every access.
pub(crate) trait FlexDir: Copy {
    fn is_row(self) -> bool;
    fn is_reverse(self) -> bool;
    #[inline]
    fn is_column(self) -> bool {
        !self.is_row()
    }
    #[inline]
    fn main_axis(self) -> AbsoluteAxis {
        if self.is_row() { AbsoluteAxis::Horizontal } else { AbsoluteAxis::Vertical }
    }
    #[inline]
    fn cross_axis(self) -> AbsoluteAxis {
        if self.is_row() { AbsoluteAxis::Vertical } else { AbsoluteAxis::Horizontal }
    }
}

impl FlexDir for FlexDirection {
    #[inline]
    fn is_row(self) -> bool {
        FlexDirection::is_row(self)
    }
    #[inline]
    fn is_reverse(self) -> bool {
        FlexDirection::is_reverse(self)
    }
}

/// `row` or `row-reverse`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Row(pub bool);

/// `column` or `column-reverse`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Column(pub bool);

impl FlexDir for Row {
    #[inline(always)]
    fn is_row(self) -> bool {
        true
    }
    #[inline(always)]
    fn is_reverse(self) -> bool {
        self.0
    }
}

impl FlexDir for Column {
    #[inline(always)]
    fn is_row(self) -> bool {
        false
    }
    #[inline(always)]
    fn is_reverse(self) -> bool {
        self.0
    }
}

pub(super) trait FlexAxisSize<T> {
    fn main(self, dir: impl FlexDir) -> T;
    fn cross(self, dir: impl FlexDir) -> T;
    fn set_main(&mut self, dir: impl FlexDir, v: T);
    fn set_cross(&mut self, dir: impl FlexDir, v: T);
    fn with_main(self, dir: impl FlexDir, v: T) -> Self;
    fn with_cross(self, dir: impl FlexDir, v: T) -> Self;
}

impl<T: Copy> FlexAxisSize<T> for Size<T> {
    #[inline]
    fn main(self, dir: impl FlexDir) -> T {
        self.get(dir.main_axis())
    }
    #[inline]
    fn cross(self, dir: impl FlexDir) -> T {
        self.get(dir.cross_axis())
    }
    #[inline]
    fn set_main(&mut self, dir: impl FlexDir, v: T) {
        self.set(dir.main_axis(), v)
    }
    #[inline]
    fn set_cross(&mut self, dir: impl FlexDir, v: T) {
        self.set(dir.cross_axis(), v)
    }
    #[inline]
    fn with_main(self, dir: impl FlexDir, v: T) -> Self {
        self.with(dir.main_axis(), v)
    }
    #[inline]
    fn with_cross(self, dir: impl FlexDir, v: T) -> Self {
        self.with(dir.cross_axis(), v)
    }
}

impl<T: Copy> FlexAxisSize<T> for Point<T> {
    #[inline]
    fn main(self, dir: impl FlexDir) -> T {
        self.get(dir.main_axis())
    }
    #[inline]
    fn cross(self, dir: impl FlexDir) -> T {
        self.get(dir.cross_axis())
    }
    #[inline]
    fn set_main(&mut self, dir: impl FlexDir, v: T) {
        self.set(dir.main_axis(), v)
    }
    #[inline]
    fn set_cross(&mut self, dir: impl FlexDir, v: T) {
        self.set(dir.cross_axis(), v)
    }
    #[inline]
    fn with_main(mut self, dir: impl FlexDir, v: T) -> Self {
        self.set(dir.main_axis(), v);
        self
    }
    #[inline]
    fn with_cross(mut self, dir: impl FlexDir, v: T) -> Self {
        self.set(dir.cross_axis(), v);
        self
    }
}

pub(super) trait FlexAxisRect<T> {
    fn main_start(&self, dir: impl FlexDir) -> T;
    fn main_end(&self, dir: impl FlexDir) -> T;
    fn cross_start(&self, dir: impl FlexDir) -> T;
    fn cross_end(&self, dir: impl FlexDir) -> T;
}

impl<T: Copy> FlexAxisRect<T> for Rect<T> {
    #[inline]
    fn main_start(&self, dir: impl FlexDir) -> T {
        self.start(dir.main_axis())
    }
    #[inline]
    fn main_end(&self, dir: impl FlexDir) -> T {
        self.end(dir.main_axis())
    }
    #[inline]
    fn cross_start(&self, dir: impl FlexDir) -> T {
        self.start(dir.cross_axis())
    }
    #[inline]
    fn cross_end(&self, dir: impl FlexDir) -> T {
        self.end(dir.cross_axis())
    }
}

pub(super) trait FlexAxisSum {
    fn main_axis_sum(&self, dir: impl FlexDir) -> f32;
    fn cross_axis_sum(&self, dir: impl FlexDir) -> f32;
}

impl FlexAxisSum for Rect<f32> {
    #[inline]
    fn main_axis_sum(&self, dir: impl FlexDir) -> f32 {
        self.axis_sum(dir.main_axis())
    }
    #[inline]
    fn cross_axis_sum(&self, dir: impl FlexDir) -> f32 {
        self.axis_sum(dir.cross_axis())
    }
}

#[inline]
pub(super) fn from_cross<T: Copy + Default>(dir: impl FlexDir, v: T) -> Size<T> {
    Size::default().with(dir.cross_axis(), v)
}

#[inline]
pub(super) fn abs_main(dir: impl FlexDir) -> AbsoluteAxis {
    dir.main_axis()
}

/// Sum of `count - 1` gaps.
#[inline]
pub(super) fn sum_axis_gaps(gap: f32, count: usize) -> f32 {
    if count <= 1 { 0.0 } else { gap * (count - 1) as f32 }
}
