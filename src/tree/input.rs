//! The question a parent asks a child, and the answer it gets back.

use crate::geometry::{AbsoluteAxis, AvailableSpace, Line, Point, Rect, Size};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RunMode {
    /// Compute and store the whole subtree's layout.
    PerformLayout,
    /// Only the size is needed.
    ComputeSize,
    /// The node is inside `display: none`.
    PerformHiddenLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SizingMode {
    /// Ignore the node's own size properties; the caller has applied them.
    ContentSize,
    /// Apply the node's own size properties.
    InherentSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RequestedAxis {
    Horizontal,
    Vertical,
    Both,
}

impl From<AbsoluteAxis> for RequestedAxis {
    #[inline]
    fn from(a: AbsoluteAxis) -> Self {
        match a {
            AbsoluteAxis::Horizontal => Self::Horizontal,
            AbsoluteAxis::Vertical => Self::Vertical,
        }
    }
}

/// Positive and negative parts of a collapsible margin.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CollapsibleMarginSet {
    pub positive: f32,
    pub negative: f32,
}

impl CollapsibleMarginSet {
    pub const ZERO: Self = Self { positive: 0.0, negative: 0.0 };

    #[inline]
    pub fn from_margin(margin: f32) -> Self {
        if margin >= 0.0 { Self { positive: margin, negative: 0.0 } } else { Self { positive: 0.0, negative: margin } }
    }
    // Margins are never NaN, so plain comparisons replace `f32::max`/`min` and their NaN handling.
    #[inline]
    pub fn collapse_with_margin(self, margin: f32) -> Self {
        Self {
            positive: if margin > self.positive { margin } else { self.positive },
            negative: if margin < self.negative { margin } else { self.negative },
        }
    }
    #[inline]
    pub fn collapse_with_set(self, other: Self) -> Self {
        Self {
            positive: if other.positive > self.positive { other.positive } else { self.positive },
            negative: if other.negative < self.negative { other.negative } else { self.negative },
        }
    }
    #[inline]
    pub fn resolve(self) -> f32 {
        self.positive + self.negative
    }
}

/// Everything a layout algorithm needs from its caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutInput {
    pub run_mode: RunMode,
    pub sizing_mode: SizingMode,
    pub axis: RequestedAxis,
    /// Border-box size the caller has already decided on.
    pub known_dimensions: Size<Option<f32>>,
    /// `false` when a known dimension came from the child's own content (CSS "indefinite").
    pub known_dimensions_are_definite: Size<bool>,
    /// Percentage basis.
    pub parent_size: Size<Option<f32>>,
    /// Space for the border box: the parent has already subtracted the child's margins (`auto`
    /// ones counting as zero), and no algorithm subtracts a node's own margins again.
    pub available_space: Size<AvailableSpace>,
    /// Whether the block-start/end margins may collapse with the parent's.
    pub vertical_margins_are_collapsible: Line<bool>,
    /// Summary of surrounding formatting-context state the answer depends on (floats); `0` when none.
    pub context_key: u32,
}

impl LayoutInput {
    pub const HIDDEN: Self = Self {
        run_mode: RunMode::PerformHiddenLayout,
        sizing_mode: SizingMode::InherentSize,
        axis: RequestedAxis::Both,
        known_dimensions: Size::NONE,
        known_dimensions_are_definite: Size::TRUE,
        parent_size: Size::NONE,
        available_space: Size::MAX_CONTENT,
        vertical_margins_are_collapsible: Line::FALSE,
        context_key: 0,
    };
}

/// First and last baselines, measured from the border-box top.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Baselines {
    pub first: Option<f32>,
    pub last: Option<f32>,
}

impl Baselines {
    pub const NONE: Self = Self { first: None, last: None };
    #[inline]
    pub fn from_first(first: Option<f32>) -> Self {
        Self { first, last: first }
    }
}

/// What a layout algorithm returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutOutput {
    /// Border-box size.
    pub size: Size<f32>,
    /// Scrollable overflow relative to the scroll origin (padding-box corner).
    pub scrollable_overflow_rect: Rect<f32>,
    /// The part of the overflow contributed by in-flow content only.
    pub inflow_overflow_rect: Rect<f32>,
    pub baselines: Baselines,
    pub top_margin: CollapsibleMarginSet,
    pub bottom_margin: CollapsibleMarginSet,
    pub margins_can_collapse_through: bool,
    /// Out-of-flow boxes whose containing block is above this node exist in its subtree, as a set
    /// of [`crate::compute::oof::OOF_ABSOLUTE`] and [`crate::compute::oof::OOF_FIXED`]; set on
    /// final-layout answers only. The containing block finds them through recorded
    /// [`crate::tree::AbsoluteContext`]s.
    pub oof: u8,
}

impl LayoutOutput {
    /// Whether a parent could tell `self` from `old`, given this node's overflow styles.
    ///
    /// Parents read a child's overflow rectangle only on axes where it propagates (visible
    /// overflow, no containment); on clipped or scrolled axes only the size matters.
    pub fn same_for_parent(&self, old: &Self, overflow: Point<crate::style::Overflow>, contain: crate::style::Contain) -> bool {
        if !self.same_except_overflow(old) {
            return false;
        }
        let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
        let contained = contain.contains_scrollable_overflow();
        let (a, b) = (self.scrollable_overflow_rect, old.scrollable_overflow_rect);
        let same_x = a.left == b.left && a.right == b.right;
        let same_y = a.top == b.top && a.bottom == b.bottom;
        let propagates_x = !is_scroll_container && !contained && overflow.x == crate::style::Overflow::Visible;
        let propagates_y = !is_scroll_container && !contained && overflow.y == crate::style::Overflow::Visible;
        (same_x || !propagates_x) && (same_y || !propagates_y)
    }

    /// Whether `self` equals `old` apart from the overflow rectangles.
    #[inline]
    pub fn same_except_overflow(&self, old: &Self) -> bool {
        self.size == old.size
            && self.baselines == old.baselines
            && self.top_margin == old.top_margin
            && self.bottom_margin == old.bottom_margin
            && self.margins_can_collapse_through == old.margins_can_collapse_through
            && self.oof == old.oof
    }

    /// What `self` specifies as the answer to a `ComputeSize` question: the size, baselines and
    /// collapsible margins. The overflow rectangles and out-of-flow bits of a size answer are
    /// unspecified, since no caller of a size question reads them; here they are zero, as in a
    /// cached size answer. Caches keep only this much.
    #[inline]
    pub const fn size_answer(self) -> Self {
        Self { scrollable_overflow_rect: Rect::ZERO, inflow_overflow_rect: Rect::ZERO, oof: 0, ..self }
    }

    pub const HIDDEN: Self = Self::from_outer_size(Size::ZERO);
    pub const DEFAULT: Self = Self::HIDDEN;

    #[inline]
    pub const fn from_outer_size(size: Size<f32>) -> Self {
        Self {
            size,
            scrollable_overflow_rect: Rect::ZERO,
            inflow_overflow_rect: Rect::ZERO,
            baselines: Baselines::NONE,
            top_margin: CollapsibleMarginSet::ZERO,
            bottom_margin: CollapsibleMarginSet::ZERO,
            margins_can_collapse_through: false,
            oof: 0,
        }
    }
    #[inline]
    pub fn from_sizes(size: Size<f32>, scrollable_overflow_rect: Rect<f32>) -> Self {
        Self { size, scrollable_overflow_rect, inflow_overflow_rect: scrollable_overflow_rect, ..Self::HIDDEN }
    }
    #[inline]
    pub fn from_sizes_and_baselines(size: Size<f32>, scrollable_overflow_rect: Rect<f32>, baselines: Baselines) -> Self {
        Self { size, scrollable_overflow_rect, inflow_overflow_rect: scrollable_overflow_rect, baselines, ..Self::HIDDEN }
    }
    /// Sets the overflow as `inflow ∪ absolute`, remembering the in-flow part.
    #[inline]
    pub fn with_overflow(mut self, inflow: Rect<f32>, absolute: Rect<f32>) -> Self {
        self.inflow_overflow_rect = inflow;
        self.scrollable_overflow_rect = crate::compute::common::overflow::union(inflow, absolute);
        self
    }
}
