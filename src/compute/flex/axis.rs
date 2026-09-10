//! Main/cross axis views over geometry.

use crate::geometry::{AbsoluteAxis, Point, Rect, Size};
use crate::style::FlexDirection;

pub(super) trait FlexAxisSize<T> {
    fn main(self, dir: FlexDirection) -> T;
    fn cross(self, dir: FlexDirection) -> T;
    fn set_main(&mut self, dir: FlexDirection, v: T);
    fn set_cross(&mut self, dir: FlexDirection, v: T);
    fn with_main(self, dir: FlexDirection, v: T) -> Self;
    fn with_cross(self, dir: FlexDirection, v: T) -> Self;
}

impl<T: Copy> FlexAxisSize<T> for Size<T> {
    #[inline]
    fn main(self, dir: FlexDirection) -> T {
        self.get(dir.main_axis())
    }
    #[inline]
    fn cross(self, dir: FlexDirection) -> T {
        self.get(dir.cross_axis())
    }
    #[inline]
    fn set_main(&mut self, dir: FlexDirection, v: T) {
        self.set(dir.main_axis(), v)
    }
    #[inline]
    fn set_cross(&mut self, dir: FlexDirection, v: T) {
        self.set(dir.cross_axis(), v)
    }
    #[inline]
    fn with_main(self, dir: FlexDirection, v: T) -> Self {
        self.with(dir.main_axis(), v)
    }
    #[inline]
    fn with_cross(self, dir: FlexDirection, v: T) -> Self {
        self.with(dir.cross_axis(), v)
    }
}

impl<T: Copy> FlexAxisSize<T> for Point<T> {
    #[inline]
    fn main(self, dir: FlexDirection) -> T {
        self.get(dir.main_axis())
    }
    #[inline]
    fn cross(self, dir: FlexDirection) -> T {
        self.get(dir.cross_axis())
    }
    #[inline]
    fn set_main(&mut self, dir: FlexDirection, v: T) {
        self.set(dir.main_axis(), v)
    }
    #[inline]
    fn set_cross(&mut self, dir: FlexDirection, v: T) {
        self.set(dir.cross_axis(), v)
    }
    #[inline]
    fn with_main(mut self, dir: FlexDirection, v: T) -> Self {
        self.set(dir.main_axis(), v);
        self
    }
    #[inline]
    fn with_cross(mut self, dir: FlexDirection, v: T) -> Self {
        self.set(dir.cross_axis(), v);
        self
    }
}

pub(super) trait FlexAxisRect<T> {
    fn main_start(&self, dir: FlexDirection) -> T;
    fn main_end(&self, dir: FlexDirection) -> T;
    fn cross_start(&self, dir: FlexDirection) -> T;
    fn cross_end(&self, dir: FlexDirection) -> T;
}

impl<T: Copy> FlexAxisRect<T> for Rect<T> {
    #[inline]
    fn main_start(&self, dir: FlexDirection) -> T {
        self.start(dir.main_axis())
    }
    #[inline]
    fn main_end(&self, dir: FlexDirection) -> T {
        self.end(dir.main_axis())
    }
    #[inline]
    fn cross_start(&self, dir: FlexDirection) -> T {
        self.start(dir.cross_axis())
    }
    #[inline]
    fn cross_end(&self, dir: FlexDirection) -> T {
        self.end(dir.cross_axis())
    }
}

pub(super) trait FlexAxisSum {
    fn main_axis_sum(&self, dir: FlexDirection) -> f32;
    fn cross_axis_sum(&self, dir: FlexDirection) -> f32;
}

impl FlexAxisSum for Rect<f32> {
    #[inline]
    fn main_axis_sum(&self, dir: FlexDirection) -> f32 {
        self.axis_sum(dir.main_axis())
    }
    #[inline]
    fn cross_axis_sum(&self, dir: FlexDirection) -> f32 {
        self.axis_sum(dir.cross_axis())
    }
}

#[inline]
pub(super) fn from_cross<T: Copy + Default>(dir: FlexDirection, v: T) -> Size<T> {
    Size::default().with(dir.cross_axis(), v)
}

#[inline]
pub(super) fn abs_main(dir: FlexDirection) -> AbsoluteAxis {
    dir.main_axis()
}

/// Sum of `count - 1` gaps.
#[inline]
pub(super) fn sum_axis_gaps(gap: f32, count: usize) -> f32 {
    if count <= 1 { 0.0 } else { gap * (count - 1) as f32 }
}
