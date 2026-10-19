//! Spineless invalidation: dirty nodes are visited deepest first, straight from a queue,
//! and dirtiness stops propagating as soon as a node's answers come back unchanged.

mod queue;

use crate::compute::oof::{OOF_ABSOLUTE, OOF_FIXED, position_bit};
use crate::geometry::Line;
use crate::tree::{CacheAccess, LayoutInput, LayoutOutput, LayoutTree, NodeId, RunMode};
use queue::DepthQueue;

pub const DIRTY: u8 = 1;
pub const QUEUED: u8 = 2;
/// A descendant's layout was rewritten; the rounding pass must descend here.
pub const ROUND_PENDING: u8 = 4;
/// With [`DIRTY`]: only this containing block's out-of-flow pass re-runs (see
/// [`crate::compute::oof`]); its own answers and its in-flow children stand.
pub const OOF_PASS: u8 = 8;
/// Bits above the scheduler's, owned by the tree for per-query facts.
pub const META_MASK: u8 = 0xF0;

/// What the scheduler needs beyond layout: parents, depths and a flag byte per node.
pub trait Incremental: LayoutTree + CacheAccess {
    fn parent(&self, node: NodeId) -> Option<NodeId>;
    fn depth(&self, node: NodeId) -> u32;
    fn flags(&self, node: NodeId) -> u8;
    fn set_flags(&mut self, node: NodeId, flags: u8);

    /// Whether `node` is laid out by a containing block's out-of-flow pass rather than its parent.
    #[inline]
    fn is_out_of_flow(&self, node: NodeId) -> bool {
        let style = self.style(node);
        style.position.is_out_of_flow() && !style.generates_no_box() && self.parent(node).is_some()
    }

    /// The containing block of the out-of-flow `node`; `None` for the initial containing block.
    fn containing_block(&self, node: NodeId) -> Option<NodeId> {
        let bit = position_bit(self.style(node).position);
        self.claiming_block(self.parent(node)?, bit)
    }

    /// The first box claiming candidates of `bit` among `from` and where candidates leaving each
    /// box go: its parent, or for an out-of-flow box its containing block. `None` for the initial
    /// containing block.
    fn claiming_block(&self, from: NodeId, bit: u8) -> Option<NodeId> {
        let mut at = from;
        loop {
            if self.containing_block_claims(at) & bit != 0 {
                return Some(at);
            }
            at = if self.is_out_of_flow(at) { self.containing_block(at)? } else { self.parent(at)? };
        }
    }
}

#[derive(Debug, Default)]
pub struct Scheduler {
    queue: DepthQueue,
    scratch: Vec<(LayoutInput, LayoutOutput, u8)>,
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
        tree.set_flags(node, (flags | DIRTY | QUEUED) & !OOF_PASS);
    }

    /// Marks the containing block `node` to re-run only its out-of-flow pass.
    pub fn mark_oof_pass<T: Incremental + ?Sized>(&mut self, tree: &mut T, node: NodeId) {
        let flags = tree.flags(node);
        if flags & DIRTY != 0 {
            return;
        }
        if flags & QUEUED == 0 {
            self.queue.push(tree.depth(node), node);
        }
        tree.set_flags(node, flags | DIRTY | QUEUED | OOF_PASS);
    }

    /// Marks the boxes claiming the out-of-flow candidates of `bits` that leave `node`.
    pub fn mark_claiming_blocks<T: Incremental + ?Sized>(&mut self, tree: &mut T, node: NodeId, bits: u8) {
        for bit in [OOF_ABSOLUTE, OOF_FIXED] {
            if bits & bit == 0 {
                continue;
            }
            let next = if tree.is_out_of_flow(node) { tree.containing_block(node) } else { tree.parent(node) };
            if let Some(cb) = next.and_then(|n| tree.claiming_block(n, bit)) {
                self.mark_oof_pass(tree, cb);
            }
        }
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
            tree.set_flags(node, flags & !(DIRTY | QUEUED | OOF_PASS));
            if flags & DIRTY == 0 {
                continue;
            }
            let parent = tree.parent(node);
            let parent_reruns = parent.map_or(root_query_changed, |p| tree.flags(p) & (DIRTY | OOF_PASS) == DIRTY);
            if parent_reruns {
                // The parent re-runs anyway; nothing to compare against. An out-of-flow node is
                // laid out again by its containing block, which the parent's re-run reaches.
                tree.cache_mut(node).clear();
                continue;
            }
            // Laid out by its containing block last time: hidden layouts are never cached.
            let placed_by_containing_block = tree.is_out_of_flow(node) && tree.cache(node).final_output().is_some();
            let changed = if flags & OOF_PASS != 0 {
                match self.oof_pass(tree, node) {
                    Some(changed) => changed,
                    None => self.recompute(tree, node),
                }
            } else {
                self.recompute(tree, node)
            };
            if changed {
                if placed_by_containing_block {
                    // Only its containing block asks it anything.
                    if let Some(cb) = tree.containing_block(node) {
                        self.mark_oof_pass(tree, cb);
                    }
                } else {
                    match parent {
                        Some(p) => self.mark(tree, p),
                        None => root_changed = true,
                    }
                }
            } else if let Some(bits) = tree.cache(node).final_output().map(|o| o.oof)
                && bits != 0
            {
                // Static positions below may have moved without this answer changing.
                self.mark_claiming_blocks(tree, node, bits);
            }
        }
        root_changed
    }

    /// Re-runs `node`'s out-of-flow pass from its record; whether its answer changed, or `None`
    /// when nothing is recorded to run it from.
    fn oof_pass<T: Incremental + ?Sized>(&mut self, tree: &mut T, node: NodeId) -> Option<bool> {
        let old = tree.cache(node).final_output()?;
        let mut context = tree.absolute_context(node)?.clone();
        let claims = tree.containing_block_claims(node);
        let (absolute, bits) = crate::compute::oof::run_pass(tree, node, &mut context, claims);
        tree.set_absolute_context(node, context);
        let mut new = old.with_overflow(old.inflow_overflow_rect, absolute);
        new.oof = bits;
        let changed = answer_changed(tree, node, RunMode::PerformLayout, new, old);
        tree.cache_mut(node).set_final_output(new);
        Some(changed)
    }

    /// Recomputes `node`'s cached answers in place; true when any may differ from before.
    fn recompute<T: Incremental + ?Sized>(&mut self, tree: &mut T, node: NodeId) -> bool {
        self.scratch.clear();
        let evicted = tree.cache_mut(node).had_eviction();
        tree.cache_mut(node).drain_observed(&mut self.scratch);
        // Nothing cached (hidden, never laid out, or already cleared): only the parent can tell.
        if self.scratch.is_empty() {
            return true;
        }
        // Answers given inside a shared block formatting context depend on siblings; let the
        // parent recompute them with the real context.
        let context_bound = self.scratch.iter().any(|(input, _, _)| {
            input.context_key != 0 || input.vertical_margins_are_collapsible != Line::FALSE
        });
        if context_bound {
            self.scratch.clear();
            return true;
        }
        let mut changed = evicted;
        for i in 0..self.scratch.len() {
            let (input, old, old_observed) = self.scratch[i];
            let new = crate::compute::compute_child_layout(tree, node, input, None);
            if answer_changed(tree, node, input.run_mode, new, old) {
                changed = true;
            }
            // An answer that now tells min-content from max-content no longer stands for the
            // questions it answered on axes it did not observe before.
            let observed = tree.cache(node).observed(&input).unwrap_or(crate::compute::OBSERVED_ALL);
            if observed & !old_observed != 0 {
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
