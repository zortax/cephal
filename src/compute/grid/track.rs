//! One track or gutter during track sizing.

use crate::style::{LengthKind, LengthPercentage, MaxTrackSizingFunction, MinTrackSizingFunction};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GridTrackKind {
    Track,
    Gutter,
}

#[derive(Clone, Debug)]
pub(super) struct GridTrack {
    pub kind: GridTrackKind,
    pub is_collapsed: bool,
    pub min: MinTrackSizingFunction,
    pub max: MaxTrackSizingFunction,
    pub offset: f32,
    pub base_size: f32,
    pub growth_limit: f32,
    pub content_alignment_adjustment: f32,
    pub item_incurred_increase: f32,
    pub base_size_planned_increase: f32,
    pub growth_limit_planned_increase: f32,
    pub infinitely_growable: bool,
}

impl GridTrack {
    const fn with_kind(kind: GridTrackKind, min: MinTrackSizingFunction, max: MaxTrackSizingFunction) -> Self {
        Self {
            kind,
            is_collapsed: false,
            min,
            max,
            offset: 0.0,
            base_size: 0.0,
            growth_limit: 0.0,
            content_alignment_adjustment: 0.0,
            item_incurred_increase: 0.0,
            base_size_planned_increase: 0.0,
            growth_limit_planned_increase: 0.0,
            infinitely_growable: false,
        }
    }
    #[inline]
    pub const fn new(min: MinTrackSizingFunction, max: MaxTrackSizingFunction) -> Self {
        Self::with_kind(GridTrackKind::Track, min, max)
    }
    #[inline]
    pub const fn gutter(size: LengthPercentage) -> Self {
        Self::with_kind(GridTrackKind::Gutter, MinTrackSizingFunction(size.0), MaxTrackSizingFunction(size.0))
    }
    #[inline]
    pub fn collapse(&mut self) {
        self.is_collapsed = true;
        self.min = MinTrackSizingFunction::ZERO;
        self.max = MaxTrackSizingFunction::ZERO;
    }
    #[inline]
    pub fn is_flexible(&self) -> bool {
        self.max.is_fr()
    }
    #[inline]
    pub fn uses_percentage(&self) -> bool {
        self.min.uses_percentage() || self.max.uses_percentage()
    }
    #[inline]
    pub fn has_intrinsic_sizing_function(&self) -> bool {
        self.min.is_intrinsic() || self.max.is_intrinsic()
    }
    #[inline]
    pub fn fit_content_limit(&self, axis_available_grid_space: Option<f32>) -> f32 {
        match self.max.kind() {
            LengthKind::FitContentPx(px) => px,
            LengthKind::FitContentPercent(f) => axis_available_grid_space.map_or(f32::INFINITY, |s| s * f),
            _ => f32::INFINITY,
        }
    }
    #[inline]
    pub fn fit_content_limited_growth_limit(&self, axis_available_grid_space: Option<f32>) -> f32 {
        self.growth_limit.min(self.fit_content_limit(axis_available_grid_space))
    }
    #[inline]
    pub fn flex_factor(&self) -> f32 {
        if self.max.is_fr() { self.max.value() } else { 0.0 }
    }
}
