//! Axis-agnostic geometry used by every algorithm.

mod abstract_axis;
mod maybe;

pub use abstract_axis::AbstractAxis;
pub use maybe::MaybeMath;

use core::ops::{Add, Sub};

/// A physical axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AbsoluteAxis {
    Horizontal,
    Vertical,
}

impl AbsoluteAxis {
    #[inline]
    pub const fn other(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

/// A width/height pair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Size<T> {
    pub width: T,
    pub height: T,
}

/// A position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point<T> {
    pub x: T,
    pub y: T,
}

/// Four edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect<T> {
    pub left: T,
    pub right: T,
    pub top: T,
    pub bottom: T,
}

/// A start/end pair along one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Line<T> {
    pub start: T,
    pub end: T,
}

/// Definite space, or an intrinsic sizing request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AvailableSpace {
    Definite(f32),
    MinContent,
    MaxContent,
}

impl<T> Size<T> {
    #[inline]
    pub const fn new(width: T, height: T) -> Self {
        Self { width, height }
    }
    #[inline]
    pub fn map<U>(self, f: impl Fn(T) -> U) -> Size<U> {
        Size { width: f(self.width), height: f(self.height) }
    }
    #[inline]
    pub fn map_width(mut self, f: impl FnOnce(T) -> T) -> Self {
        self.width = f(self.width);
        self
    }
    #[inline]
    pub fn map_height(mut self, f: impl FnOnce(T) -> T) -> Self {
        self.height = f(self.height);
        self
    }
    #[inline]
    pub fn zip_map<U, R>(self, other: Size<U>, f: impl Fn(T, U) -> R) -> Size<R> {
        Size { width: f(self.width, other.width), height: f(self.height, other.height) }
    }
    #[inline]
    pub fn get(self, axis: AbsoluteAxis) -> T {
        match axis {
            AbsoluteAxis::Horizontal => self.width,
            AbsoluteAxis::Vertical => self.height,
        }
    }
    #[inline]
    pub fn get_ref(&self, axis: AbsoluteAxis) -> &T {
        match axis {
            AbsoluteAxis::Horizontal => &self.width,
            AbsoluteAxis::Vertical => &self.height,
        }
    }
    #[inline]
    pub fn set(&mut self, axis: AbsoluteAxis, value: T) {
        match axis {
            AbsoluteAxis::Horizontal => self.width = value,
            AbsoluteAxis::Vertical => self.height = value,
        }
    }
    #[inline]
    pub fn with(mut self, axis: AbsoluteAxis, value: T) -> Self {
        self.set(axis, value);
        self
    }
    #[inline]
    pub fn transpose(self) -> Self {
        Size { width: self.height, height: self.width }
    }
}

impl<T: Copy> Size<T> {
    #[inline]
    pub const fn splat(v: T) -> Self {
        Self { width: v, height: v }
    }
}

impl Size<f32> {
    pub const ZERO: Self = Self { width: 0.0, height: 0.0 };
    #[inline]
    pub fn f32_max(self, other: Self) -> Self {
        Size { width: self.width.max(other.width), height: self.height.max(other.height) }
    }
    #[inline]
    pub fn f32_min(self, other: Self) -> Self {
        Size { width: self.width.min(other.width), height: self.height.min(other.height) }
    }
    #[inline]
    pub fn has_non_zero_area(self) -> bool {
        self.width != 0.0 && self.height != 0.0
    }
}

impl Size<Option<f32>> {
    pub const NONE: Self = Self { width: None, height: None };
    #[inline]
    pub fn or(self, other: Self) -> Self {
        Size { width: self.width.or(other.width), height: self.height.or(other.height) }
    }
    #[inline]
    pub fn unwrap_or(self, other: Size<f32>) -> Size<f32> {
        Size { width: self.width.unwrap_or(other.width), height: self.height.unwrap_or(other.height) }
    }
    #[inline]
    pub fn both_some(self) -> bool {
        self.width.is_some() && self.height.is_some()
    }
    /// Derives the missing axis from an aspect ratio (width / height).
    #[inline]
    pub fn maybe_apply_aspect_ratio(self, ratio: Option<f32>) -> Self {
        match ratio {
            Some(r) => match (self.width, self.height) {
                (Some(w), None) => Size { width: Some(w), height: Some(w / r) },
                (None, Some(h)) => Size { width: Some(h * r), height: Some(h) },
                _ => self,
            },
            None => self,
        }
    }
}

impl Size<AvailableSpace> {
    pub const MIN_CONTENT: Self = Self { width: AvailableSpace::MinContent, height: AvailableSpace::MinContent };
    pub const MAX_CONTENT: Self = Self { width: AvailableSpace::MaxContent, height: AvailableSpace::MaxContent };
    #[inline]
    pub fn into_options(self) -> Size<Option<f32>> {
        self.map(AvailableSpace::into_option)
    }
    #[inline]
    pub fn maybe_set(self, value: Size<Option<f32>>) -> Self {
        self.zip_map(value, |a, v| v.map_or(a, AvailableSpace::Definite))
    }
}

impl Size<bool> {
    pub const TRUE: Self = Self { width: true, height: true };
    pub const FALSE: Self = Self { width: false, height: false };
}

impl<T: Add<Output = T>> Add for Size<T> {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Size { width: self.width + rhs.width, height: self.height + rhs.height }
    }
}

impl<T: Sub<Output = T>> Sub for Size<T> {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Size { width: self.width - rhs.width, height: self.height - rhs.height }
    }
}

impl<T> Point<T> {
    #[inline]
    pub fn get(self, axis: AbsoluteAxis) -> T {
        match axis {
            AbsoluteAxis::Horizontal => self.x,
            AbsoluteAxis::Vertical => self.y,
        }
    }
    #[inline]
    pub fn set(&mut self, axis: AbsoluteAxis, value: T) {
        match axis {
            AbsoluteAxis::Horizontal => self.x = value,
            AbsoluteAxis::Vertical => self.y = value,
        }
    }
    #[inline]
    pub fn transpose(self) -> Self {
        Point { x: self.y, y: self.x }
    }
    #[inline]
    pub fn map<U>(self, f: impl Fn(T) -> U) -> Point<U> {
        Point { x: f(self.x), y: f(self.y) }
    }
}

impl Point<f32> {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
}

impl<T: Add<Output = T>> Add for Point<T> {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Point { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl<T> Rect<T> {
    #[inline]
    pub fn map<U>(self, f: impl Fn(T) -> U) -> Rect<U> {
        Rect { left: f(self.left), right: f(self.right), top: f(self.top), bottom: f(self.bottom) }
    }
    #[inline]
    pub fn zip_size<U, R>(self, size: Size<U>, f: impl Fn(T, U) -> R) -> Rect<R>
    where
        U: Copy,
    {
        Rect {
            left: f(self.left, size.width),
            right: f(self.right, size.width),
            top: f(self.top, size.height),
            bottom: f(self.bottom, size.height),
        }
    }
    #[inline]
    pub fn start(self, axis: AbsoluteAxis) -> T {
        match axis {
            AbsoluteAxis::Horizontal => self.left,
            AbsoluteAxis::Vertical => self.top,
        }
    }
    #[inline]
    pub fn end(self, axis: AbsoluteAxis) -> T {
        match axis {
            AbsoluteAxis::Horizontal => self.right,
            AbsoluteAxis::Vertical => self.bottom,
        }
    }
    #[inline]
    pub fn line(self, axis: AbsoluteAxis) -> Line<T> {
        match axis {
            AbsoluteAxis::Horizontal => Line { start: self.left, end: self.right },
            AbsoluteAxis::Vertical => Line { start: self.top, end: self.bottom },
        }
    }
}

impl<T: Copy> Rect<T> {
    #[inline]
    pub const fn splat(v: T) -> Self {
        Self { left: v, right: v, top: v, bottom: v }
    }
}

impl<T: Add<Output = T> + Copy> Rect<T> {
    #[inline]
    pub fn horizontal_axis_sum(&self) -> T {
        self.left + self.right
    }
    #[inline]
    pub fn vertical_axis_sum(&self) -> T {
        self.top + self.bottom
    }
    #[inline]
    pub fn sum_axes(&self) -> Size<T> {
        Size { width: self.left + self.right, height: self.top + self.bottom }
    }
    #[inline]
    pub fn axis_sum(&self, axis: AbsoluteAxis) -> T {
        match axis {
            AbsoluteAxis::Horizontal => self.horizontal_axis_sum(),
            AbsoluteAxis::Vertical => self.vertical_axis_sum(),
        }
    }
}

impl<T: Add<Output = T>> Add for Rect<T> {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Rect {
            left: self.left + rhs.left,
            right: self.right + rhs.right,
            top: self.top + rhs.top,
            bottom: self.bottom + rhs.bottom,
        }
    }
}

impl Rect<f32> {
    pub const ZERO: Self = Self { left: 0.0, right: 0.0, top: 0.0, bottom: 0.0 };
}

impl Rect<Option<f32>> {
    pub const NONE: Self = Self { left: None, right: None, top: None, bottom: None };
}

impl<T> Line<T> {
    #[inline]
    pub fn map<U>(self, f: impl Fn(T) -> U) -> Line<U> {
        Line { start: f(self.start), end: f(self.end) }
    }
}

impl<T: Add<Output = T> + Copy> Line<T> {
    #[inline]
    pub fn sum(&self) -> T {
        self.start + self.end
    }
}

impl Line<bool> {
    pub const TRUE: Self = Self { start: true, end: true };
    pub const FALSE: Self = Self { start: false, end: false };
}

impl Line<f32> {
    pub const ZERO: Self = Self { start: 0.0, end: 0.0 };
}

impl AvailableSpace {
    pub const ZERO: Self = Self::Definite(0.0);
    #[inline]
    pub fn into_option(self) -> Option<f32> {
        match self {
            Self::Definite(v) => Some(v),
            _ => None,
        }
    }
    #[inline]
    pub fn is_definite(self) -> bool {
        matches!(self, Self::Definite(_))
    }
    #[inline]
    pub fn unwrap_or(self, default: f32) -> f32 {
        self.into_option().unwrap_or(default)
    }
    /// Free space left after `used`; infinite for max-content, none for min-content.
    #[inline]
    pub fn compute_free_space(self, used: f32) -> f32 {
        match self {
            Self::MaxContent => f32::INFINITY,
            Self::MinContent => 0.0,
            Self::Definite(v) => v - used,
        }
    }
    #[inline]
    pub fn map_definite(self, f: impl FnOnce(f32) -> f32) -> Self {
        match self {
            Self::Definite(v) => Self::Definite(f(v)),
            other => other,
        }
    }
}

impl From<f32> for AvailableSpace {
    fn from(v: f32) -> Self {
        Self::Definite(v)
    }
}

impl From<Option<f32>> for AvailableSpace {
    fn from(v: Option<f32>) -> Self {
        v.map_or(Self::MaxContent, Self::Definite)
    }
}
