//! Algorithm dispatch, the root entry point and hidden layout.

pub mod absolute;
pub mod block;
pub mod common;
pub mod reposition;
#[cfg(feature = "flex")]
pub mod flex;
#[cfg(feature = "grid")]
pub mod grid;
pub mod leaf;
pub mod plan;
pub mod scratch;

use crate::geometry::{AvailableSpace, Line, Point, Size};
use crate::style::Display;
use crate::tree::{AnswerSource, CacheAccess, ChildRequest, Layout, LayoutInput, LayoutOutput, LayoutTree, NodeId, RequestedAxis, RunMode, SizingMode};
use block::BlockContext;

/// Query counters, enabled by the `stats` feature.
#[cfg(feature = "stats")]
pub mod stats {
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    pub static HIDDEN: AtomicUsize = AtomicUsize::new(0);
    pub static CHEAP_LEAF: AtomicUsize = AtomicUsize::new(0);
    pub static HIT_SIZE: AtomicUsize = AtomicUsize::new(0);
    pub static HIT_FINAL: AtomicUsize = AtomicUsize::new(0);
    pub static MISS_SIZE: AtomicUsize = AtomicUsize::new(0);
    pub static MISS_FINAL: AtomicUsize = AtomicUsize::new(0);
    pub static MEASURE: AtomicUsize = AtomicUsize::new(0);
    pub static SPECULATIVE: AtomicUsize = AtomicUsize::new(0);
    pub static RELAXED_HIT: AtomicUsize = AtomicUsize::new(0);
    pub static BATCHES: AtomicUsize = AtomicUsize::new(0);
    pub static PLAN_FEW_REQUESTS: AtomicUsize = AtomicUsize::new(0);
    pub static PLAN_FEW_GROUPS: AtomicUsize = AtomicUsize::new(0);
    pub static PLAN_LITTLE_WORK: AtomicUsize = AtomicUsize::new(0);
    pub static BATCH_NANOS: AtomicUsize = AtomicUsize::new(0);
    pub static BATCH_REQUESTS: AtomicUsize = AtomicUsize::new(0);
    /// A size miss whose answer equals one already cached for the node under another key.
    pub static COLLISION: AtomicUsize = AtomicUsize::new(0);
    /// Container runs answering a single-axis size question.
    pub static RUN_SIZE_AXIS: AtomicUsize = AtomicUsize::new(0);
    /// Container runs answering a two-axis size question.
    pub static RUN_SIZE_BOTH: AtomicUsize = AtomicUsize::new(0);
    /// Single-line flex container runs that took the fused path, and that could not.
    pub static FLEX_FUSED: AtomicUsize = AtomicUsize::new(0);
    pub static FLEX_NOT_FUSED: AtomicUsize = AtomicUsize::new(0);
    pub fn bump(c: &AtomicUsize) {
        c.fetch_add(1, Relaxed);
    }
    pub fn report() -> String {
        let all = [("hidden", &HIDDEN), ("cheap_leaf", &CHEAP_LEAF), ("hit_size", &HIT_SIZE), ("hit_final", &HIT_FINAL), ("miss_size", &MISS_SIZE), ("miss_final", &MISS_FINAL), ("measure", &MEASURE), ("speculative", &SPECULATIVE), ("relaxed_hit", &RELAXED_HIT), ("batches", &BATCHES), ("plan_few_requests", &PLAN_FEW_REQUESTS), ("plan_few_groups", &PLAN_FEW_GROUPS), ("plan_little_work", &PLAN_LITTLE_WORK), ("batch_us", &BATCH_NANOS), ("batch_requests", &BATCH_REQUESTS), ("collision", &COLLISION), ("run_size_axis", &RUN_SIZE_AXIS), ("run_size_both", &RUN_SIZE_BOTH), ("flex_fused", &FLEX_FUSED), ("flex_not_fused", &FLEX_NOT_FUSED)];
        let s = all.iter().map(|(n, c)| format!("{n}={}", c.swap(0, Relaxed))).collect::<Vec<_>>().join(" ");
        s
    }
}

/// Width bit of [`LayoutTree::take_observed`]: the answer told min-content from max-content
/// available width.
pub const OBSERVED_WIDTH: u8 = 0b01;
/// Height bit of [`LayoutTree::take_observed`].
pub const OBSERVED_HEIGHT: u8 = 0b10;
/// Both axes; what trees that do not track observation report.
pub const OBSERVED_ALL: u8 = OBSERVED_WIDTH | OBSERVED_HEIGHT;

/// The observation bit of `axis`.
#[inline]
pub fn observed_bit(axis: crate::geometry::AbsoluteAxis) -> u8 {
    match axis {
        crate::geometry::AbsoluteAxis::Horizontal => OBSERVED_WIDTH,
        crate::geometry::AbsoluteAxis::Vertical => OBSERVED_HEIGHT,
    }
}

/// Lays out `node` for `inputs`, serving from the cache when the same question was answered before.
pub fn compute_child_layout<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    mut inputs: LayoutInput,
    block_ctx: Option<&mut BlockContext<'_>>,
) -> LayoutOutput {
    let meta = tree.query_meta(node);
    // Hidden results are never cached, so a hidden child's `order` is always rewritten.
    if inputs.run_mode == RunMode::PerformHiddenLayout || meta.hidden {
        #[cfg(feature = "stats")]
        stats::bump(&stats::HIDDEN);
        return compute_hidden_layout(tree, node);
    }
    // A leaf with nothing to measure costs less to recompute than to look up.
    if meta.cheap_leaf {
        return compute_cheap_leaf(tree, node, inputs, block_ctx);
    }
    #[cfg(feature = "verify-cache")]
    let raw_inputs = inputs;
    normalize(&mut inputs, meta);
    let generation = tree.generation();
    let key = crate::tree::cache::CacheKey::from_input(&inputs);
    if let Some((hit, observed)) = tree.cache(node).lookup(&key, inputs.run_mode, generation) {
        #[cfg(feature = "stats")]
        stats::bump(if inputs.run_mode == RunMode::PerformLayout { &stats::HIT_FINAL } else { &stats::HIT_SIZE });
        #[cfg(feature = "verify-cache")]
        verify::hit(tree, node, raw_inputs, key, &hit);
        tree.note_observed(observed);
        tree.observe_answer(node, &inputs, &hit, AnswerSource::Cached);
        return verify::poison(&inputs, hit);
    }
    #[cfg(feature = "stats")]
    {
        stats::bump(if inputs.run_mode == RunMode::PerformLayout { &stats::MISS_FINAL } else { &stats::MISS_SIZE });
        if tree.speculation() > 0 {
            stats::bump(&stats::SPECULATIVE);
        }
        if inputs.run_mode == RunMode::ComputeSize {
            stats::bump(if inputs.axis == RequestedAxis::Both { &stats::RUN_SIZE_BOTH } else { &stats::RUN_SIZE_AXIS });
        }
    }
    // What this answer observed, apart from what the caller's other answers did.
    let outer = tree.take_observed();
    let output = compute_uncached(tree, node, inputs, block_ctx);
    let observed = tree.take_observed();
    tree.note_observed(outer | observed);
    #[cfg(feature = "stats")]
    if inputs.run_mode == RunMode::ComputeSize
        && tree.cache(node).entries().iter().any(|(i, o)| i.run_mode == RunMode::ComputeSize && i.axis == inputs.axis && *o == output)
    {
        stats::bump(&stats::COLLISION);
    }
    if tree.speculation() == 0 {
        tree.cache_mut(node).store_observed(key, inputs.run_mode, output, generation, observed);
    }
    tree.observe_answer(node, &inputs, &output, AnswerSource::Computed);
    verify::poison(&inputs, output)
}

/// [`compute_child_layout`]'s answer for a cheap leaf that is not hidden, asked for its layout or
/// size; callers that know a child is one may call it directly when the tree
/// [forwards](LayoutTree::forwards_child_layout) child layout.
#[inline(always)]
pub fn compute_cheap_leaf<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    inputs: LayoutInput,
    block_ctx: Option<&mut BlockContext<'_>>,
) -> LayoutOutput {
    #[cfg(feature = "stats")]
    stats::bump(&stats::CHEAP_LEAF);
    let output = tree.compute_leaf(node, inputs, block_ctx);
    tree.observe_answer(node, &inputs, &output, AnswerSource::CheapLeaf);
    verify::poison(&inputs, output)
}

/// Checks that shortcuts and canonicalised cache hits give the answer a full computation gives.
pub mod verify {
    use super::*;

    /// Without `verify-cache`, the answer unchanged; with it, the axis a single-axis question did
    /// not ask for is NaN, so any caller reading it fails the tests.
    #[inline(always)]
    pub fn poison(inputs: &LayoutInput, output: LayoutOutput) -> LayoutOutput {
        #[cfg(feature = "verify-cache")]
        if inputs.run_mode == RunMode::ComputeSize {
            let mut output = output;
            match inputs.axis {
                RequestedAxis::Horizontal => output.size.height = f32::NAN,
                RequestedAxis::Vertical => output.size.width = f32::NAN,
                RequestedAxis::Both => {}
            }
            return output;
        }
        let _ = inputs;
        output
    }

    /// Recomputes `raw` speculatively and asserts it matches `answer` bit for bit.
    #[cfg(feature = "verify-cache")]
    pub fn shortcut<T: LayoutTree + CacheAccess + ?Sized>(tree: &mut T, node: NodeId, raw: LayoutInput, answer: &LayoutOutput, what: &str) {
        let outer = tree.take_observed();
        let full = speculate(tree, |tree| compute_uncached(tree, node, raw, None));
        tree.take_observed();
        tree.note_observed(outer);
        assert!(
            same_bits(&full, answer, raw.axis, raw.run_mode),
            "{what}: shortcut answer differs for {node:?}\n question: {raw:?}\n shortcut: {answer:?}\n full:     {full:?}"
        );
    }

    #[cfg(feature = "verify-cache")]
    pub(super) fn hit<T: LayoutTree + CacheAccess + ?Sized>(
        tree: &mut T,
        node: NodeId,
        raw: LayoutInput,
        key: crate::tree::cache::CacheKey,
        answer: &LayoutOutput,
    ) {
        // Hits for the very question that was stored need no check; the scheduler covers those.
        if tree.cache(node).stored_key(&key, raw.run_mode) == Some(crate::tree::cache::CacheKey::from_input(&raw)) {
            return;
        }
        // Answers inside a shared block formatting context depend on it; it is not at hand here.
        if tree.query_meta(node).cheap_leaf || raw.context_key != 0 || raw.vertical_margins_are_collapsible != Line::FALSE {
            return;
        }
        shortcut(tree, node, raw, answer, "cache hit");
    }

    /// Bitwise equality of the parts of an answer the question asked for.
    #[cfg(feature = "verify-cache")]
    pub fn same_bits(a: &LayoutOutput, b: &LayoutOutput, axis: RequestedAxis, run_mode: RunMode) -> bool {
        let bits = |v: f32| v.to_bits();
        let w = bits(a.size.width) == bits(b.size.width);
        let h = bits(a.size.height) == bits(b.size.height);
        match (run_mode, axis) {
            (RunMode::ComputeSize, RequestedAxis::Horizontal) => w,
            (RunMode::ComputeSize, RequestedAxis::Vertical) => h,
            _ => {
                let rect = |r: crate::geometry::Rect<f32>| [r.left, r.right, r.top, r.bottom].map(bits);
                let opt = |v: Option<f32>| v.map(bits);
                w && h
                    && rect(a.scrollable_overflow_rect) == rect(b.scrollable_overflow_rect)
                    && rect(a.inflow_overflow_rect) == rect(b.inflow_overflow_rect)
                    && opt(a.baselines.first) == opt(b.baselines.first)
                    && opt(a.baselines.last) == opt(b.baselines.last)
                    && a.top_margin == b.top_margin
                    && a.bottom_margin == b.bottom_margin
                    && a.margins_can_collapse_through == b.margins_can_collapse_through
            }
        }
    }
}

/// Canonicalises a question so equivalent questions share a cache entry.
///
/// A parent size the node never resolves against cannot change the answer, and every algorithm
/// sizes against a definite known dimension in place of the available space on that axis. Beyond
/// that, [`NodeKind`](crate::tree::NodeKind) says which available space the node's algorithm never
/// reads:
///
/// - the default leaf reads none on an axis it knows, and none vertically when the measure
///   function ignores the available height;
/// - block flow never reads the available height, and the width only when it does not know it;
/// - grid reads none on an axis it knows, and treats a min-content height as max-content;
/// - flex, on an axis it knows, only asks whether the space is definite; a single-line container
///   whose main size is unknown never reads the definite main space's value.
#[inline]
pub fn normalize(inputs: &mut LayoutInput, meta: crate::tree::QueryMeta) {
    use crate::tree::NodeKind;
    if !meta.dependency.width {
        inputs.parent_size.width = None;
    }
    if !meta.dependency.height {
        inputs.parent_size.height = None;
    }
    let kind = meta.kind;
    let known = inputs.known_dimensions;
    #[inline(always)]
    fn known_axis(kind: NodeKind, known: f32, definite: bool, avail: AvailableSpace) -> AvailableSpace {
        if definite || (kind != NodeKind::Opaque && !(kind.is_flex() && !avail.is_definite())) {
            AvailableSpace::Definite(known)
        } else if kind.is_flex() {
            AvailableSpace::MaxContent
        } else {
            avail
        }
    }
    if let Some(w) = known.width {
        inputs.available_space.width = known_axis(kind, w, inputs.known_dimensions_are_definite.width, inputs.available_space.width);
    }
    if let Some(h) = known.height {
        inputs.available_space.height = known_axis(kind, h, inputs.known_dimensions_are_definite.height, inputs.available_space.height);
    }
    if kind == NodeKind::Opaque || kind == NodeKind::Leaf {
        return;
    }
    let avail = &mut inputs.available_space;
    match kind {
        NodeKind::LeafIgnoringHeight | NodeKind::Block if known.height.is_none() => avail.height = AvailableSpace::MaxContent,
        NodeKind::Grid if avail.height == AvailableSpace::MinContent => avail.height = AvailableSpace::MaxContent,
        NodeKind::FlexRowSingleLine if known.width.is_none() && avail.width.is_definite() => {
            avail.width = AvailableSpace::Definite(0.0);
        }
        NodeKind::FlexColumnSingleLine if known.height.is_none() && avail.height.is_definite() => {
            avail.height = AvailableSpace::Definite(0.0);
        }
        _ => {}
    }
}

/// Whether `tree` already holds the answer to `request`, without marking it live.
pub fn is_cached<T: LayoutTree + CacheAccess + ?Sized>(tree: &T, request: &ChildRequest) -> bool {
    let meta = tree.query_meta(request.node);
    if meta.hidden || meta.cheap_leaf {
        return false;
    }
    let mut input = request.input;
    normalize(&mut input, meta);
    tree.cache(request.node).holds(&input)
}

/// Runs `f` without caching or writing layouts, for hypothetical passes made while only sizing.
pub fn speculate<T: CacheAccess + ?Sized, R>(tree: &mut T, f: impl FnOnce(&mut T) -> R) -> R {
    let depth = tree.speculation();
    tree.set_speculation(depth + 1);
    let r = f(tree);
    tree.set_speculation(depth);
    r
}

/// Dispatches on display mode without consulting the cache.
pub fn compute_uncached<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    inputs: LayoutInput,
    block_ctx: Option<&mut BlockContext<'_>>,
) -> LayoutOutput {
    let style = tree.style(node);
    let display = style.display;
    let has_children = tree.child_count(node) > 0;
    match (display, has_children) {
        (Display::None, _) => compute_hidden_layout(tree, node),
        (_, false) => tree.compute_leaf(node, inputs, block_ctx),
        (Display::Block, true) => block_observing(tree, |tree| block::compute_block_layout(tree, node, inputs, block_ctx)),
        (Display::FlowRoot, true) => block_observing(tree, |tree| block::compute_block_layout(tree, node, inputs, None)),
        #[cfg(feature = "flex")]
        (Display::Flex, true) => crate::compute::flex::compute_flexbox_layout(tree, node, inputs),
        #[cfg(feature = "grid")]
        (Display::Grid, true) => {
            let output = crate::compute::grid::compute_grid_layout(tree, node, inputs);
            // Grid treats a min-content height as max-content before reading anything; its width
            // is taken as observed.
            tree.take_observed();
            tree.note_observed(OBSERVED_WIDTH);
            output
        }
        #[allow(unreachable_patterns)]
        _ => block_observing(tree, |tree| block::compute_block_layout(tree, node, inputs, None)),
    }
}

/// Runs block layout, keeping only width observations: block flow never reads its available
/// height, and every height its children are asked under is its own choice.
#[inline]
fn block_observing<T: LayoutTree + CacheAccess + ?Sized>(tree: &mut T, f: impl FnOnce(&mut T) -> LayoutOutput) -> LayoutOutput {
    let output = f(tree);
    let observed = tree.take_observed();
    tree.note_observed(observed & OBSERVED_WIDTH);
    output
}

/// Zeroes the subtree's layout; never cached.
pub fn compute_hidden_layout<T: LayoutTree + CacheAccess + ?Sized>(tree: &mut T, node: NodeId) -> LayoutOutput {
    tree.cache_mut(node).clear();
    tree.set_unrounded_layout(node, &Layout::with_order(0));
    let children: scratch::Scratch<NodeId> = scratch::Scratch::collect(tree.children(node));
    for &child in &children {
        compute_child_layout(tree, child, LayoutInput::HIDDEN, None);
    }
    LayoutOutput::HIDDEN
}

/// Lays out the root into `available_space` and stores its layout.
pub fn compute_root_layout<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    root: NodeId,
    available_space: Size<AvailableSpace>,
) {
    let style = tree.style(root);
    let calc = |id, basis| tree.resolve_calc(id, basis);
    let parent_size = available_space.into_options();
    let padding = style.padding.map(|p| p.resolve_or_zero(parent_size.width, &calc));
    let border = style.border.map(|b| b.resolve_or_zero(parent_size.width, &calc));
    let margin = style.margin.map(|m| m.resolve(parent_size.width, &calc));
    let padding_border_size = (padding + border).sum_axes();
    let box_sizing_adjustment =
        if style.box_sizing == crate::style::BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };

    use crate::geometry::MaybeMath;
    let mut known_dimensions = Size::NONE;
    if style.is_block() {
        let resolve = |s: Size<crate::style::Length>| block::resolve_size(s, parent_size, &calc);
        let min_size = resolve(style.min_size.map(|v| v.raw()))
            .maybe_apply_aspect_ratio(style.aspect_ratio)
            .maybe_add(box_sizing_adjustment);
        let max_size = resolve(style.max_size.map(|v| v.raw()))
            .maybe_apply_aspect_ratio(style.aspect_ratio)
            .maybe_add(box_sizing_adjustment);
        let clamped_style_size = resolve(style.size.map(|v| v.raw()))
            .maybe_apply_aspect_ratio(style.aspect_ratio)
            .maybe_add(box_sizing_adjustment)
            .maybe_clamp(min_size, max_size);
        let min_max_definite_size = min_size.zip_map(max_size, |min, max| match (min, max) {
            (Some(min), Some(max)) if max <= min => Some(min),
            _ => None,
        });
        // A block root stretches to a definite available width less its margins.
        let margin_sum = margin.left.unwrap_or(0.0) + margin.right.unwrap_or(0.0);
        let available_space_based_size =
            Size { width: available_space.width.into_option().maybe_sub(margin_sum), height: None };
        known_dimensions = min_max_definite_size
            .or(clamped_style_size)
            .or(available_space_based_size)
            .maybe_max(padding_border_size.map(Some));
    }

    let output = compute_child_layout(
        tree,
        root,
        LayoutInput {
            run_mode: RunMode::PerformLayout,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Both,
            known_dimensions,
            known_dimensions_are_definite: Size::TRUE,
            parent_size,
            available_space,
            vertical_margins_are_collapsible: Line::FALSE,
            context_key: 0,
        },
        None,
    );

    let style = tree.style(root);
    let scrollbar_size = Size {
        width: if style.overflow.y == crate::style::Overflow::Scroll { style.scrollbar_width } else { 0.0 },
        height: if style.overflow.x == crate::style::Overflow::Scroll { style.scrollbar_width } else { 0.0 },
    };
    let x = match (style.direction, available_space.width) {
        (crate::style::Direction::Rtl, AvailableSpace::Definite(w)) => w - output.size.width,
        _ => 0.0,
    };
    tree.set_unrounded_layout(
        root,
        &Layout {
            order: 0,
            location: Point { x, y: 0.0 },
            size: output.size,
            scrollable_overflow_rect: output.scrollable_overflow_rect,
            scrollbar_size,
            border,
            padding,
            margin: margin.map(|m| m.unwrap_or(0.0)),
        },
    );
}

/// A `ComputeSize` question for one axis of a child.
#[inline]
pub fn size_request(
    child: NodeId,
    known_dimensions: Size<Option<f32>>,
    parent_size: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    sizing_mode: SizingMode,
    axis: RequestedAxis,
) -> ChildRequest {
    ChildRequest {
        node: child,
        input: LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode,
            axis,
            known_dimensions,
            known_dimensions_are_definite: Size::TRUE,
            parent_size,
            available_space,
            vertical_margins_are_collapsible: Line::FALSE,
            context_key: 0,
        },
    }
}

/// A `PerformLayout` question for a child whose margins never collapse.
#[inline]
pub fn layout_request(
    child: NodeId,
    known_dimensions: Size<Option<f32>>,
    parent_size: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    sizing_mode: SizingMode,
) -> ChildRequest {
    ChildRequest {
        node: child,
        input: LayoutInput {
            run_mode: RunMode::PerformLayout,
            sizing_mode,
            axis: RequestedAxis::Both,
            known_dimensions,
            known_dimensions_are_definite: Size::TRUE,
            parent_size,
            available_space,
            vertical_margins_are_collapsible: Line::FALSE,
            context_key: 0,
        },
    }
}

/// Independent child questions, answered in the order asked.
///
/// A tree whose executor distributes batches gets them as one batch at [`Questions::finish`];
/// any other tree is asked each question as it is formed, so no request or full answer is kept.
/// Forming a question never reads a cache, so both orders store the same answers.
pub(crate) struct Questions<A: scratch::Pooled + Copy> {
    requests: scratch::Scratch<ChildRequest>,
    answers: scratch::Scratch<A>,
    eager: bool,
    asked: usize,
}

impl<A: scratch::Pooled + Copy> Questions<A> {
    #[inline]
    pub fn new<T: LayoutTree + ?Sized>(tree: &T, capacity: usize) -> Self {
        let eager = !tree.batches_help();
        Self {
            requests: if eager { scratch::Scratch::take() } else { scratch::Scratch::with_capacity(capacity) },
            answers: scratch::Scratch::with_capacity(capacity),
            eager,
            asked: 0,
        }
    }

    /// Asks `request`; `read` keeps the part of the answer the caller needs. Returns its index.
    #[inline]
    pub fn ask<T: LayoutTree + ?Sized>(&mut self, tree: &mut T, request: ChildRequest, read: impl FnOnce(&LayoutOutput) -> A) -> usize {
        if self.eager {
            let output = tree.compute_child_layout(request.node, request.input, None);
            self.answers.push(read(&output));
        } else {
            self.requests.push(request);
        }
        self.asked += 1;
        self.asked - 1
    }

    /// [`Self::ask`] for a child known to be a cheap leaf that is not hidden, of a tree that
    /// [forwards](LayoutTree::forwards_child_layout) child layout.
    #[inline]
    pub fn ask_cheap_leaf<T: LayoutTree + CacheAccess + ?Sized>(
        &mut self,
        tree: &mut T,
        request: ChildRequest,
        read: impl FnOnce(&LayoutOutput) -> A,
    ) -> usize {
        if self.eager {
            let output = compute_cheap_leaf(tree, request.node, request.input, None);
            self.answers.push(read(&output));
        } else {
            self.requests.push(request);
        }
        self.asked += 1;
        self.asked - 1
    }

    /// Answers whatever is still unasked.
    #[inline]
    pub fn finish<T: LayoutTree + ?Sized>(&mut self, tree: &mut T, read: impl Fn(&LayoutOutput) -> A) {
        if self.eager || self.requests.is_empty() {
            return;
        }
        let mut outputs: scratch::Scratch<LayoutOutput> = scratch::Scratch::with_capacity(self.requests.len());
        tree.compute_child_layouts(&self.requests, &mut outputs);
        self.answers.extend(outputs.iter().map(read));
    }

    #[inline]
    pub fn get(&self, index: usize) -> A {
        self.answers[index]
    }

    /// Every answer, in the order asked; complete after [`Self::finish`].
    #[inline]
    pub fn answers(&self) -> &[A] {
        &self.answers
    }
}

/// Convenience queries every container algorithm issues.
pub trait LayoutTreeExt: LayoutTree + CacheAccess {
    /// Measures one axis of a child; the other axis of the answer is unspecified.
    #[inline]
    fn measure_child_size(
        &mut self,
        child: NodeId,
        known_dimensions: Size<Option<f32>>,
        parent_size: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        sizing_mode: SizingMode,
        axis: crate::geometry::AbsoluteAxis,
        vertical_margins_are_collapsible: Line<bool>,
    ) -> f32 {
        compute_child_layout(
            self,
            child,
            LayoutInput {
                run_mode: RunMode::ComputeSize,
                sizing_mode,
                axis: axis.into(),
                known_dimensions,
                known_dimensions_are_definite: Size::TRUE,
                parent_size,
                available_space,
                vertical_margins_are_collapsible,
                context_key: 0,
            },
            None,
        )
        .size
        .get(axis)
    }

    #[inline]
    fn measure_child_size_both(
        &mut self,
        child: NodeId,
        known_dimensions: Size<Option<f32>>,
        parent_size: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        sizing_mode: SizingMode,
        vertical_margins_are_collapsible: Line<bool>,
    ) -> Size<f32> {
        compute_child_layout(
            self,
            child,
            LayoutInput {
                run_mode: RunMode::ComputeSize,
                sizing_mode,
                axis: RequestedAxis::Both,
                known_dimensions,
                known_dimensions_are_definite: Size::TRUE,
                parent_size,
                available_space,
                vertical_margins_are_collapsible,
                context_key: 0,
            },
            None,
        )
        .size
    }

    #[inline]
    fn perform_child_layout(
        &mut self,
        child: NodeId,
        known_dimensions: Size<Option<f32>>,
        parent_size: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        sizing_mode: SizingMode,
        vertical_margins_are_collapsible: Line<bool>,
    ) -> LayoutOutput {
        compute_child_layout(
            self,
            child,
            LayoutInput {
                run_mode: RunMode::PerformLayout,
                sizing_mode,
                axis: RequestedAxis::Both,
                known_dimensions,
                known_dimensions_are_definite: Size::TRUE,
                parent_size,
                available_space,
                vertical_margins_are_collapsible,
                context_key: 0,
            },
            None,
        )
    }
}

impl<T: LayoutTree + CacheAccess + ?Sized> LayoutTreeExt for T {}
