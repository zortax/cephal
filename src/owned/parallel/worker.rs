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
}

impl<'a, Ctx: Send + Sync, F> Worker<'a, Ctx, F> {
    pub fn new(tree: &'a Arc<Tree<Ctx>>, measure: &'a Arc<F>, config: Config, pool: &'a Pool, nodes: usize) -> Self {
        let mut log = Log::default();
        log.touched.reserve(nodes);
        Self { tree, measure, config, pool, log, speculation: 0 }
    }

    pub fn into_log(self) -> Log {
        self.log
    }

    /// Records the output of the batch request `r`.
    pub fn answered(&mut self, r: usize, output: LayoutOutput) {
        self.log.answers.push((r, output));
    }

    #[inline]
    fn index(&self, node: NodeId) -> usize {
        self.tree.index(node)
    }
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
    fn leaf_box(&self, node: NodeId) -> Option<crate::compute::leaf::LeafBox> {
        self.tree.leaf_box(node)
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
        if *slot != *layout {
            if *touched_gen != self.tree.generation {
                *touched_gen = self.tree.generation;
                self.log.touched.push((node, *slot));
            }
            *slot = *layout;
        }
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
            self.log.absolute.push((self.index(node) as u32, context));
        }
    }
    fn absolute_context(&self, node: NodeId) -> Option<AbsoluteContext> {
        let i = self.index(node) as u32;
        self.log.absolute(i).or_else(|| self.tree.absolute_contexts.get(&i).cloned())
    }

    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput, block_ctx: Option<&mut BlockContext<'_>>) -> LayoutOutput {
        crate::compute::compute_child_layout(self, node, inputs, block_ctx)
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
