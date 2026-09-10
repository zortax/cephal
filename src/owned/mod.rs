//! A ready-made tree: node columns, interned styles, per-node caches and contexts.

mod column;
mod id;
mod node;
pub mod parallel;
mod styles;
mod view;

pub use id::NodeHandle;
pub use parallel::{Executor, Inline, Parallel, SyncMeasureFn};
pub use styles::StyleTable;

use crate::geometry::{AvailableSpace, Size};
use crate::style::{CalcId, CalcOp, CalcTable, Ident, Position, Style};
use crate::schedule::{META_MASK, ROUND_PENDING, Scheduler};
use crate::tree::{Layout, MeasureInput, MeasureOutput, NodeCache, NodeId};
use std::sync::Arc;
use crate::hash::FxHashMap;
use node::{Links, NONE};

/// Measures leaf content: `(context, input, style)`.
pub trait MeasureFn<Ctx>: FnMut(&mut Ctx, MeasureInput, &Style) -> MeasureOutput {}
impl<Ctx, F: FnMut(&mut Ctx, MeasureInput, &Style) -> MeasureOutput> MeasureFn<Ctx> for F {}

/// Owns nodes as parallel columns indexed by node index.
pub struct Tree<Ctx = ()> {
    links: Vec<Links>,
    style_ids: Vec<u32>,
    flags: Vec<u8>,
    unrounded: column::Column<Layout>,
    rounded: Vec<Layout>,
    /// Absolute unrounded origin used by the last rounding pass.
    abs_origin: Vec<crate::geometry::Point<f32>>,
    caches: column::Column<NodeCache>,
    context_slot: Vec<u32>,
    generations: Vec<u16>,
    free: Vec<u32>,
    contexts: Vec<Option<Ctx>>,
    free_contexts: Vec<u32>,
    styles: StyleTable,
    calc: CalcTable,
    idents: FxHashMap<String, Ident>,
    ident_names: Vec<String>,
    generation: u32,
    use_rounding: bool,
    detailed_grid_wanted: bool,
    /// Nodes written during the current layout call, with their layout before it.
    touched: Vec<(NodeId, Layout)>,
    /// Generation in which each node was last written.
    touched_gen: column::Column<u32>,
    /// Nodes whose unrounded layout changed in the last layout call.
    changed: Vec<NodeId>,
    detailed_grid: FxHashMap<u32, crate::tree::DetailedGridInfo>,
    absolute_contexts: FxHashMap<u32, crate::tree::AbsoluteContext>,
    scheduler: Scheduler,
    /// The last root and available space laid out.
    last_root: Option<(NodeId, Size<AvailableSpace>)>,
}

impl<Ctx> Default for Tree<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Ctx> Tree<Ctx> {
    pub fn new() -> Self {
        Self::with_capacity(16)
    }

    pub fn with_capacity(n: usize) -> Self {
        Self {
            links: Vec::with_capacity(n),
            style_ids: Vec::with_capacity(n),
            flags: Vec::with_capacity(n),
            unrounded: column::Column::with_capacity(n),
            rounded: Vec::new(),
            abs_origin: Vec::new(),
            caches: column::Column::with_capacity(n),
            context_slot: Vec::with_capacity(n),
            generations: Vec::with_capacity(n),
            free: Vec::new(),
            contexts: Vec::new(),
            free_contexts: Vec::new(),
            styles: StyleTable::new(),
            calc: CalcTable::new(),
            idents: FxHashMap::default(),
            ident_names: Vec::new(),
            generation: 1,
            use_rounding: true,
            detailed_grid_wanted: false,
            touched: Vec::new(),
            touched_gen: column::Column::with_capacity(n),
            changed: Vec::new(),
            detailed_grid: FxHashMap::default(),
            absolute_contexts: FxHashMap::default(),
            scheduler: Scheduler::new(),
            last_root: None,
        }
    }

    pub fn enable_rounding(&mut self) {
        self.use_rounding = true;
    }
    pub fn disable_rounding(&mut self) {
        self.use_rounding = false;
    }

    /// Records resolved tracks for every grid container, readable via `detailed_grid_info`.
    /// Takes effect on the next layout of each grid.
    pub fn enable_detailed_grid_info(&mut self) {
        self.detailed_grid_wanted = true;
    }

    /// Interns a grid line or area name.
    pub fn intern(&mut self, name: &str) -> Ident {
        if let Some(&id) = self.idents.get(name) {
            return id;
        }
        let id = Ident(self.ident_names.len() as u32);
        self.ident_names.push(name.to_owned());
        self.idents.insert(name.to_owned(), id);
        id
    }
    pub fn ident_name(&self, id: Ident) -> &str {
        &self.ident_names[id.0 as usize]
    }

    /// Registers a `calc()` program for use in styles.
    pub fn register_calc(&mut self, program: &[CalcOp]) -> CalcId {
        self.calc.register(program)
    }

    pub fn node_count(&self) -> usize {
        self.links.len() - self.free.len()
    }

    // --- construction -------------------------------------------------------------------------

    pub fn new_leaf(&mut self, style: Style) -> NodeId {
        self.alloc(style, None)
    }

    pub fn new_leaf_with_context(&mut self, style: Style, context: Ctx) -> NodeId {
        self.alloc(style, Some(context))
    }

    pub fn new_with_children(&mut self, style: Style, children: &[NodeId]) -> NodeId {
        let node = self.alloc(style, None);
        for &child in children {
            self.append_child(node, child);
        }
        node
    }

    /// Per-node facts kept in the upper flag bits: hidden, width/height parent dependency, has context.
    fn meta_bits(&self, style_id: u32, context_slot: u32) -> u8 {
        let style = self.styles.get(style_id);
        let dep = self.styles.parent_size_dependency(style_id);
        (style.generates_no_box() as u8) << 4 | (dep.width as u8) << 5 | (dep.height as u8) << 6 | ((context_slot != NONE) as u8) << 7
    }

    #[inline]
    fn refresh_meta(&mut self, i: usize) {
        let bits = self.meta_bits(self.style_ids[i], self.context_slot[i]);
        self.flags[i] = (self.flags[i] & !META_MASK) | bits;
    }

    fn alloc(&mut self, style: Style, context: Option<Ctx>) -> NodeId {
        let style_id = self.styles.intern(style);
        let context_slot = match context {
            Some(ctx) => self.alloc_context(ctx),
            None => NONE,
        };
        let meta = self.meta_bits(style_id, context_slot);
        let index = match self.free.pop() {
            Some(i) => {
                let i = i as usize;
                self.links[i] = Links::detached();
                self.style_ids[i] = style_id;
                self.flags[i] = meta;
                self.touched_gen[i] = 0;
                self.unrounded[i] = Layout::ZERO;
                if !self.rounded.is_empty() {
                    self.rounded[i] = Layout::ZERO;
                    self.abs_origin[i] = crate::geometry::Point { x: f32::NAN, y: f32::NAN };
                }
                self.caches[i] = NodeCache::new();
                self.context_slot[i] = context_slot;
                i
            }
            None => {
                self.links.push(Links::detached());
                self.style_ids.push(style_id);
                self.flags.push(meta);
                self.touched_gen.push(0);
                self.unrounded.push(Layout::ZERO);
                if !self.rounded.is_empty() {
                    self.rounded.push(Layout::ZERO);
                    self.abs_origin.push(crate::geometry::Point { x: f32::NAN, y: f32::NAN });
                }
                self.caches.push(NodeCache::new());
                self.context_slot.push(context_slot);
                self.generations.push(1);
                self.links.len() - 1
            }
        };
        id::make(index as u32, self.generations[index])
    }

    fn alloc_context(&mut self, ctx: Ctx) -> u32 {
        match self.free_contexts.pop() {
            Some(i) => {
                self.contexts[i as usize] = Some(ctx);
                i
            }
            None => {
                self.contexts.push(Some(ctx));
                self.contexts.len() as u32 - 1
            }
        }
    }

    /// Appends `child` as the last child of `parent`.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.insert_child_before(parent, child, None);
    }

    /// Inserts `child` before `before` (or last when `None`); `child` must be detached.
    pub fn insert_child_before(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        let p = self.index(parent);
        let c = self.index(child);
        debug_assert_eq!(self.links[c].parent, NONE, "child must be detached");
        let next = before.map_or(NONE, |b| self.index(b) as u32);
        let prev = if next == NONE { self.links[p].last_child } else { self.links[next as usize].prev };
        self.links[c].parent = p as u32;
        self.links[c].prev = prev;
        self.links[c].next = next;
        if prev == NONE {
            self.links[p].first_child = c as u32;
        } else {
            self.links[prev as usize].next = c as u32;
        }
        if next == NONE {
            self.links[p].last_child = c as u32;
        } else {
            self.links[next as usize].prev = c as u32;
        }
        self.links[p].child_count += 1;
        let depth = self.links[p].depth + 1;
        self.set_depth(c, depth);
        self.mark_dirty_index(p);
    }

    /// Inserts `child` at `index` among `parent`'s children.
    pub fn insert_child_at(&mut self, parent: NodeId, index: usize, child: NodeId) {
        let before = self.children(parent).nth(index);
        self.insert_child_before(parent, child, before);
    }

    /// Replaces the child list of `parent`.
    pub fn set_children(&mut self, parent: NodeId, children: &[NodeId]) {
        let old: Vec<NodeId> = self.children(parent).collect();
        for c in old {
            self.detach(c);
        }
        for &c in children {
            if self.parent(c).is_some() {
                self.detach(c);
            }
            self.append_child(parent, c);
        }
    }

    /// Unlinks `child` from its parent, keeping the subtree alive.
    pub fn detach(&mut self, child: NodeId) {
        let c = self.index(child);
        let p = self.links[c].parent;
        if p == NONE {
            return;
        }
        let (prev, next) = (self.links[c].prev, self.links[c].next);
        if prev == NONE {
            self.links[p as usize].first_child = next;
        } else {
            self.links[prev as usize].next = next;
        }
        if next == NONE {
            self.links[p as usize].last_child = prev;
        } else {
            self.links[next as usize].prev = prev;
        }
        self.links[p as usize].child_count -= 1;
        self.links[c].parent = NONE;
        self.links[c].prev = NONE;
        self.links[c].next = NONE;
        self.set_depth(c, 0);
        self.mark_dirty_index(p as usize);
    }

    /// Removes `child` from `parent`.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        debug_assert_eq!(self.parent(child), Some(parent));
        self.detach(child);
    }

    /// Frees `node` and its subtree.
    pub fn remove(&mut self, node: NodeId) {
        self.detach(node);
        let mut stack = vec![self.index(node)];
        while let Some(i) = stack.pop() {
            let mut c = self.links[i].first_child;
            while c != NONE {
                stack.push(c as usize);
                c = self.links[c as usize].next;
            }
            let slot = self.context_slot[i];
            if slot != NONE {
                self.contexts[slot as usize] = None;
                self.free_contexts.push(slot);
            }
            self.styles.release(self.style_ids[i]);
            self.caches[i].clear();
            self.links[i] = Links::detached();
            self.generations[i] = self.generations[i].wrapping_add(1).max(1);
            self.free.push(i as u32);
        }
    }

    /// Drops every node.
    pub fn clear(&mut self) {
        self.links.clear();
        self.style_ids.clear();
        self.flags.clear();
        self.touched_gen.clear();
        self.touched.clear();
        self.unrounded.clear();
        self.rounded.clear();
        self.abs_origin.clear();
        self.caches.clear();
        self.context_slot.clear();
        self.generations.clear();
        self.free.clear();
        self.contexts.clear();
        self.free_contexts.clear();
        self.styles = StyleTable::new();
        self.changed.clear();
        self.detailed_grid.clear();
        self.absolute_contexts.clear();
        self.scheduler = Scheduler::new();
        self.last_root = None;
    }

    /// Forgets every cached answer and layout so the next call lays out from scratch.
    pub fn clear_layout(&mut self) {
        for c in self.caches.iter_mut() {
            c.clear();
        }
        for f in &mut self.flags {
            *f &= META_MASK;
        }
        for l in self.unrounded.iter_mut() {
            *l = Layout::ZERO;
        }
        self.rounded.clear();
        self.abs_origin.clear();
        self.touched.clear();
        self.changed.clear();
        self.detailed_grid.clear();
        self.absolute_contexts.clear();
        self.scheduler = Scheduler::new();
        self.last_root = None;
    }

    /// A copy with the same structure, styles and contexts but no layout state, for checking
    /// that incremental results match a cold computation.
    pub fn clone_cold(&self) -> Self
    where
        Ctx: Clone,
    {
        Self {
            links: self.links.clone(),
            style_ids: self.style_ids.clone(),
            flags: self.flags.iter().map(|f| f & META_MASK).collect(),
            unrounded: vec![Layout::ZERO; self.unrounded.len()].into(),
            rounded: Vec::new(),
            abs_origin: Vec::new(),
            caches: (0..self.caches.len()).map(|_| NodeCache::new()).collect::<Vec<_>>().into(),
            context_slot: self.context_slot.clone(),
            generations: self.generations.clone(),
            free: self.free.clone(),
            contexts: self.contexts.clone(),
            free_contexts: self.free_contexts.clone(),
            styles: self.styles.clone(),
            calc: self.calc.clone(),
            idents: self.idents.clone(),
            ident_names: self.ident_names.clone(),
            generation: 1,
            use_rounding: self.use_rounding,
            detailed_grid_wanted: self.detailed_grid_wanted,
            touched: Vec::new(),
            touched_gen: vec![0; self.touched_gen.len()].into(),
            changed: Vec::new(),
            detailed_grid: FxHashMap::default(),
            absolute_contexts: FxHashMap::default(),
            scheduler: Scheduler::new(),
            last_root: None,
        }
    }

    fn set_depth(&mut self, root: usize, depth: u32) {
        let mut stack = vec![(root, depth)];
        while let Some((i, d)) = stack.pop() {
            self.links[i].depth = d;
            let mut c = self.links[i].first_child;
            while c != NONE {
                stack.push((c as usize, d + 1));
                c = self.links[c as usize].next;
            }
        }
    }

    // --- styles and contexts ------------------------------------------------------------------

    pub fn style(&self, node: NodeId) -> &Style {
        self.styles.get(self.style_ids[self.index(node)])
    }

    #[inline]
    fn leaf_box(&self, node: NodeId) -> Option<crate::compute::leaf::LeafBox> {
        self.styles.leaf_box(self.style_ids[self.index(node)])
    }

    /// See [`crate::tree::LayoutTree::query_meta`]; two loads per query.
    #[inline]
    fn query_meta(&self, node: NodeId) -> crate::tree::QueryMeta {
        let i = self.index(node);
        let f = self.flags[i];
        crate::tree::QueryMeta {
            hidden: f & 0x10 != 0,
            cheap_leaf: f & 0x80 == 0 && self.links[i].child_count == 0,
            dependency: Size { width: f & 0x20 != 0, height: f & 0x40 != 0 },
        }
    }

    /// Replaces the style; an equal style is a no-op.
    pub fn set_style(&mut self, node: NodeId, style: Style) {
        let i = self.index(node);
        let old = self.style_ids[i];
        let old_style = self.styles.get(old);
        if old_style == &style {
            return;
        }
        // An absolutely positioned box that stays absolute cannot move its siblings.
        let absolute_only = old_style.position == Position::Absolute
            && style.position == Position::Absolute
            && !old_style.generates_no_box()
            && !style.generates_no_box()
            && self.links[i].parent != NONE;
        let new = self.styles.intern(style);
        self.styles.release(old);
        self.style_ids[i] = new;
        self.refresh_meta(i);
        if absolute_only {
            let id = self.id_of(i);
            let mut scheduler = core::mem::take(&mut self.scheduler);
            scheduler.mark_absolute(&mut view::Marker { tree: self }, id);
            self.scheduler = scheduler;
        } else {
            self.mark_dirty_with_parent(i);
        }
    }

    /// Edits the style in place; dirtiness is derived from the resulting diff.
    pub fn patch_style(&mut self, node: NodeId, f: impl FnOnce(&mut Style)) {
        let mut style = self.style(node).clone();
        f(&mut style);
        self.set_style(node, style);
    }

    pub fn context(&self, node: NodeId) -> Option<&Ctx> {
        let slot = self.context_slot[self.index(node)];
        if slot == NONE { None } else { self.contexts[slot as usize].as_ref() }
    }

    pub fn context_mut(&mut self, node: NodeId) -> Option<&mut Ctx> {
        let slot = self.context_slot[self.index(node)];
        if slot == NONE { None } else { self.contexts[slot as usize].as_mut() }
    }

    pub fn set_context(&mut self, node: NodeId, context: Option<Ctx>) {
        let i = self.index(node);
        let slot = self.context_slot[i];
        match (slot != NONE, context) {
            (true, Some(ctx)) => self.contexts[slot as usize] = Some(ctx),
            (true, None) => {
                self.contexts[slot as usize] = None;
                self.free_contexts.push(slot);
                self.context_slot[i] = NONE;
            }
            (false, Some(ctx)) => self.context_slot[i] = self.alloc_context(ctx),
            (false, None) => {}
        }
        self.refresh_meta(i);
        self.mark_dirty_index(i);
    }

    /// Signals that the content behind `node`'s context changed.
    pub fn invalidate(&mut self, node: NodeId) {
        let i = self.index(node);
        self.mark_dirty_index(i);
    }

    // --- structure ----------------------------------------------------------------------------

    pub fn parent(&self, node: NodeId) -> Option<NodeId> {
        let p = self.links[self.index(node)].parent;
        (p != NONE).then(|| self.id_of(p as usize))
    }

    pub fn children(&self, node: NodeId) -> node::Children<'_> {
        node::Children { links: &self.links, generations: &self.generations, next: self.links[self.index(node)].first_child }
    }

    pub fn child_count(&self, node: NodeId) -> usize {
        self.links[self.index(node)].child_count as usize
    }

    pub fn depth(&self, node: NodeId) -> u32 {
        self.links[self.index(node)].depth
    }

    // --- results ------------------------------------------------------------------------------

    /// Rounded layout when rounding is enabled, otherwise the unrounded one.
    pub fn layout(&self, node: NodeId) -> &Layout {
        let i = self.index(node);
        if self.use_rounding && !self.rounded.is_empty() { &self.rounded[i] } else { &self.unrounded[i] }
    }

    pub fn unrounded_layout(&self, node: NodeId) -> &Layout {
        &self.unrounded[self.index(node)]
    }

    /// Nodes whose unrounded layout changed during the last layout call.
    pub fn changed(&self) -> &[NodeId] {
        &self.changed
    }

    /// Resolved tracks of a grid container from the last layout.
    pub fn detailed_grid_info(&self, node: NodeId) -> Option<&crate::tree::DetailedGridInfo> {
        self.detailed_grid.get(&(self.index(node) as u32))
    }

    /// Whether `node`'s cache is empty.
    pub fn dirty(&self, node: NodeId) -> bool {
        self.caches[self.index(node)].is_empty()
    }

    // --- layout -------------------------------------------------------------------------------

    /// Lays out the tree under `root` without measuring leaves.
    pub fn compute_layout(&mut self, root: NodeId, available_space: Size<AvailableSpace>) {
        self.compute_layout_with_measure(root, available_space, |_: &mut Ctx, input: MeasureInput, _: &Style| {
            MeasureOutput::from_size(input.known_dimensions.unwrap_or(Size::ZERO))
        });
    }

    /// Lays out the tree under `root`, measuring leaves with `measure`.
    ///
    /// Only nodes marked since the previous call are recomputed; the result equals a cold layout.
    pub fn compute_layout_with_measure(
        &mut self,
        root: NodeId,
        available_space: Size<AvailableSpace>,
        measure: impl MeasureFn<Ctx>,
    ) {
        Self::layout_with(self, root, available_space, measure, &Inline);
    }

    /// Lays out the tree under `root`, spreading cold subtrees over `executor`'s threads.
    ///
    /// Results are identical to [`Self::compute_layout_with_measure`] with the same measure function.
    pub fn compute_layout_parallel<F: SyncMeasureFn<Ctx>>(
        &mut self,
        root: NodeId,
        available_space: Size<AvailableSpace>,
        executor: &Parallel<F>,
    ) where
        Ctx: Send + Sync + 'static,
    {
        let measure = executor.measure_fn();
        let measure = |ctx: &mut Ctx, input: MeasureInput, style: &Style| measure(&*ctx, input, style);
        // Workers hold the tree through this handle while a batch runs.
        let mut shared = Arc::new(core::mem::take(self));
        Self::layout_with(&mut shared, root, available_space, measure, executor);
        *self = Arc::try_unwrap(shared).ok().expect("no worker holds the tree after layout");
    }

    fn layout_with<A: parallel::TreeSlot<Ctx>, E: Executor<Ctx>>(
        mut slot: A,
        root: NodeId,
        available_space: Size<AvailableSpace>,
        measure: impl MeasureFn<Ctx>,
        executor: &E,
    ) {
        let this = slot.get_mut();
        this.generation = this.generation.wrapping_add(1).max(1);
        this.changed.clear();
        let root_query_changed = this.last_root != Some((root, available_space));
        let mut scheduler = core::mem::take(&mut this.scheduler);
        {
            let mut view = view::View { tree: &mut slot, measure, executor, speculation: 0, ctx: core::marker::PhantomData };
            scheduler.run(&mut view, root, root_query_changed);
            if root_query_changed {
                let tree = view.tree.get_mut();
                let i = tree.index(root);
                tree.caches[i].clear();
            }
            crate::compute::compute_root_layout(&mut view, root, available_space);
        }
        let this = slot.get_mut();
        this.scheduler = scheduler;
        this.finish_frame(root, available_space, root_query_changed);
    }

    /// Turns the frame's touched list into `changed()` and rounds what moved.
    fn finish_frame(&mut self, root: NodeId, available_space: Size<AvailableSpace>, root_query_changed: bool) {
        self.last_root = Some((root, available_space));
        for (node, before) in self.touched.drain(..) {
            let (index, generation) = id::split(node);
            if self.generations[index as usize] == generation && self.unrounded[index as usize] != before {
                self.changed.push(node);
            }
        }
        if self.use_rounding {
            if self.rounded.len() != self.unrounded.len() {
                // First rounding pass, or nodes were added: round everything.
                self.rounded = self.unrounded.to_vec();
                self.abs_origin = vec![crate::geometry::Point { x: f32::NAN, y: f32::NAN }; self.unrounded.len()];
                self.round_subtree(root, 0.0, 0.0, true);
            } else if !self.changed.is_empty() {
                self.round_changed();
            }
        }
        let _ = root_query_changed;
    }

    /// Re-rounds the rewritten nodes and their ancestors, and every subtree whose origin moved;
    /// nothing else is visited, so a contained edit costs its own ancestor chain only.
    fn round_changed(&mut self) {
        let mut pending: Vec<u32> = Vec::with_capacity(self.changed.len() * 2);
        for &node in &self.changed {
            let (index, _) = id::split(node);
            let mut i = index;
            loop {
                if self.flags[i as usize] & ROUND_PENDING != 0 {
                    break;
                }
                self.flags[i as usize] |= ROUND_PENDING;
                pending.push(i);
                let p = self.links[i as usize].parent;
                if p == NONE {
                    break;
                }
                i = p;
            }
        }
        // Parents first, so a node reads its parent's already updated origin.
        pending.sort_unstable_by_key(|&i| self.links[i as usize].depth);
        for i in pending {
            let i = i as usize;
            // Already re-rounded by a moved ancestor's subtree walk.
            if self.flags[i] & ROUND_PENDING == 0 {
                continue;
            }
            self.flags[i] &= !ROUND_PENDING;
            let p = self.links[i].parent;
            let origin = if p == NONE { crate::geometry::Point { x: 0.0, y: 0.0 } } else { self.abs_origin[p as usize] };
            let u = self.unrounded[i];
            let x = origin.x + u.location.x;
            let y = origin.y + u.location.y;
            let moved = self.abs_origin[i].x != x || self.abs_origin[i].y != y;
            self.abs_origin[i] = crate::geometry::Point { x, y };
            self.rounded[i] = crate::round::round_one(&u, x, y);
            if moved {
                let mut c = self.links[i].first_child;
                while c != NONE {
                    self.round_subtree(self.id_of(c as usize), x, y, true);
                    c = self.links[c as usize].next;
                }
            }
        }
    }

    /// Rounds `node`; descends where a descendant was rewritten or the origin moved.
    fn round_subtree(&mut self, root: NodeId, parent_x: f32, parent_y: f32, force: bool) {
        let mut stack: Vec<(usize, f32, f32, bool)> = vec![(self.index(root), parent_x, parent_y, force)];
        while let Some((i, px, py, force)) = stack.pop() {
            let u = self.unrounded[i];
            let x = px + u.location.x;
            let y = py + u.location.y;
            let pending = self.flags[i] & ROUND_PENDING != 0;
            self.flags[i] &= !ROUND_PENDING;
            let moved = self.abs_origin[i].x != x || self.abs_origin[i].y != y;
            if !(force || pending || moved) {
                continue;
            }
            self.abs_origin[i] = crate::geometry::Point { x, y };
            self.rounded[i] = crate::round::round_one(&u, x, y);
            let mut c = self.links[i].first_child;
            while c != NONE {
                stack.push((c as usize, x, y, force || moved));
                c = self.links[c as usize].next;
            }
        }
    }

    // --- internals ----------------------------------------------------------------------------

    #[inline]
    /// Stores a layout, remembering the previous value for `changed()`.
    fn write_unrounded(&mut self, i: usize, layout: &Layout) {
        if self.unrounded[i] != *layout {
            if self.touched_gen[i] != self.generation {
                self.touched_gen[i] = self.generation;
                self.touched.push((id::make(i as u32, self.generations[i]), self.unrounded[i]));
            }
            self.unrounded[i] = *layout;
        }
    }

    fn index(&self, node: NodeId) -> usize {
        let (index, generation) = id::split(node);
        debug_assert_eq!(self.generations[index as usize], generation, "stale node id {node:?}");
        index as usize
    }

    #[inline]
    fn id_of(&self, index: usize) -> NodeId {
        id::make(index as u32, self.generations[index])
    }

    /// Queues `node` for recomputation.
    fn mark_dirty_index(&mut self, i: usize) {
        let node = self.id_of(i);
        let mut scheduler = core::mem::take(&mut self.scheduler);
        scheduler.mark(&mut view::Marker { tree: self }, node);
        self.scheduler = scheduler;
    }

    /// Queues `node` and its parent for recomputation.
    fn mark_dirty_with_parent(&mut self, i: usize) {
        self.mark_dirty_index(i);
        let p = self.links[i].parent;
        if p != NONE {
            self.mark_dirty_index(p as usize);
        }
    }

    /// Queues `node` for recomputation, as after changing what its measure function returns.
    pub fn mark_dirty(&mut self, node: NodeId) {
        let i = self.index(node);
        self.mark_dirty_index(i);
    }

    /// Cached answers of `node`, for inspection.
    pub fn cache_entries(&self, node: NodeId) -> Vec<(crate::tree::LayoutInput, crate::tree::LayoutOutput)> {
        self.caches[self.index(node)].entries()
    }

    /// Whether `node` is queued for recomputation.
    pub fn is_dirty(&self, node: NodeId) -> bool {
        self.flags[self.index(node)] & crate::schedule::DIRTY != 0
    }
}
