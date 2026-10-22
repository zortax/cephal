//! A worker's view of the shared tree: it writes its own subtree in place and logs the rest.

use super::pool::Pool;
use super::{Config, Log, SyncMeasureFn};
use crate::compute::block::BlockContext;
use crate::owned::Tree;
use crate::owned::node::NONE;
use crate::style::{CalcId, Ident, Style};
use crate::tree::{
    AbsoluteContext, CacheAccess, ChildRequest, DetailedGridInfo, IdentSuffix, Layout, LayoutInput, LayoutOutput,
    LayoutTree, MeasureInput, MeasureOutput, NodeCache, NodeId,
};
use std::sync::Arc;

pub(super) struct Worker<'a, Ctx, F> {
    tree: &'a Arc<Tree<Ctx>>,
    measure: &'a Arc<F>,
    config: Config,
    pool: &'a Pool,
    log: Log,
    speculation: u32,
    observed: u8,
}

impl<'a, Ctx: Send + Sync, F> Worker<'a, Ctx, F> {
    pub fn new(tree: &'a Arc<Tree<Ctx>>, measure: &'a Arc<F>, config: Config, pool: &'a Pool, nodes: usize) -> Self {
        let mut log = Log::default();
        // Workers lay out cold subtrees, whose nodes have no layout yet.
        log.fresh.reserve(nodes);
        Self { tree, measure, config, pool, log, speculation: 0, observed: 0 }
    }

    pub fn into_log(self) -> Log {
        self.log
    }

    /// Records the output of the batch request `r` and the axes it observed.
    pub fn answered(&mut self, r: usize, output: LayoutOutput, observed: u8) {
        self.log.answers.push((r, output, observed));
    }

    #[inline]
    fn index(&self, node: NodeId) -> usize {
        self.tree.index(node)
    }
}

/// A leaf with a context, kept out of line so that inlining `compute_leaf` stays small.
#[inline(never)]
fn measured_leaf<Ctx: Send + Sync + 'static, F: SyncMeasureFn<Ctx>>(
    worker: &mut Worker<'_, Ctx, F>,
    node: NodeId,
    inputs: LayoutInput,
) -> LayoutOutput {
    crate::compute::leaf::compute_leaf_layout(worker, node, inputs)
}

impl<Ctx: Send + Sync + 'static, F: SyncMeasureFn<Ctx>> LayoutTree for Worker<'_, Ctx, F> {
    type ChildIter<'b>
        = crate::owned::node::Children<'b>
    where
        Self: 'b;

    #[inline]
    fn children(&self, node: NodeId) -> Self::ChildIter<'_> {
        self.tree.children(node)
    }
    #[inline]
    fn child_count(&self, node: NodeId) -> usize {
        self.tree.child_count(node)
    }
    #[inline]
    fn child_at(&self, node: NodeId, index: usize) -> NodeId {
        self.tree.children(node).nth(index).expect("child index in range")
    }
    #[inline]
    fn style(&self, node: NodeId) -> &Style {
        self.tree.style(node)
    }
    #[inline]
    fn query_meta(&self, node: NodeId) -> crate::tree::QueryMeta {
        self.tree.query_meta(node)
    }
    #[inline]
    fn style_plan(&self, node: NodeId) -> Option<&crate::compute::plan::StylePlan> {
        self.tree.style_plan(node)
    }
    #[inline]
    fn resolve_calc(&self, id: CalcId, basis: f32) -> f32 {
        self.tree.calc.resolve(id, basis)
    }
    fn suffixed_ident(&self, base: Ident, suffix: IdentSuffix) -> Option<Ident> {
        let name = format!("{}{}", self.tree.ident_name(base), suffix.as_str());
        self.tree.idents.get(&name).copied()
    }
    #[inline]
    fn leaf_is_cheap(&self, node: NodeId) -> bool {
        self.tree.context_slot[self.tree.index(node)] == NONE
    }

    #[inline]
    fn is_measured_leaf(&self, node: NodeId) -> bool {
        self.tree.context_slot[self.tree.index(node)] != NONE
    }

    /// Inlined, so a caller reading part of an empty leaf's answer computes only that part.
    #[inline(always)]
    fn compute_leaf(&mut self, node: NodeId, inputs: LayoutInput, _: Option<&mut BlockContext<'_>>) -> LayoutOutput {
        if self.tree.context_slot[self.tree.index(node)] == NONE {
            return crate::compute::leaf::compute_empty_leaf_layout(self, node, inputs);
        }
        measured_leaf(self, node, inputs)
    }

    fn measure(&mut self, node: NodeId, input: MeasureInput) -> MeasureOutput {
        let i = self.index(node);
        let slot = self.tree.context_slot[i];
        if slot == NONE {
            return MeasureOutput::from_size(input.known_dimensions.unwrap_or(crate::geometry::Size::ZERO));
        }
        let style = self.tree.styles.get(self.tree.style_ids[i]);
        let ctx = self.tree.contexts[slot as usize].as_ref().expect("context slot is live");
        (self.measure)(ctx, input, style)
    }

    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        if self.speculation > 0 {
            return;
        }
        let i = self.index(node);
        // SAFETY: `node` lies in this worker's subtree (see `column`).
        let (slot, touched_gen) = unsafe { (self.tree.unrounded.slot(i), self.tree.touched_gen.slot(i)) };
        // As `Tree::write_unrounded`.
        if *touched_gen == self.tree.generation {
        } else if *touched_gen == 0 {
            *touched_gen = self.tree.generation;
            self.log.fresh.push(i as u32);
        } else if *slot != *layout {
            *touched_gen = self.tree.generation;
            self.log.touched.push((node, *slot));
        }
        *slot = *layout;
    }
    fn unrounded_layout(&self, node: NodeId) -> Option<Layout> {
        Some(*self.tree.unrounded.get(self.index(node)))
    }

    fn set_detailed_grid_info(&mut self, node: NodeId, info: DetailedGridInfo) {
        self.log.detailed.push((self.index(node) as u32, info));
    }
    fn detailed_grid_info(&self, node: NodeId) -> Option<DetailedGridInfo> {
        let i = self.index(node) as u32;
        self.log.detailed(i).or_else(|| self.tree.detailed_grid.get(&i).cloned())
    }

    fn set_absolute_context(&mut self, node: NodeId, context: AbsoluteContext) {
        if self.speculation == 0 {
            self.log.absolute.push((self.index(node) as u32, Some(context)));
        }
    }
    #[inline]
    fn clear_absolute_context(&mut self, node: NodeId) {
        if self.speculation > 0 || (self.tree.absolute_contexts.is_empty() && self.log.absolute.is_empty()) {
            return;
        }
        if self.absolute_context(node).is_some() {
            self.log.absolute.push((self.index(node) as u32, None));
        }
    }
    fn keeps_placements(&self) -> bool {
        true
    }
    fn absolute_context(&self, node: NodeId) -> Option<&AbsoluteContext> {
        let i = self.index(node) as u32;
        match self.log.absolute(i) {
            Some(context) => context,
            None => self.tree.absolute_contexts.get(&i),
        }
    }

    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput, block_ctx: Option<&mut BlockContext<'_>>) -> LayoutOutput {
        crate::compute::compute_child_layout(self, node, inputs, block_ctx)
    }

    #[inline]
    fn take_observed(&mut self) -> u8 {
        core::mem::take(&mut self.observed)
    }

    #[inline]
    fn note_observed(&mut self, axes: u8) {
        self.observed |= axes;
    }

    #[inline]
    fn measure_ignores_available_height(&self) -> bool {
        self.tree.measure_ignores_available_height
    }

    #[inline]
    fn batches_help(&self) -> bool {
        true
    }

    fn compute_child_layouts(&mut self, requests: &[ChildRequest], out: &mut Vec<LayoutOutput>) {
        let groups = if self.speculation == 0 { super::plan(self.pool, self.config, self.tree, requests) } else { None };
        let logs = groups.map(|groups| super::pool::run_batch(self.pool, self.tree.clone(), self.measure.clone(), self.config, requests, groups));
        super::finish_batch(self, requests, logs, |worker, log| worker.log.absorb(log), out);
    }
}

impl<Ctx: Send + Sync, F> CacheAccess for Worker<'_, Ctx, F> {
    #[inline]
    fn cache(&self, node: NodeId) -> &NodeCache {
        self.tree.caches.get(self.index(node))
    }
    #[inline]
    fn cache_mut(&mut self, node: NodeId) -> &mut NodeCache {
        // SAFETY: `node` lies in this worker's subtree (see `column`).
        unsafe { self.tree.caches.slot(self.index(node)) }
    }
    #[inline]
    fn generation(&self) -> u32 {
        self.tree.generation
    }
    #[inline]
    fn speculation(&self) -> u32 {
        self.speculation
    }
    #[inline]
    fn set_speculation(&mut self, depth: u32) {
        self.speculation = depth;
    }
}
