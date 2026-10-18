//! Hooks a tree implementation provides.

use super::cache::NodeCache;
use super::{DetailedGridInfo, Layout, LayoutInput, LayoutOutput, NodeId};
use crate::compute::block::BlockContext;
use crate::compute::reposition::AbsoluteContext;
use crate::geometry::{AvailableSpace, Size};
use crate::style::{CalcId, Ident, Style};

/// Implicit grid line name suffixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentSuffix {
    Start,
    End,
}

impl IdentSuffix {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "-start",
            Self::End => "-end",
        }
    }
}

/// What a leaf is asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasureInput {
    /// Content-box dimensions the caller already fixed.
    pub known_dimensions: Size<Option<f32>>,
    /// Content-box available space.
    pub available_space: Size<AvailableSpace>,
}

/// What a leaf answers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasureOutput {
    /// Content-box size.
    pub size: Size<f32>,
    /// Baselines from the content-box top.
    pub baselines: super::Baselines,
}

impl MeasureOutput {
    #[inline]
    pub const fn from_size(size: Size<f32>) -> Self {
        Self { size, baselines: super::Baselines::NONE }
    }
}

/// One child question in a batch of independent ones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChildRequest {
    pub node: NodeId,
    pub input: LayoutInput,
}

/// What [`crate::compute::compute_child_layout`] needs to know about a node before probing its
/// cache; implementations may keep it in one word per node.
#[derive(Clone, Copy, Debug)]
pub struct QueryMeta {
    /// `display: none`.
    pub hidden: bool,
    /// A leaf with nothing to measure.
    pub cheap_leaf: bool,
    /// See [`Style::parent_size_dependency`].
    pub dependency: Size<bool>,
    /// The algorithm that answers the node's questions; lets equivalent questions share a key.
    pub kind: NodeKind,
}

/// Which algorithm answers a node's questions, so [`crate::compute::normalize`] can drop inputs it
/// never reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum NodeKind {
    /// Unknown; questions are cached as asked.
    #[default]
    Opaque,
    /// The default leaf algorithm around [`LayoutTree::measure`].
    Leaf,
    /// [`Self::Leaf`] whose measurement never depends on the available height.
    LeafIgnoringHeight,
    /// Block flow with children.
    Block,
    /// A flex container with children; `SingleLine` for `nowrap`.
    FlexRow,
    FlexColumn,
    FlexRowSingleLine,
    FlexColumnSingleLine,
    /// A grid container with children.
    Grid,
}

impl NodeKind {
    /// The kind of a container from its style; leaves are opaque, since `compute_leaf` may be
    /// overridden.
    pub fn of_container(style: &Style) -> Self {
        use crate::style::Display;
        match style.display {
            Display::Flex => match (style.flex_direction.is_row(), style.flex_wrap.is_multi_line()) {
                (true, true) => Self::FlexRow,
                (false, true) => Self::FlexColumn,
                (true, false) => Self::FlexRowSingleLine,
                (false, false) => Self::FlexColumnSingleLine,
            },
            Display::Grid => Self::Grid,
            Display::None => Self::Opaque,
            _ => Self::Block,
        }
    }

    #[inline]
    pub fn is_flex(self) -> bool {
        matches!(self, Self::FlexRow | Self::FlexColumn | Self::FlexRowSingleLine | Self::FlexColumnSingleLine)
    }
}

/// A tree the layout algorithms can walk and write into.
pub trait LayoutTree {
    type ChildIter<'a>: Iterator<Item = NodeId>
    where
        Self: 'a;

    fn children(&self, node: NodeId) -> Self::ChildIter<'_>;
    fn child_count(&self, node: NodeId) -> usize;
    fn child_at(&self, node: NodeId, index: usize) -> NodeId;
    fn style(&self, node: NodeId) -> &Style;

    fn resolve_calc(&self, _id: CalcId, _basis: f32) -> f32 {
        0.0
    }

    /// The ident for `"<base>-start"` / `"<base>-end"`, when one was interned (grid areas).
    fn suffixed_ident(&self, _base: Ident, _suffix: IdentSuffix) -> Option<Ident> {
        None
    }

    /// Content measurement for leaves; the default is an empty box.
    fn measure(&mut self, _node: NodeId, input: MeasureInput) -> MeasureOutput {
        MeasureOutput::from_size(input.known_dimensions.unwrap_or(Size::ZERO))
    }

    /// Whether measuring `node` is cheap enough that caching its answers is a net loss.
    fn leaf_is_cheap(&self, _node: NodeId) -> bool {
        false
    }

    /// Overridable leaf algorithm; the default applies the box model around [`LayoutTree::measure`].
    fn compute_leaf(&mut self, node: NodeId, inputs: LayoutInput, _block_ctx: Option<&mut BlockContext<'_>>) -> LayoutOutput {
        crate::compute::leaf::compute_leaf_layout(self, node, inputs)
    }

    /// The node's style lengths resolved once, when none resolves against a parent size;
    /// implementations may keep one per interned style. See [`crate::compute::plan::StylePlan`].
    #[inline]
    fn style_plan(&self, _node: NodeId) -> Option<&crate::compute::plan::StylePlan> {
        None
    }

    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout);

    /// The layout stored by the last `set_unrounded_layout`; enables fast re-placement.
    fn unrounded_layout(&self, _node: NodeId) -> Option<Layout> {
        None
    }

    fn set_detailed_grid_info(&mut self, _node: NodeId, _info: DetailedGridInfo) {}

    /// The per-query facts about `node`; the default derives them from the style and children.
    #[inline]
    fn query_meta(&self, node: NodeId) -> QueryMeta {
        let style = self.style(node);
        let leaf = self.child_count(node) == 0;
        QueryMeta {
            hidden: style.generates_no_box(),
            cheap_leaf: leaf && self.leaf_is_cheap(node),
            dependency: style.parent_size_dependency(),
            kind: if leaf { NodeKind::Opaque } else { NodeKind::of_container(style) },
        }
    }

    /// Returns the axes (`compute::OBSERVED_WIDTH`/`OBSERVED_HEIGHT`) on which answers computed since
    /// the last call told min-content from max-content available space, and starts afresh.
    /// [`crate::compute::compute_child_layout`] stores them with each answer so a question
    /// differing only there can share it. Without storage, every axis counts as observed and
    /// questions are cached as asked.
    #[inline]
    fn take_observed(&mut self) -> u8 {
        crate::compute::OBSERVED_ALL
    }

    /// Adds `axes` to what [`Self::take_observed`] will return.
    #[inline]
    fn note_observed(&mut self, _axes: u8) {}

    /// Whether [`Self::measure`] never depends on the available height, as with text that wraps
    /// at the available width only. Lets leaves share answers across available heights.
    #[inline]
    fn measure_ignores_available_height(&self) -> bool {
        false
    }

    /// Whether grid containers should produce [`DetailedGridInfo`]; it costs allocations per layout.
    fn wants_detailed_grid_info(&self) -> bool {
        false
    }

    /// The grid info stored by the last `set_detailed_grid_info`; enables fast re-placement.
    fn detailed_grid_info(&self, _node: NodeId) -> Option<DetailedGridInfo> {
        None
    }

    /// Records the geometry a container used to place its absolutely positioned children.
    fn set_absolute_context(&mut self, _node: NodeId, _context: AbsoluteContext) {}

    /// The context stored by the last `set_absolute_context`; enables fast re-placement.
    fn absolute_context(&self, _node: NodeId) -> Option<AbsoluteContext> {
        None
    }

    /// Lays out one child; implementations normally forward to [`crate::compute::compute_child_layout`].
    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput, block_ctx: Option<&mut BlockContext<'_>>) -> LayoutOutput;

    /// Whether [`Self::compute_child_layout`] only forwards to
    /// [`crate::compute::compute_child_layout`], so an algorithm that knows a child is a cheap leaf
    /// may answer it as that function would, without the call.
    #[inline]
    fn forwards_child_layout(&self) -> bool {
        false
    }

    /// Sees every answer [`crate::compute::compute_child_layout`] hands out, before the asking
    /// algorithm reads it; hidden answers excepted. Runs during speculative passes too (see
    /// [`CacheAccess::speculation`]) and on whichever thread answered. For counting and recording
    /// only: the answer is fixed by its question, and nothing a later query reads may change here.
    #[inline]
    fn observe_answer(&mut self, _node: NodeId, _inputs: &LayoutInput, _output: &LayoutOutput, _source: AnswerSource) {}

    /// Whether [`Self::compute_child_layouts`] may beat asking one by one; algorithms then spend
    /// extra work collecting questions that a serial tree would rather answer lazily.
    #[inline]
    fn batches_help(&self) -> bool {
        false
    }

    /// Answers independent child questions, in order; a parallel implementation may distribute them.
    fn compute_child_layouts(&mut self, requests: &[ChildRequest], out: &mut Vec<LayoutOutput>) {
        out.clear();
        for r in requests {
            let output = self.compute_child_layout(r.node, r.input, None);
            out.push(output);
        }
    }
}

/// Where an answer passed to [`LayoutTree::observe_answer`] came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnswerSource {
    /// Recomputed because the node is a leaf cheaper to measure than to look up; never cached.
    CheapLeaf,
    /// Served from the node's cache.
    Cached,
    /// Computed by the node's algorithm, and stored unless speculating.
    Computed,
}

/// Per-node query caches.
pub trait CacheAccess {
    fn cache(&self, node: NodeId) -> &NodeCache;
    fn cache_mut(&mut self, node: NodeId) -> &mut NodeCache;
    /// The frame counter used to mark live cache entries.
    fn generation(&self) -> u32;
    /// Nesting depth of speculative computation; while positive, nothing is cached or written.
    fn speculation(&self) -> u32;
    fn set_speculation(&mut self, depth: u32);
}
