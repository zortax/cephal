//! Re-placing one absolutely positioned child without re-running its container.
//!
//! An absolutely positioned box never affects its container's size or in-flow siblings, so a
//! change confined to it only needs the geometry the container used last time, which the
//! container records. The container's overflow rectangle may still move, in which case the
//! caller falls back to a full re-run.

use crate::compute::common::overflow::union;
use crate::geometry::{Point, Rect, Size};
use crate::style::{Direction, Position};
use crate::tree::{CacheAccess, LayoutOutput, LayoutTree, NodeId};

/// Geometry a container used when placing its absolutely positioned children.
#[derive(Clone, Debug, PartialEq)]
pub enum AbsoluteGeometry {
    Block {
        area_size: Size<f32>,
        area_offset: Point<f32>,
        direction: Direction,
        is_scroll_container: bool,
    },
    #[cfg(feature = "flex")]
    Flex(crate::compute::flex::AbsoluteFlexContext),
    #[cfg(feature = "grid")]
    Grid(crate::compute::grid::AbsoluteGridContext),
}

/// What a container recorded about its absolutely positioned children on its last layout.
#[derive(Clone, Debug, PartialEq)]
pub struct AbsoluteContext {
    pub geometry: AbsoluteGeometry,
    /// Each absolute child's overflow contribution, in child order.
    pub contributions: Vec<(NodeId, Rect<f32>)>,
}

impl AbsoluteContext {
    pub fn new(geometry: AbsoluteGeometry) -> Self {
        Self { geometry, contributions: Vec::new() }
    }

    pub fn is_scroll_container(&self) -> bool {
        match &self.geometry {
            AbsoluteGeometry::Block { is_scroll_container, .. } => *is_scroll_container,
            #[cfg(feature = "flex")]
            AbsoluteGeometry::Flex(c) => c.is_scroll_container,
            #[cfg(feature = "grid")]
            AbsoluteGeometry::Grid(c) => c.is_scroll_container,
        }
    }
}

/// Outcome of a fast re-placement.
pub enum Reposition {
    /// The child was placed; carries the container's updated answer.
    Done(LayoutOutput),
    /// The container must re-run.
    NeedsParent,
}

/// Re-places `child` inside `parent` using recorded state; `parent_output` is the parent's cached
/// final answer, whose overflow rectangle is recomputed from the recorded contributions.
///
/// Costs the child's own layout plus one pass over the container's absolute children; the
/// container's in-flow children are never visited.
pub fn reposition_absolute_child<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    parent: NodeId,
    child: NodeId,
    parent_output: &LayoutOutput,
) -> Reposition {
    let Some(mut context) = tree.absolute_context(parent) else { return Reposition::NeedsParent };
    let child_style = tree.style(child);
    if child_style.position != Position::Absolute || child_style.generates_no_box() {
        return Reposition::NeedsParent;
    }
    // Block containers place inset-less axes at the static position, which only the flow knows.
    if matches!(context.geometry, AbsoluteGeometry::Block { .. }) {
        let inset = child_style.inset;
        if (inset.left.is_auto() && inset.right.is_auto()) || (inset.top.is_auto() && inset.bottom.is_auto()) {
            return Reposition::NeedsParent;
        }
    }
    let Some(slot) = context.contributions.iter().position(|(c, _)| *c == child) else { return Reposition::NeedsParent };
    // The container wrote the child's `order` last time; the child list has not changed since.
    let Some(order) = tree.unrounded_layout(child).map(|l| l.order) else { return Reposition::NeedsParent };

    let contribution = match &context.geometry {
        AbsoluteGeometry::Block { area_size, area_offset, direction, is_scroll_container } => {
            let cs = tree.style(child);
            let (overflow, contain, scrollbar_width) = (cs.overflow, cs.contain, cs.scrollbar_width);
            crate::compute::block::layout_absolute_block_item(
                tree,
                child,
                order,
                Point::ZERO,
                overflow,
                contain,
                scrollbar_width,
                *area_size,
                *area_offset,
                *direction,
                *is_scroll_container,
            )
        }
        #[cfg(feature = "flex")]
        AbsoluteGeometry::Flex(ctx) => crate::compute::flex::layout_absolute_flex_child(tree, child, order, ctx),
        #[cfg(feature = "grid")]
        AbsoluteGeometry::Grid(ctx) => crate::compute::grid::reposition_absolute_grid_child(tree, parent, child, order, ctx),
    };
    context.contributions[slot].1 = contribution;
    let absolute_overflow = context.contributions.iter().fold(Rect::ZERO, |acc, (_, r)| union(acc, *r));
    tree.set_absolute_context(parent, context);
    Reposition::Done(parent_output.with_overflow(parent_output.inflow_overflow_rect, absolute_overflow))
}
