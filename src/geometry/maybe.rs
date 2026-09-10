//! Arithmetic over optional values: a `None` left side stays `None`; a `None` right side is the identity.

use super::{AvailableSpace, Size};

pub trait MaybeMath<Rhs, Out> {
    fn maybe_min(self, rhs: Rhs) -> Out;
    fn maybe_max(self, rhs: Rhs) -> Out;
    /// `max` is applied first, so `min` wins when they conflict.
    fn maybe_clamp(self, min: Rhs, max: Rhs) -> Out;
    fn maybe_add(self, rhs: Rhs) -> Out;
    fn maybe_sub(self, rhs: Rhs) -> Out;
}

impl MaybeMath<Option<f32>, Option<f32>> for Option<f32> {
    #[inline]
    fn maybe_min(self, rhs: Option<f32>) -> Option<f32> {
        match (self, rhs) {
            (Some(l), Some(r)) => Some(l.min(r)),
            (l, _) => l,
        }
    }
    #[inline]
    fn maybe_max(self, rhs: Option<f32>) -> Option<f32> {
        match (self, rhs) {
            (Some(l), Some(r)) => Some(l.max(r)),
            (l, _) => l,
        }
    }
    #[inline]
    fn maybe_clamp(self, min: Option<f32>, max: Option<f32>) -> Option<f32> {
        match (self, min, max) {
            (Some(b), Some(lo), Some(hi)) => Some(b.min(hi).max(lo)),
            (Some(b), None, Some(hi)) => Some(b.min(hi)),
            (Some(b), Some(lo), None) => Some(b.max(lo)),
            (b, _, _) => b,
        }
    }
    #[inline]
    fn maybe_add(self, rhs: Option<f32>) -> Option<f32> {
        match (self, rhs) {
            (Some(l), Some(r)) => Some(l + r),
            (l, _) => l,
        }
    }
    #[inline]
    fn maybe_sub(self, rhs: Option<f32>) -> Option<f32> {
        match (self, rhs) {
            (Some(l), Some(r)) => Some(l - r),
            (l, _) => l,
        }
    }
}

impl MaybeMath<f32, Option<f32>> for Option<f32> {
    #[inline]
    fn maybe_min(self, rhs: f32) -> Option<f32> {
        self.map(|l| l.min(rhs))
    }
    #[inline]
    fn maybe_max(self, rhs: f32) -> Option<f32> {
        self.map(|l| l.max(rhs))
    }
    #[inline]
    fn maybe_clamp(self, min: f32, max: f32) -> Option<f32> {
        self.map(|l| l.min(max).max(min))
    }
    #[inline]
    fn maybe_add(self, rhs: f32) -> Option<f32> {
        self.map(|l| l + rhs)
    }
    #[inline]
    fn maybe_sub(self, rhs: f32) -> Option<f32> {
        self.map(|l| l - rhs)
    }
}

impl MaybeMath<Option<f32>, f32> for f32 {
    #[inline]
    fn maybe_min(self, rhs: Option<f32>) -> f32 {
        rhs.map_or(self, |r| self.min(r))
    }
    #[inline]
    fn maybe_max(self, rhs: Option<f32>) -> f32 {
        rhs.map_or(self, |r| self.max(r))
    }
    #[inline]
    fn maybe_clamp(self, min: Option<f32>, max: Option<f32>) -> f32 {
        match (min, max) {
            (Some(lo), Some(hi)) => self.min(hi).max(lo),
            (None, Some(hi)) => self.min(hi),
            (Some(lo), None) => self.max(lo),
            (None, None) => self,
        }
    }
    #[inline]
    fn maybe_add(self, rhs: Option<f32>) -> f32 {
        rhs.map_or(self, |r| self + r)
    }
    #[inline]
    fn maybe_sub(self, rhs: Option<f32>) -> f32 {
        rhs.map_or(self, |r| self - r)
    }
}

impl MaybeMath<f32, AvailableSpace> for AvailableSpace {
    /// Intrinsic keywords become definite when capped.
    #[inline]
    fn maybe_min(self, rhs: f32) -> AvailableSpace {
        match self {
            AvailableSpace::Definite(v) => AvailableSpace::Definite(v.min(rhs)),
            _ => AvailableSpace::Definite(rhs),
        }
    }
    #[inline]
    fn maybe_max(self, rhs: f32) -> AvailableSpace {
        self.map_definite(|v| v.max(rhs))
    }
    #[inline]
    fn maybe_clamp(self, min: f32, max: f32) -> AvailableSpace {
        self.map_definite(|v| v.min(max).max(min))
    }
    #[inline]
    fn maybe_add(self, rhs: f32) -> AvailableSpace {
        self.map_definite(|v| v + rhs)
    }
    #[inline]
    fn maybe_sub(self, rhs: f32) -> AvailableSpace {
        self.map_definite(|v| v - rhs)
    }
}

impl MaybeMath<Option<f32>, AvailableSpace> for AvailableSpace {
    #[inline]
    fn maybe_min(self, rhs: Option<f32>) -> AvailableSpace {
        match (self, rhs) {
            (AvailableSpace::Definite(v), Some(r)) => AvailableSpace::Definite(v.min(r)),
            (AvailableSpace::Definite(v), None) => AvailableSpace::Definite(v),
            (_, Some(r)) => AvailableSpace::Definite(r),
            (s, None) => s,
        }
    }
    #[inline]
    fn maybe_max(self, rhs: Option<f32>) -> AvailableSpace {
        rhs.map_or(self, |r| self.maybe_max(r))
    }
    #[inline]
    fn maybe_clamp(self, min: Option<f32>, max: Option<f32>) -> AvailableSpace {
        self.map_definite(|v| v.maybe_clamp(min, max))
    }
    #[inline]
    fn maybe_add(self, rhs: Option<f32>) -> AvailableSpace {
        rhs.map_or(self, |r| self.maybe_add(r))
    }
    #[inline]
    fn maybe_sub(self, rhs: Option<f32>) -> AvailableSpace {
        rhs.map_or(self, |r| self.maybe_sub(r))
    }
}

impl<In: Copy, Out, T: MaybeMath<In, Out>> MaybeMath<Size<In>, Size<Out>> for Size<T> {
    #[inline]
    fn maybe_min(self, rhs: Size<In>) -> Size<Out> {
        Size { width: self.width.maybe_min(rhs.width), height: self.height.maybe_min(rhs.height) }
    }
    #[inline]
    fn maybe_max(self, rhs: Size<In>) -> Size<Out> {
        Size { width: self.width.maybe_max(rhs.width), height: self.height.maybe_max(rhs.height) }
    }
    #[inline]
    fn maybe_clamp(self, min: Size<In>, max: Size<In>) -> Size<Out> {
        Size {
            width: self.width.maybe_clamp(min.width, max.width),
            height: self.height.maybe_clamp(min.height, max.height),
        }
    }
    #[inline]
    fn maybe_add(self, rhs: Size<In>) -> Size<Out> {
        Size { width: self.width.maybe_add(rhs.width), height: self.height.maybe_add(rhs.height) }
    }
    #[inline]
    fn maybe_sub(self, rhs: Size<In>) -> Size<Out> {
        Size { width: self.width.maybe_sub(rhs.width), height: self.height.maybe_sub(rhs.height) }
    }
}

