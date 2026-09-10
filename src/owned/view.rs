//! The trait implementation the algorithms drive during one layout call.

use super::node::NONE;
use super::parallel::{Executor, TreeSlot};
use super::{MeasureFn, Tree};
use crate::compute::block::BlockContext;
use crate::style::{CalcId, Ident, Style};
use crate::tree::{
    CacheAccess, ChildRequest, Layout, LayoutInput, LayoutOutput, LayoutTree, MeasureInput, MeasureOutput, NodeCache, NodeId,
};

pub(super) struct View<'a, Ctx, F, E, A> {
    pub tree: A,
    pub measure: F,
    pub executor: &'a E,
    pub speculation: u32,
    pub ctx: core::marker::PhantomData<Ctx>,
}

impl<Ctx, F: MeasureFn<Ctx>, E: Executor<Ctx>, A: TreeSlot<Ctx>> LayoutTree for View<'_, Ctx, F, E, A> {
    type ChildIter<'b>
        = super::node::Children<'b>
    where
        Self: 'b;

    #[inline]
    fn children(&self, node: NodeId) -> Self::ChildIter<'_> {
        self.tree.get().children(node)
    }
    #[inline]
    fn child_count(&self, node: NodeId) -> usize {
        self.tree.get().child_count(node)
    }
    #[inline]
    fn child_at(&self, node: NodeId, index: usize) -> NodeId {
        self.tree.get().children(node).nth(index).expect("child index in range")
    }
    #[inline]
    fn style(&self, node: NodeId) -> &Style {
        self.tree.get().style(node)
    }
    #[inline]
    fn query_meta(&self, node: NodeId) -> crate::tree::QueryMeta {
        self.tree.get().query_meta(node)
    }
    #[inline]
    fn leaf_box(&self, node: NodeId) -> Option<crate::compute::leaf::LeafBox> {
        self.tree.get().leaf_box(node)
    }
    #[inline]
    fn resolve_calc(&self, id: CalcId, basis: f32) -> f32 {
        self.tree.get().calc.resolve(id, basis)
    }
    fn suffixed_ident(&self, base: Ident, suffix: crate::tree::IdentSuffix) -> Option<Ident> {
        let name = format!("{}{}", self.tree.get().ident_name(base), suffix.as_str());
        self.tree.get().idents.get(&name).copied()
    }

    #[inline]
    fn leaf_is_cheap(&self, node: NodeId) -> bool {
        self.tree.get().context_slot[self.tree.get().index(node)] == NONE
    }

    fn measure(&mut self, node: NodeId, input: MeasureInput) -> MeasureOutput {
        let tree = self.tree.get_mut();
        let i = tree.index(node);
        let slot = tree.context_slot[i];
        if slot == NONE {
            return MeasureOutput::from_size(input.known_dimensions.unwrap_or(crate::geometry::Size::ZERO));
        }
        let style = tree.styles.get(tree.style_ids[i]);
        let ctx = tree.contexts[slot as usize].as_mut().expect("context slot is live");
        (self.measure)(ctx, input, style)
    }

    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        if self.speculation > 0 {
            return;
        }
        let i = self.tree.get().index(node);
        self.tree.get_mut().write_unrounded(i, layout);
    }

    fn wants_detailed_grid_info(&self) -> bool {
        self.tree.get().detailed_grid_wanted
    }

    fn set_detailed_grid_info(&mut self, node: NodeId, info: crate::tree::DetailedGridInfo) {
        let i = self.tree.get().index(node) as u32;
        self.tree.get_mut().detailed_grid.insert(i, info);
    }

    fn unrounded_layout(&self, node: NodeId) -> Option<Layout> {
        Some(self.tree.get().unrounded[self.tree.get().index(node)])
    }

    fn detailed_grid_info(&self, node: NodeId) -> Option<crate::tree::DetailedGridInfo> {
        self.tree.get().detailed_grid.get(&(self.tree.get().index(node) as u32)).cloned()
    }

    fn set_absolute_context(&mut self, node: NodeId, context: crate::tree::AbsoluteContext) {
        if self.speculation > 0 {
            return;
        }
        let i = self.tree.get().index(node) as u32;
        self.tree.get_mut().absolute_contexts.insert(i, context);
    }

    fn absolute_context(&self, node: NodeId) -> Option<crate::tree::AbsoluteContext> {
        self.tree.get().absolute_contexts.get(&(self.tree.get().index(node) as u32)).cloned()
    }

    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput, block_ctx: Option<&mut BlockContext<'_>>) -> LayoutOutput {
        crate::compute::compute_child_layout(self, node, inputs, block_ctx)
    }

    #[inline]
    fn batches_help(&self) -> bool {
        self.executor.distributes()
    }

    fn compute_child_layouts(&mut self, requests: &[ChildRequest], out: &mut Vec<LayoutOutput>) {
        let logs = if self.speculation == 0 { self.executor.run(&self.tree, requests) } else { None };
        super::parallel::finish_batch(self, requests, logs, |view, log| view.tree.get_mut().commit(log), out);
    }
}

impl<Ctx, F, E, A: TreeSlot<Ctx>> CacheAccess for View<'_, Ctx, F, E, A> {
    #[inline]
    fn cache(&self, node: NodeId) -> &NodeCache {
        &self.tree.get().caches[self.tree.get().index(node)]
    }
    #[inline]
    fn cache_mut(&mut self, node: NodeId) -> &mut NodeCache {
        let tree = self.tree.get_mut();
        let i = tree.index(node);
        &mut tree.caches[i]
    }
    #[inline]
    fn generation(&self) -> u32 {
        self.tree.get().generation
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

impl<Ctx, F: MeasureFn<Ctx>, E: Executor<Ctx>, A: TreeSlot<Ctx>> crate::schedule::Incremental for View<'_, Ctx, F, E, A> {
    #[inline]
    fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.tree.get().parent(node)
    }
    #[inline]
    fn depth(&self, node: NodeId) -> u32 {
        self.tree.get().depth(node)
    }
    #[inline]
    fn flags(&self, node: NodeId) -> u8 {
        self.tree.get().flags[self.tree.get().index(node)]
    }
    #[inline]
    fn set_flags(&mut self, node: NodeId, flags: u8) {
        let tree = self.tree.get_mut();
        let i = tree.index(node);
        tree.flags[i] = flags;
    }
}

/// Marks nodes between layout calls; never lays anything out.
pub(super) struct Marker<'a, Ctx> {
    pub tree: &'a mut Tree<Ctx>,
}

impl<Ctx> LayoutTree for Marker<'_, Ctx> {
    type ChildIter<'b>
        = super::node::Children<'b>
    where
        Self: 'b;
    fn children(&self, node: NodeId) -> Self::ChildIter<'_> {
        self.tree.children(node)
    }
    fn child_count(&self, node: NodeId) -> usize {
        self.tree.child_count(node)
    }
    fn child_at(&self, node: NodeId, index: usize) -> NodeId {
        self.tree.children(node).nth(index).expect("child index in range")
    }
    fn style(&self, node: NodeId) -> &Style {
        self.tree.style(node)
    }
    fn set_unrounded_layout(&mut self, _: NodeId, _: &Layout) {
        unreachable!("marker never lays out")
    }
    fn compute_child_layout(&mut self, _: NodeId, _: LayoutInput, _: Option<&mut BlockContext<'_>>) -> LayoutOutput {
        unreachable!("marker never lays out")
    }
}

impl<Ctx> CacheAccess for Marker<'_, Ctx> {
    fn cache(&self, node: NodeId) -> &NodeCache {
        &self.tree.caches[self.tree.index(node)]
    }
    fn cache_mut(&mut self, node: NodeId) -> &mut NodeCache {
        let i = self.tree.index(node);
        &mut self.tree.caches[i]
    }
    fn generation(&self) -> u32 {
        self.tree.generation
    }
    fn speculation(&self) -> u32 {
        0
    }
    fn set_speculation(&mut self, _: u32) {}
}

impl<Ctx> crate::schedule::Incremental for Marker<'_, Ctx> {
    fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.tree.parent(node)
    }
    fn depth(&self, node: NodeId) -> u32 {
        self.tree.depth(node)
    }
    fn flags(&self, node: NodeId) -> u8 {
        self.tree.flags[self.tree.index(node)]
    }
    fn set_flags(&mut self, node: NodeId, flags: u8) {
        let i = self.tree.index(node);
        self.tree.flags[i] = flags;
    }
}

impl<Ctx> crate::round::RoundTree for Tree<Ctx> {
    fn child_ids(&self, node: NodeId, out: &mut Vec<NodeId>) {
        out.extend(self.children(node));
    }
    #[inline]
    fn unrounded(&self, node: NodeId) -> Layout {
        self.unrounded[self.index(node)]
    }
    #[inline]
    fn set_rounded(&mut self, node: NodeId, layout: Layout) {
        let i = self.index(node);
        self.rounded[i] = layout;
    }
}
