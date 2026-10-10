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
    pub fn bump(c: &AtomicUsize) {
        c.fetch_add(1, Relaxed);
    }
    pub fn report() -> String {
        let all = [("hidden", &HIDDEN), ("cheap_leaf", &CHEAP_LEAF), ("hit_size", &HIT_SIZE), ("hit_final", &HIT_FINAL), ("miss_size", &MISS_SIZE), ("miss_final", &MISS_FINAL), ("measure", &MEASURE), ("speculative", &SPECULATIVE), ("relaxed_hit", &RELAXED_HIT), ("batches", &BATCHES), ("plan_few_requests", &PLAN_FEW_REQUESTS), ("plan_few_groups", &PLAN_FEW_GROUPS), ("plan_little_work", &PLAN_LITTLE_WORK), ("batch_us", &BATCH_NANOS), ("batch_requests", &BATCH_REQUESTS)];
        let s = all.iter().map(|(n, c)| format!("{n}={}", c.swap(0, Relaxed))).collect::<Vec<_>>().join(" ");
        s
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
        #[cfg(feature = "stats")]
        stats::bump(&stats::CHEAP_LEAF);
        let output = tree.compute_leaf(node, inputs, block_ctx);
        tree.observe_answer(node, &inputs, &output, AnswerSource::CheapLeaf);
        return output;
    }
    normalize(&mut inputs, meta);
    let generation = tree.generation();
    let key = crate::tree::cache::CacheKey::from_input(&inputs);
    if let Some(hit) = tree.cache(node).get_keyed(&key, inputs.run_mode, generation) {
        #[cfg(feature = "stats")]
        stats::bump(if inputs.run_mode == RunMode::PerformLayout { &stats::HIT_FINAL } else { &stats::HIT_SIZE });
        tree.observe_answer(node, &inputs, &hit, AnswerSource::Cached);
        return hit;
    }
    #[cfg(feature = "stats")]
    {
        stats::bump(if inputs.run_mode == RunMode::PerformLayout { &stats::MISS_FINAL } else { &stats::MISS_SIZE });
        if tree.speculation() > 0 {
            stats::bump(&stats::SPECULATIVE);
        }
    }
    let output = compute_uncached(tree, node, inputs, block_ctx);
    if tree.speculation() == 0 {
        tree.cache_mut(node).store_keyed(key, inputs.run_mode, output, generation);
    }
    tree.observe_answer(node, &inputs, &output, AnswerSource::Computed);
    output
}

/// Canonicalises a question so equivalent questions share a cache entry.
///
/// A parent size the node never resolves against cannot change the answer, and every algorithm
/// sizes against a definite known dimension in place of the available space on that axis.
#[inline]
pub fn normalize(inputs: &mut LayoutInput, meta: crate::tree::QueryMeta) {
    if !meta.dependency.width {
        inputs.parent_size.width = None;
    }
    if !meta.dependency.height {
        inputs.parent_size.height = None;
    }
    if let Some(w) = inputs.known_dimensions.width
        && inputs.known_dimensions_are_definite.width
    {
        inputs.available_space.width = AvailableSpace::Definite(w);
    }
    if let Some(h) = inputs.known_dimensions.height
        && inputs.known_dimensions_are_definite.height
    {
        inputs.available_space.height = AvailableSpace::Definite(h);
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
        (Display::Block, true) => block::compute_block_layout(tree, node, inputs, block_ctx),
        (Display::FlowRoot, true) => block::compute_block_layout(tree, node, inputs, None),
        #[cfg(feature = "flex")]
        (Display::Flex, true) => crate::compute::flex::compute_flexbox_layout(tree, node, inputs),
        #[cfg(feature = "grid")]
        (Display::Grid, true) => crate::compute::grid::compute_grid_layout(tree, node, inputs),
        #[allow(unreachable_patterns)]
        _ => block::compute_block_layout(tree, node, inputs, None),
    }
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
