//! Spineless invalidation: dirty nodes are visited deepest first, straight from a queue,
//! and dirtiness stops propagating as soon as a node's answers come back unchanged.

mod queue;

use crate::compute::common::overflow::{scrollable_overflow_contribution, union};
use crate::compute::oof::{OOF_ABSOLUTE, OOF_FIXED, position_bit};
use crate::geometry::{Line, Point, Rect};
use crate::style::{Direction, Display};
use crate::tree::{CacheAccess, LayoutInput, LayoutOutput, LayoutTree, NodeId, RunMode};
use queue::DepthQueue;
use std::collections::HashSet;

type FxHashSet<T> = HashSet<T, crate::hash::FxBuild>;

pub const DIRTY: u8 = 1;
pub const QUEUED: u8 = 2;
/// A descendant's layout was rewritten; the rounding pass must descend here.
pub const ROUND_PENDING: u8 = 4;
/// With [`DIRTY`]: only this containing block's out-of-flow pass re-runs (see
/// [`crate::compute::oof`]), and its in-flow overflow is gathered again when the scheduler holds
/// the node in its in-flow set; its other answers and its in-flow children stand.
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
    /// The initial containing block's pass must re-run (see [`Self::take_initial_containing_block`]).
    initial_containing_block: bool,
}

/// How a node's answers changed, as its parent sees them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Change {
    None,
    /// Only the final layout's overflow rectangles, on axes where they propagate.
    Overflow,
    Full,
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

    /// Marks `node` to gather its in-flow overflow again, recording it in `inflow`: a child's final
    /// answer changed only in its overflow. A pending full recomputation covers it.
    fn mark_inflow<T: Incremental + ?Sized>(&mut self, tree: &mut T, node: NodeId, inflow: &mut FxHashSet<NodeId>) {
        let flags = tree.flags(node);
        if flags & (DIRTY | OOF_PASS) == DIRTY {
            return;
        }
        if flags & QUEUED == 0 {
            self.queue.push(tree.depth(node), node);
        }
        tree.set_flags(node, flags | DIRTY | QUEUED | OOF_PASS);
        inflow.insert(node);
    }

    /// Marks the containing block `cb` to re-run only its out-of-flow pass; `None` is the initial
    /// containing block.
    pub fn mark_containing_block<T: Incremental + ?Sized>(&mut self, tree: &mut T, cb: Option<NodeId>) {
        match cb {
            Some(cb) => self.mark_oof_pass(tree, cb),
            None => self.initial_containing_block = true,
        }
    }

    /// Whether the initial containing block's pass must re-run, and forgets it: pass it to
    /// [`crate::compute::compute_root_layout_incremental`] after [`Self::run`].
    pub fn take_initial_containing_block(&mut self) -> bool {
        core::mem::take(&mut self.initial_containing_block)
    }

    /// Marks the boxes claiming the out-of-flow candidates of `bits` that leave `node`.
    pub fn mark_claiming_blocks<T: Incremental + ?Sized>(&mut self, tree: &mut T, node: NodeId, bits: u8) {
        for bit in [OOF_ABSOLUTE, OOF_FIXED] {
            if bits & bit == 0 {
                continue;
            }
            // Out-of-flow boxes leave for their containing block, others for their parent.
            let next = if tree.is_out_of_flow(node) { tree.containing_block(node) } else { tree.parent(node) };
            let cb = next.and_then(|n| tree.claiming_block(n, bit));
            self.mark_containing_block(tree, cb);
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Recomputes every dirty node bottom-up. Returns whether the root's answer may have changed.
    pub fn run<T: Incremental + ?Sized>(&mut self, tree: &mut T, _root: NodeId, root_query_changed: bool) -> bool {
        let mut root_changed = root_query_changed;
        // Nodes queued with [`OOF_PASS`] whose in-flow overflow must be gathered again.
        let mut inflow_pending = FxHashSet::default();
        while let Some((depth, node)) = self.queue.pop_deepest() {
            let actual_depth = tree.depth(node);
            if actual_depth != depth {
                // Reparented while queued: revisit at its new depth.
                self.queue.push(actual_depth, node);
                continue;
            }
            let flags = tree.flags(node);
            tree.set_flags(node, flags & !(DIRTY | QUEUED | OOF_PASS));
            let inflow = !inflow_pending.is_empty() && inflow_pending.remove(&node);
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
            let placed_by_containing_block = tree.is_out_of_flow(node) && !in_hidden_subtree(tree, node);
            // Whether boxes below may have moved: static positions recorded in this subtree, or
            // candidates laid out from them.
            let (change, moved) = if flags & OOF_PASS != 0 {
                match self.partial_pass(tree, node, inflow) {
                    Some(result) => result,
                    None => (self.recompute(tree, node), true),
                }
            } else {
                (self.recompute(tree, node), true)
            };
            if change != Change::None {
                if placed_by_containing_block {
                    // Only its containing block asks it anything.
                    let cb = tree.containing_block(node);
                    self.mark_containing_block(tree, cb);
                } else {
                    match parent {
                        // The parent's in-flow layout reads a child's overflow only to gather its
                        // own; it need not run again for that.
                        Some(p) if change == Change::Overflow => self.mark_inflow(tree, p, &mut inflow_pending),
                        Some(p) => self.mark(tree, p),
                        None => {
                            // What the root passes on may have moved with it; its overflow is not
                            // read by the initial containing block's pass.
                            root_changed = true;
                            if change == Change::Full {
                                self.initial_containing_block = true;
                            }
                        }
                    }
                }
            }
            if change != Change::Full
                && moved
                && let Some(bits) = tree.cache(node).final_output().map(|o| o.oof)
                && bits != 0
            {
                // Static positions below may have moved without this answer changing.
                self.mark_claiming_blocks(tree, node, bits);
            }
        }
        root_changed
    }

    /// Re-runs `node`'s out-of-flow pass from its record and, with `inflow`, gathers its in-flow
    /// overflow again from its children's layouts; how its answer changed and whether it laid out
    /// candidates, or `None` when nothing is recorded to run it from.
    fn partial_pass<T: Incremental + ?Sized>(&mut self, tree: &mut T, node: NodeId, inflow: bool) -> Option<(Change, bool)> {
        let old = tree.cache(node).final_output()?;
        let inflow_rect = if inflow {
            let gathered = gather_inflow_overflow(tree, node, &old);
            #[cfg(feature = "stats")]
            crate::compute::stats::bump(if gathered.is_some() { &crate::compute::stats::INFLOW_GATHERED } else { &crate::compute::stats::INFLOW_FALLBACK });
            gathered?
        } else {
            old.inflow_overflow_rect
        };
        let claims = tree.containing_block_claims(node);
        // Only containing blocks are asked for their out-of-flow pass, so a box claiming nothing
        // is here for its in-flow overflow: it lays out no candidate (no overflow of its own), and
        // the bits of its answer are those its unchanged record gave.
        let (absolute, bits) = if inflow && claims == 0 {
            (Rect::ZERO, old.oof)
        } else {
            let mut context = tree.absolute_context(node)?.clone();
            let result = crate::compute::oof::run_pass(tree, node, &mut context, claims);
            tree.set_absolute_context(node, context);
            result
        };
        let mut new = old.with_overflow(inflow_rect, absolute);
        new.oof = bits;
        let change = answer_changed(tree, node, RunMode::PerformLayout, new, old);
        tree.cache_mut(node).set_final_output(new);
        Some((change, !inflow || claims != 0))
    }

    /// Recomputes `node`'s cached answers in place; how they may differ from before.
    fn recompute<T: Incremental + ?Sized>(&mut self, tree: &mut T, node: NodeId) -> Change {
        self.scratch.clear();
        let evicted = tree.cache_mut(node).had_eviction();
        tree.cache_mut(node).drain_observed(&mut self.scratch);
        // Nothing cached (hidden, never laid out, or already cleared): only the parent can tell.
        if self.scratch.is_empty() {
            return Change::Full;
        }
        // Answers given inside a shared block formatting context depend on siblings; let the
        // parent recompute them with the real context.
        let context_bound = self.scratch.iter().any(|(input, _, _)| {
            input.context_key != 0 || input.vertical_margins_are_collapsible != Line::FALSE
        });
        if context_bound {
            self.scratch.clear();
            return Change::Full;
        }
        let mut change = if evicted { Change::Full } else { Change::None };
        for i in 0..self.scratch.len() {
            let (input, old, old_observed) = self.scratch[i];
            let new = crate::compute::compute_child_layout(tree, node, input, None);
            change = change.max(answer_changed(tree, node, input.run_mode, new, old));
            // An answer that now tells min-content from max-content no longer stands for the
            // questions it answered on axes it did not observe before.
            let observed = tree.cache(node).observed(&input).unwrap_or(crate::compute::OBSERVED_ALL);
            if observed & !old_observed != 0 {
                change = Change::Full;
            }
        }
        self.scratch.clear();
        // A parent that asked this node questions speculatively depends on answers no entry
        // records, which may have changed with the recorded ones unchanged, or with only their
        // overflow changed.
        if tree.cache(node).has_unstored() { Change::Full } else { change }
    }
}

/// Whether an ancestor of `node` generates no box, so `node` is laid out hidden by its parent.
fn in_hidden_subtree<T: Incremental + ?Sized>(tree: &T, node: NodeId) -> bool {
    let mut at = tree.parent(node);
    while let Some(n) = at {
        if tree.style(n).generates_no_box() {
            return true;
        }
        at = tree.parent(n);
    }
    false
}

/// How the parent could observe `new` differing from `old`.
///
/// A final answer whose overflow rectangle moved only on clipped axes is invisible to the parent,
/// which nevertheless stores that rectangle in the node's layout; it is patched here so the
/// layout still equals a cold computation. So it is when the parent sees only the overflow move
/// ([`Change::Overflow`]), as the parent then only gathers its own overflow again.
fn answer_changed<T: Incremental + ?Sized>(tree: &mut T, node: NodeId, run_mode: RunMode, new: LayoutOutput, old: LayoutOutput) -> Change {
    // Size answers leave their overflow and out-of-flow bits unspecified: a cached one has none.
    let (new, old) = if run_mode == RunMode::ComputeSize { (new.size_answer(), old.size_answer()) } else { (new, old) };
    if new == old {
        return Change::None;
    }
    let style = tree.style(node);
    let change = if new.same_for_parent(&old, style.overflow, style.contain) {
        Change::None
    } else if run_mode == RunMode::PerformLayout && new.same_except_overflow(&old) {
        Change::Overflow
    } else {
        return Change::Full;
    };
    if run_mode == RunMode::PerformLayout
        && let Some(mut layout) = tree.unrounded_layout(node)
        && layout.scrollable_overflow_rect != new.scrollable_overflow_rect
    {
        layout.scrollable_overflow_rect = new.scrollable_overflow_rect;
        tree.set_unrounded_layout(node, &layout);
    }
    change
}

/// `node`'s in-flow overflow gathered from its children's stored layouts, as its layout algorithm
/// gathers it while placing them; `None` where that is not provably the same value.
///
/// Block, flex and grid containers all fold [`scrollable_overflow_contribution`] over their in-flow
/// children from [`Rect::ZERO`], each child's location taken relative to the padding box (mirrored
/// in right-to-left containers) from the layout the container stored for it. A scroll container
/// then adds its end padding, which is left to its algorithm. The fold is a minimum or maximum per
/// side, so its order matters only for `-0.0` against `0.0` (and NaN), which send the node back
/// to its algorithm.
fn gather_inflow_overflow<T: Incremental + ?Sized>(tree: &T, node: NodeId, old: &LayoutOutput) -> Option<Rect<f32>> {
    let style = tree.style(node);
    if style.is_scroll_container() || !matches!(style.display, Display::Block | Display::FlowRoot | Display::Flex | Display::Grid) {
        return None;
    }
    // The border as the algorithm resolved it, from the question it answered.
    let input = tree.cache(node).final_input()?;
    let calc = |id, basis| tree.resolve_calc(id, basis);
    let border = style.border.map(|b| b.resolve_or_zero(input.parent_size.width, &calc));
    let rtl = style.direction == Direction::Rtl;
    let width = old.size.width;
    let mut rect = Rect::ZERO;
    for child in tree.children(node) {
        let child_style = tree.style(child);
        if child_style.generates_no_box() || child_style.position.is_out_of_flow() {
            continue;
        }
        let layout = tree.unrounded_layout(child)?;
        let location = if rtl {
            Point { x: width - (layout.location.x + layout.size.width) - border.right, y: layout.location.y - border.top }
        } else {
            Point { x: layout.location.x - border.left, y: layout.location.y - border.top }
        };
        let contribution = scrollable_overflow_contribution(
            location,
            layout.size,
            layout.scrollable_overflow_rect,
            child_style.overflow,
            child_style.contain,
            false,
        );
        let ambiguous = |v: f32| v.is_nan() || v.to_bits() == (-0.0f32).to_bits();
        if ambiguous(contribution.left) || ambiguous(contribution.right) || ambiguous(contribution.top) || ambiguous(contribution.bottom) {
            return None;
        }
        rect = union(rect, contribution);
    }
    Some(rect)
}
