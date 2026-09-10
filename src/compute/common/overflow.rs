//! Scrollable overflow contributions.

use crate::geometry::{Point, Rect, Size};
use crate::style::{Contain, Overflow};

/// The rectangle a child adds to its parent's scrollable overflow, in scroll-origin coordinates.
///
/// A scroll container or contained box contributes only its border box; `clip` clips its own
/// axis; `visible` propagates the child's own overflow. Boxes wholly before the scroll origin of a
/// scroll container are unreachable and contribute nothing.
#[inline]
pub fn scrollable_overflow_contribution(
    location: Point<f32>,
    size: Size<f32>,
    child_overflow: Rect<f32>,
    overflow: Point<Overflow>,
    contain: Contain,
    parent_is_scroll_container: bool,
) -> Rect<f32> {
    let is_scroll_container = overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
    let contained = contain.contains_scrollable_overflow();
    let propagates_x = !is_scroll_container && !contained && overflow.x == Overflow::Visible;
    let propagates_y = !is_scroll_container && !contained && overflow.y == Overflow::Visible;
    let end_x = if propagates_x { size.width.max(child_overflow.right) } else { size.width };
    let end_y = if propagates_y { size.height.max(child_overflow.bottom) } else { size.height };
    if end_x <= 0.0 || end_y <= 0.0 {
        return Rect::ZERO;
    }
    let start_x = if propagates_x { child_overflow.left.min(0.0) } else { 0.0 };
    let start_y = if propagates_y { child_overflow.top.min(0.0) } else { 0.0 };
    let contribution = Rect {
        left: location.x + start_x,
        right: location.x + end_x,
        top: location.y + start_y,
        bottom: location.y + end_y,
    };
    if parent_is_scroll_container && (contribution.right <= 0.0 || contribution.bottom <= 0.0) {
        Rect::ZERO
    } else {
        contribution
    }
}

/// Union of two overflow rects; both always contain the origin.
#[inline]
pub fn union(a: Rect<f32>, b: Rect<f32>) -> Rect<f32> {
    Rect { left: a.left.min(b.left), right: a.right.max(b.right), top: a.top.min(b.top), bottom: a.bottom.max(b.bottom) }
}
