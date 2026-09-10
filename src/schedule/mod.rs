//! Spineless invalidation: dirty nodes are visited deepest first, straight from a queue,
//! and dirtiness stops propagating as soon as a node's answers come back unchanged.

mod queue;

use crate::geometry::Line;
use crate::tree::{CacheAccess, LayoutInput, LayoutOutput, LayoutTree, NodeId, RunMode};
use queue::DepthQueue;

pub const DIRTY: u8 = 1;
pub const QUEUED: u8 = 2;
/// A descendant's layout was rewritten; the rounding pass must descend here.
pub const ROUND_PENDING: u8 = 4;
/// Only this absolutely positioned node changed; its container need not re-run.
pub const ABS_ONLY: u8 = 8;
/// Bits above the scheduler's, owned by the tree for per-query facts.
pub const META_MASK: u8 = 0xF0;

/// What the scheduler needs beyond layout: parents, depths and a flag byte per node.
pub trait Incremental: LayoutTree + CacheAccess {
    fn parent(&self, node: NodeId) -> Option<NodeId>;
    fn depth(&self, node: NodeId) -> u32;
    fn flags(&self, node: NodeId) -> u8;
    fn set_flags(&mut self, node: NodeId, flags: u8);
}

#[derive(Default)]
pub struct Scheduler {
    queue: DepthQueue,
    scratch: Vec<(LayoutInput, LayoutOutput)>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks `node` for recomputation.
    pub fn mark<T: Incremental + ?Sized>(&mut self, tree: &mut T, node: NodeId) {
        let flags = tree.flags(node);
        if flags & QUEUED == 0 {
            self.queue.push(tree.depth(node), node);
        }
        tree.set_flags(node, (flags | DIRTY | QUEUED) & !ABS_ONLY);
    }

    /// Marks an absolutely positioned `node` whose change cannot affect its siblings.
    pub fn mark_absolute<T: Incremental + ?Sized>(&mut self, tree: &mut T, node: NodeId) {
        let flags = tree.flags(node);
        if flags & DIRTY != 0 {
            return;
        }
        if flags & QUEUED == 0 {
            self.queue.push(tree.depth(node), node);
        }
        tree.set_flags(node, flags | DIRTY | QUEUED | ABS_ONLY);
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Recomputes every dirty node bottom-up. Returns whether the root's answer may have changed.
    pub fn run<T: Incremental + ?Sized>(&mut self, tree: &mut T, _root: NodeId, root_query_changed: bool) -> bool {
        let mut root_changed = root_query_changed;
        while let Some((depth, node)) = self.queue.pop_deepest() {
            let actual_depth = tree.depth(node);
            if actual_depth != depth {
                // Reparented while queued: revisit at its new depth.
                self.queue.push(actual_depth, node);
                continue;
            }
            let flags = tree.flags(node);
            tree.set_flags(node, flags & !(DIRTY | QUEUED | ABS_ONLY));
            if flags & DIRTY == 0 {
                continue;
            }
            let parent = tree.parent(node);
            let parent_dirty = parent.map_or(root_query_changed, |p| tree.flags(p) & DIRTY != 0);
            if parent_dirty {
                // The parent re-runs anyway; nothing to compare against.
                tree.cache_mut(node).clear();
                continue;
            }
            if flags & ABS_ONLY != 0
                && let Some(p) = parent
            {
                self.recompute(tree, node);
                if self.reposition(tree, p, node) {
                    continue;
                }
                self.mark(tree, p);
                continue;
            }
            if self.recompute(tree, node) {
                match parent {
                    Some(p) => self.mark(tree, p),
                    None => root_changed = true,
                }
            }
        }
        root_changed
    }

    /// Re-places an absolutely positioned child; true when the container's answer is intact.
    fn reposition<T: Incremental + ?Sized>(&mut self, tree: &mut T, parent: NodeId, node: NodeId) -> bool {
        let Some(old) = tree.cache(parent).final_output() else { return false };
        match crate::compute::reposition::reposition_absolute_child(tree, parent, node, &old) {
            crate::compute::reposition::Reposition::Done(new) => {
                if answer_changed(tree, parent, RunMode::PerformLayout, new, old) {
                    return false;
                }
                tree.cache_mut(parent).set_final_output(new);
                true
            }
            crate::compute::reposition::Reposition::NeedsParent => false,
        }
    }

    /// Recomputes `node`'s cached answers in place; true when any may differ from before.
    fn recompute<T: Incremental + ?Sized>(&mut self, tree: &mut T, node: NodeId) -> bool {
        self.scratch.clear();
        let evicted = tree.cache_mut(node).had_eviction();
        tree.cache_mut(node).drain_all(&mut self.scratch);
        // Nothing cached (hidden, never laid out, or already cleared): only the parent can tell.
        if self.scratch.is_empty() {
            return true;
        }
        // Answers given inside a shared block formatting context depend on siblings; let the
        // parent recompute them with the real context.
        let context_bound = self.scratch.iter().any(|(input, _)| {
            input.context_key != 0 || input.vertical_margins_are_collapsible != Line::FALSE
        });
        if context_bound {
            self.scratch.clear();
            return true;
        }
        let mut changed = evicted;
        for i in 0..self.scratch.len() {
            let (input, old) = self.scratch[i];
            let new = crate::compute::compute_child_layout(tree, node, input, None);
            if answer_changed(tree, node, input.run_mode, new, old) {
                changed = true;
            }
        }
        self.scratch.clear();
        changed
    }
}

/// Whether the parent could observe `new` differing from `old`.
///
/// A final answer whose overflow rectangle moved only on clipped axes is invisible to the parent,
/// which nevertheless stores that rectangle in the node's layout; it is patched here so the
/// layout still equals a cold computation.
fn answer_changed<T: Incremental + ?Sized>(tree: &mut T, node: NodeId, run_mode: RunMode, new: LayoutOutput, old: LayoutOutput) -> bool {
    if new == old {
        return false;
    }
    let style = tree.style(node);
    if !new.same_for_parent(&old, style.overflow, style.contain) {
        return true;
    }
    if run_mode == RunMode::PerformLayout
        && let Some(mut layout) = tree.unrounded_layout(node)
        && layout.scrollable_overflow_rect != new.scrollable_overflow_rect
    {
        layout.scrollable_overflow_rect = new.scrollable_overflow_rect;
        tree.set_unrounded_layout(node, &layout);
    }
    false
}
