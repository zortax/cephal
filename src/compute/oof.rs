//! Out-of-flow (`absolute` and `fixed`) boxes, laid out by their containing block.
//!
//! A container never lays out its out-of-flow children itself, unless it is their containing
//! block. On its final layout it records, in document order, a *seed* for each out-of-flow child
//! (its static position, relative to the container's border box) and a *through* entry for each
//! in-flow child whose answer says out-of-flow boxes pass through it
//! ([`LayoutOutput::oof`](crate::tree::LayoutOutput)). The record is the container's
//! [`AbsoluteContext`]. At the end of the same final layout, the container lays out the candidates
//! it claims ([`LayoutTree::containing_block_claims`](crate::tree::LayoutTree)), found by walking
//! the records down through the in-flow children that pass candidates through, and reports the
//! rest in its own answer's bits. A box with no containing block below the root is laid out by a
//! last pass against the initial containing block, the viewport.
//!
//! The walk pulls what Taffy pushes through its answers: cephal's answers stay `Copy`. Each
//! laid-out box's [`Layout::location`] stays relative to its parent: the walk knows where the
//! parent sits within the containing block, and subtracts it.
//!
//! The scheduler re-runs only this pass ([`crate::schedule::OOF_PASS`]) when a candidate's static
//! position or its own answer changes without its containing block's in-flow layout changing;
//! the pass then reads the containing block's recorded geometry, so cold and incremental layouts
//! call the same function on the same inputs.

use crate::compute::LayoutTreeExt;
use crate::compute::common::overflow::{scrollable_overflow_contribution, union};
use crate::compute::common::sizing_keyword::{SizingKeywordResolution, resolve_sizing_keyword};
use crate::compute::scratch::Scratch;
use crate::geometry::{AbsoluteAxis, AvailableSpace, Line, MaybeMath, Point, Rect, Size};
use crate::style::{AlignItems, AlignItemsKeyword, AlignmentSafety, BoxSizing, Dimension, Direction, Overflow, Position, Style};
use crate::tree::{CacheAccess, Layout, LayoutInput, LayoutOutput, LayoutTree, NodeId, RequestedAxis, RunMode, SizingMode};

/// Bit of [`LayoutOutput::oof`] and of containing-block claims: `position: absolute`.
pub const OOF_ABSOLUTE: u8 = 1;
/// Bit of [`LayoutOutput::oof`] and of containing-block claims: `position: fixed`.
pub const OOF_FIXED: u8 = 2;
/// Both bits; what the initial containing block claims.
pub const OOF_ALL: u8 = OOF_ABSOLUTE | OOF_FIXED;

/// The bit of an out-of-flow `position`; zero for in-flow ones.
#[inline]
pub fn position_bit(position: Position) -> u8 {
    match position {
        Position::Absolute => OOF_ABSOLUTE,
        Position::Fixed => OOF_FIXED,
        _ => 0,
    }
}

/// The default containing-block rule: a positioned box contains absolutely positioned
/// descendants; only the initial containing block contains fixed ones.
#[inline]
pub fn default_claims(style: &Style) -> u8 {
    if style.position.is_positioned() { OOF_ABSOLUTE } else { 0 }
}

/// The edge of a static-position area an out-of-flow box's margin box is aligned to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisStaticEdge {
    Start,
    Center,
    End,
}

/// The physical alignment of a margin box within a static-position area, in one axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisStaticAlign {
    pub keyword: AxisStaticEdge,
    /// `Safe`: use `fallback` when the margin box overflows the area.
    pub safety: AlignmentSafety,
    pub fallback: AxisStaticEdge,
}

impl AxisStaticAlign {
    #[inline]
    pub const fn from_keyword(keyword: AxisStaticEdge) -> Self {
        Self { keyword, safety: AlignmentSafety::Default, fallback: keyword }
    }
}

/// The static position of an out-of-flow box in one axis: an area and an alignment within it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisStaticPosition {
    pub area: Line<f32>,
    pub align: AxisStaticAlign,
}

impl AxisStaticPosition {
    /// A zero-extent area at `anchor`.
    #[inline]
    pub const fn from_edge(anchor: f32, edge: AxisStaticEdge) -> Self {
        Self { area: Line { start: anchor, end: anchor }, align: AxisStaticAlign::from_keyword(edge) }
    }

    /// Aligns per a self-alignment value whose `self-start`/`self-end` are resolved; `None` is
    /// `normal`, which, like `stretch`, behaves as `start`. `axis_is_rtl` flips `start` and `end`.
    pub fn from_alignment(alignment: Option<AlignItems>, area: Line<f32>, axis_is_rtl: bool) -> Self {
        fn edge_for(keyword: Option<AlignItemsKeyword>, axis_is_rtl: bool) -> AxisStaticEdge {
            let start_position = !matches!(keyword, Some(AlignItemsKeyword::End | AlignItemsKeyword::FlexEnd)) ^ axis_is_rtl;
            match keyword {
                Some(AlignItemsKeyword::Center) => AxisStaticEdge::Center,
                _ if start_position => AxisStaticEdge::Start,
                _ => AxisStaticEdge::End,
            }
        }
        let keyword = alignment.map(|a| a.keyword);
        let safety = alignment.map_or(AlignmentSafety::Default, |a| a.safety);
        let fallback = if safety == AlignmentSafety::Safe { Some(AlignItemsKeyword::Start) } else { keyword };
        Self { area, align: AxisStaticAlign { keyword: edge_for(keyword, axis_is_rtl), safety, fallback: edge_for(fallback, axis_is_rtl) } }
    }

    #[inline]
    fn translate(&mut self, by: f32) {
        self.area.start += by;
        self.area.end += by;
    }
}

/// One entry of a container's out-of-flow record, relative to the container's border box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OofEntry {
    /// An out-of-flow child.
    Seed {
        node: NodeId,
        /// The `order` its layout gets.
        order: u32,
        /// Its [`position_bit`].
        bit: u8,
        static_position: Point<AxisStaticPosition>,
    },
    /// A box whose subtree holds out-of-flow boxes of `bits` that the container does not lay out:
    /// an in-flow child, or a box the container laid out whose candidates it does not claim.
    Through {
        node: NodeId,
        /// The child index, for in-flow children; ordering only.
        order: u32,
        bits: u8,
        /// Its border box within the container's.
        location: Point<f32>,
    },
}

impl OofEntry {
    #[inline]
    pub fn bits(&self) -> u8 {
        match *self {
            Self::Seed { bit, .. } => bit,
            Self::Through { bits, .. } => bits,
        }
    }
    #[inline]
    pub fn order(&self) -> u32 {
        match *self {
            Self::Seed { order, .. } | Self::Through { order, .. } => order,
        }
    }
}

/// What a containing block's out-of-flow boxes are placed against: the border box less borders
/// and scrollbar gutters, relative to the border box.
#[derive(Clone, Debug, PartialEq)]
pub struct OofGeometry {
    pub area_size: Size<f32>,
    pub area_offset: Point<f32>,
    pub direction: Direction,
    /// The containing block's `overflow`.
    pub overflow: Point<Overflow>,
    /// A grid container's tracks: boxes are placed against the grid area their placement names.
    #[cfg(feature = "grid")]
    pub grid: Option<std::sync::Arc<crate::compute::grid::GridGeometry>>,
}

/// A container's out-of-flow record from its last final layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AbsoluteContext {
    /// Out-of-flow children and in-flow children passing candidates through, in document order.
    pub entries: Vec<OofEntry>,
    /// Boxes this container laid out whose subtrees pass candidates on, in the order laid out.
    pub surfaced: Vec<OofEntry>,
    /// Present when the container claims some candidate.
    pub geometry: Option<OofGeometry>,
}

/// A box to lay out, with its static position relative to the containing block's border box.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    node: NodeId,
    order: u32,
    bit: u8,
    static_position: Point<AxisStaticPosition>,
    /// Where the box's parent's border box sits within the containing block's.
    parent_origin: Point<f32>,
}

crate::compute::scratch::pooled!(Candidate);

/// Collects the candidates of `mask` under `entries`, which are relative to a box sitting at
/// `origin`; their static positions come out relative to that box.
fn collect<T: LayoutTree + ?Sized>(tree: &T, entries: &[OofEntry], mask: u8, origin: Point<f32>, out: &mut Scratch<Candidate>) {
    for entry in entries {
        match *entry {
            OofEntry::Seed { node, order, bit, static_position } => {
                if bit & mask != 0 {
                    out.push(Candidate { node, order, bit, static_position, parent_origin: origin });
                }
            }
            OofEntry::Through { node, bits, location, .. } => {
                let mask = bits & mask;
                if mask == 0 {
                    continue;
                }
                let Some(context) = tree.absolute_context(node) else { continue };
                let start = out.len();
                let inner = Point { x: origin.x + location.x, y: origin.y + location.y };
                collect(tree, &context.entries, mask, inner, out);
                collect(tree, &context.surfaced, mask, inner, out);
                // Translated on the way up, level by level, as Taffy's candidates bubble.
                for c in &mut out[start..] {
                    c.static_position.x.translate(location.x);
                    c.static_position.y.translate(location.y);
                }
            }
        }
    }
}

/// Ends a container's final layout: records `entries` (document order, see [`OofEntry`]) and lays
/// out the candidates the container claims. Returns their overflow and the bits passed on.
///
/// `geometry` is asked only when some candidate is claimed. Call outside speculation, with
/// `entries` non-empty.
pub(crate) fn finish<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    entries: Vec<OofEntry>,
    geometry: impl FnOnce() -> OofGeometry,
) -> (Rect<f32>, u8) {
    let claims = tree.containing_block_claims(node);
    let bits = entries.iter().fold(0, |acc, e| acc | e.bits());
    let geometry = if bits & claims != 0 { Some(geometry()) } else { None };
    let mut context = AbsoluteContext { entries, surfaced: Vec::new(), geometry };
    let result = run_pass(tree, node, &mut context, claims);
    tree.set_absolute_context(node, context);
    result
}

/// Ends a container's final layout that found no out-of-flow boxes.
#[inline]
pub(crate) fn finish_none<T: LayoutTree + CacheAccess + ?Sized>(tree: &mut T, node: NodeId) {
    if tree.speculation() == 0 {
        tree.clear_absolute_context(node);
    }
}

/// Lays out the candidates `cb` claims from its record, refilling `context.surfaced`; returns their
/// overflow and the bits `cb` passes on. What these layouts observe is not part of `cb`'s answer.
pub(crate) fn run_pass<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    cb: NodeId,
    context: &mut AbsoluteContext,
    claims: u8,
) -> (Rect<f32>, u8) {
    context.surfaced.clear();
    let mut bits = 0;
    let mut claimed = 0;
    for e in &context.entries {
        bits |= e.bits() & !claims;
        claimed |= e.bits() & claims;
    }
    if claimed == 0 {
        return (Rect::ZERO, bits);
    }
    let Some(geometry) = context.geometry.as_ref() else { return (Rect::ZERO, bits) };
    let mut work: Scratch<Candidate> = Scratch::take();
    collect(tree, &context.entries, claims, Point::ZERO, &mut work);
    let outer = tree.take_observed();
    let adds_overflow = tree.out_of_flow_overflow(cb);
    let overflow = lay_out(tree, Some(cb), &mut work, claims, adds_overflow, geometry, &mut bits, Some(&mut context.surfaced));
    tree.take_observed();
    tree.note_observed(outer);
    (overflow, bits)
}

/// Lays out `work` and every claimed candidate surfacing from it; unclaimed ones are added to
/// `bits` and, when given, recorded in `surfaced`.
#[allow(clippy::too_many_arguments)]
fn lay_out<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    cb: Option<NodeId>,
    work: &mut Scratch<Candidate>,
    claims: u8,
    adds_overflow: u8,
    geometry: &OofGeometry,
    bits: &mut u8,
    mut surfaced: Option<&mut Vec<OofEntry>>,
) -> Rect<f32> {
    let is_scroll_container = geometry.overflow.x.is_scroll_container() || geometry.overflow.y.is_scroll_container();
    let rtl = geometry.direction == Direction::Rtl;
    let mut overflow = Rect::ZERO;
    let mut i = 0;
    while i < work.len() {
        let candidate = work[i];
        i += 1;
        let placed = layout_oof_box(tree, cb, candidate, geometry);
        let (location, size) = (placed.location, placed.size);
        // Measured from the scroll origin: the inline-start edge, the right one in RTL.
        let relative = if rtl {
            Point { x: geometry.area_size.width - (location.x - geometry.area_offset.x) - size.width, y: location.y - geometry.area_offset.y }
        } else {
            Point { x: location.x - geometry.area_offset.x, y: location.y - geometry.area_offset.y }
        };
        if candidate.bit & adds_overflow != 0 {
            overflow = union(
                overflow,
                scrollable_overflow_contribution(relative, size, placed.output.scrollable_overflow_rect, placed.overflow, placed.contain, is_scroll_container),
            );
        }
        let inner = placed.output.oof;
        if inner & !claims != 0 {
            *bits |= inner & !claims;
            if let Some(surfaced) = surfaced.as_deref_mut() {
                surfaced.push(OofEntry::Through { node: candidate.node, order: candidate.order, bits: inner & !claims, location });
            }
        }
        if inner & claims != 0
            && let Some(context) = tree.absolute_context(candidate.node)
        {
            let start = work.len();
            collect(tree, &context.entries, inner & claims, location, work);
            collect(tree, &context.surfaced, inner & claims, location, work);
            for c in &mut work[start..] {
                c.static_position.x.translate(location.x);
                c.static_position.y.translate(location.y);
            }
        }
    }
    overflow
}

/// The last pass of a root layout: lays out every candidate `root` passes on (`bits`) against the
/// initial containing block, `area_size` at `area_offset` from the root's border box. Their
/// overflow is nobody's.
pub(crate) fn initial_containing_block_pass<T: LayoutTree + CacheAccess + ?Sized>(tree: &mut T, root: NodeId, bits: u8, geometry: &OofGeometry) {
    let mut work: Scratch<Candidate> = Scratch::take();
    if let Some(context) = tree.absolute_context(root) {
        collect(tree, &context.entries, bits, Point::ZERO, &mut work);
        collect(tree, &context.surfaced, bits, Point::ZERO, &mut work);
    }
    let mut rest = 0;
    lay_out(tree, None, &mut work, OOF_ALL, 0, geometry, &mut rest, None);
}

/// Lays out the root itself when it is out of flow: against the initial containing block, with its
/// static position at the origin. Returns its answer.
pub(crate) fn layout_root<T: LayoutTree + CacheAccess + ?Sized>(tree: &mut T, root: NodeId, geometry: &OofGeometry) -> LayoutOutput {
    let style = tree.style(root);
    let rtl = geometry.direction == Direction::Rtl;
    let start = if rtl { geometry.area_size.width } else { 0.0 };
    let candidate = Candidate {
        node: root,
        order: 0,
        bit: position_bit(style.position),
        static_position: Point {
            x: AxisStaticPosition {
                area: Line { start, end: start },
                align: AxisStaticAlign::from_keyword(if rtl { AxisStaticEdge::End } else { AxisStaticEdge::Start }),
            },
            y: AxisStaticPosition::from_edge(0.0, AxisStaticEdge::Start),
        },
        parent_origin: Point::ZERO,
    };
    layout_oof_box(tree, None, candidate, geometry).output
}

/// Self-alignment of an out-of-flow box in one axis, relative to its containing block's writing
/// mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OofAlignment {
    /// `normal` or `auto`: stretch-fit for non-replaced, non-table boxes; start alignment.
    Normal,
    Stretch,
    Start,
    Center,
    End,
}

impl OofAlignment {
    fn from_resolved(alignment: Option<AlignItems>) -> Self {
        match alignment.map(|a| a.keyword) {
            None => Self::Normal,
            Some(AlignItemsKeyword::Stretch) => Self::Stretch,
            Some(AlignItemsKeyword::Center) => Self::Center,
            Some(AlignItemsKeyword::End | AlignItemsKeyword::FlexEnd | AlignItemsKeyword::SelfEnd) => Self::End,
            // `baseline` falls back to `start`.
            Some(AlignItemsKeyword::Start | AlignItemsKeyword::FlexStart | AlignItemsKeyword::SelfStart | AlignItemsKeyword::Baseline) => Self::Start,
        }
    }

    /// The physical edge the margin box is aligned to; `normal` and `stretch` behave as `start`.
    fn physical_edge(self, reversed: bool) -> AxisStaticEdge {
        match (self, reversed) {
            (Self::Center, _) => AxisStaticEdge::Center,
            (Self::End, false) | (Self::Normal | Self::Stretch | Self::Start, true) => AxisStaticEdge::End,
            (Self::End, true) | (Self::Normal | Self::Stretch | Self::Start, false) => AxisStaticEdge::Start,
        }
    }
}

#[inline]
fn align_in_line(container: Line<f32>, size: f32, edge: AxisStaticEdge) -> f32 {
    match edge {
        AxisStaticEdge::Start => container.start,
        AxisStaticEdge::End => container.end - size,
        AxisStaticEdge::Center => (container.start + container.end - size) / 2.0,
    }
}

/// The absolute positioning model (css-position-3 §4) in one axis; physical coordinates relative
/// to the start of the containing block's area.
#[derive(Clone, Copy, Debug)]
struct OofAxis {
    cb_size: f32,
    inset: Line<Option<f32>>,
    margin: Line<Option<f32>>,
    static_position: AxisStaticPosition,
    alignment: OofAlignment,
    safety: AlignmentSafety,
    is_inline: bool,
    /// The containing block's start is this axis's physical end (the inline axis of RTL).
    reversed: bool,
    /// The containing block scrolls in this axis.
    is_scroll_axis: bool,
}

impl OofAxis {
    fn start_edge(&self) -> AxisStaticEdge {
        if self.reversed { AxisStaticEdge::End } else { AxisStaticEdge::Start }
    }

    fn end_edge(&self) -> AxisStaticEdge {
        if self.reversed { AxisStaticEdge::Start } else { AxisStaticEdge::End }
    }

    /// The inset-modified containing block (css-position-3 §4.1).
    fn inset_modified_containing_block(&self) -> Line<f32> {
        let (mut imcb, weaker_edge) = match (self.inset.start, self.inset.end) {
            (Some(start), Some(end)) => (Line { start, end: self.cb_size - end }, self.end_edge()),
            // A lone auto inset resolves to zero, and is the weaker one.
            (Some(start), None) => (Line { start, end: self.cb_size }, AxisStaticEdge::End),
            (None, Some(end)) => (Line { start: 0.0, end: self.cb_size - end }, AxisStaticEdge::Start),
            // Both auto: from the static-position rectangle and the alignment within it.
            (None, None) => {
                let sp = self.static_position;
                let imcb = match sp.align.keyword {
                    AxisStaticEdge::Start => Line { start: sp.area.start, end: self.cb_size },
                    AxisStaticEdge::End => Line { start: 0.0, end: sp.area.end },
                    AxisStaticEdge::Center => {
                        // Centred on the area's centre, bounded by the nearer edge.
                        let center = (sp.area.start + sp.area.end) / 2.0;
                        let start_distance = center;
                        let end_distance = self.cb_size - center;
                        if start_distance.abs() <= end_distance.abs() {
                            Line { start: 0.0, end: 2.0 * start_distance }
                        } else {
                            Line { start: self.cb_size - 2.0 * end_distance, end: self.cb_size }
                        }
                    }
                };
                (imcb, self.end_edge())
            }
        };
        // Overconstrained: the weaker inset gives way until the size is zero.
        if imcb.end < imcb.start {
            match weaker_edge {
                AxisStaticEdge::Start => imcb.start = imcb.end,
                _ => imcb.end = imcb.start,
            }
        }
        imcb
    }

    /// The border box's stretch-fit size: the IMCB less the non-auto margins.
    fn stretch_fit_size(&self) -> f32 {
        let imcb = self.inset_modified_containing_block();
        let non_auto_margin_sum = self.margin.start.unwrap_or(0.0) + self.margin.end.unwrap_or(0.0);
        (imcb.end - imcb.start - non_auto_margin_sum).max(0.0)
    }

    /// Whether an `auto` size is the stretch-fit size rather than fit-content (css-position-3 §4.3).
    fn is_stretch_sized(&self, is_replaced: bool, is_table: bool) -> bool {
        self.inset.start.is_some()
            && self.inset.end.is_some()
            && match self.alignment {
                OofAlignment::Stretch => true,
                OofAlignment::Normal => !is_replaced && !is_table,
                _ => false,
            }
    }

    /// Auto margins for `size` (css-position-3 §4.2).
    fn resolve_margins(&self, size: f32) -> Line<f32> {
        let both_insets = self.inset.start.is_some() && self.inset.end.is_some();
        let imcb = self.inset_modified_containing_block();
        let free_space = imcb.end - imcb.start - size;
        match (self.margin.start, self.margin.end) {
            (Some(start), Some(end)) => Line { start, end },
            // With an auto inset, auto margins are zero.
            _ if !both_insets => Line { start: self.margin.start.unwrap_or(0.0), end: self.margin.end.unwrap_or(0.0) },
            (Some(start), None) => Line { start, end: free_space - start },
            (None, Some(end)) => Line { start: free_space - end, end },
            (None, None) => {
                // In the inline axis the end margin absorbs negative free space.
                if free_space < 0.0 && self.is_inline {
                    if self.reversed { Line { start: free_space, end: 0.0 } } else { Line { start: 0.0, end: free_space } }
                } else {
                    Line { start: free_space / 2.0, end: free_space / 2.0 }
                }
            }
        }
    }

    /// The physical start of the border box (css-position-3 §4.4).
    fn position(&self, size: f32, margin: Line<f32>) -> f32 {
        let imcb = self.inset_modified_containing_block();
        match (self.inset.start, self.inset.end) {
            (None, None) => {
                let sp = self.static_position;
                let margin_box_size = size + margin.start + margin.end;
                let offset = if sp.align.safety == AlignmentSafety::Safe && margin_box_size > imcb.end - imcb.start {
                    align_in_line(imcb, margin_box_size, self.start_edge())
                } else {
                    align_in_line(sp.area, margin_box_size, sp.align.keyword)
                };
                offset + margin.start
            }
            // One auto inset: aligned to the edge of the other.
            (Some(_), None) => imcb.start + margin.start,
            (None, Some(_)) => imcb.end - size - margin.end,
            (Some(_), Some(_)) => {
                // Auto margins took all the free space.
                if self.margin.start.is_none() || self.margin.end.is_none() {
                    return imcb.start + margin.start;
                }
                let margin_box_size = size + margin.start + margin.end;
                let imcb_size = imcb.end - imcb.start;
                let edge = self.alignment.physical_edge(self.reversed);
                let margin_box_start = match self.safety {
                    AlignmentSafety::Unsafe => align_in_line(imcb, margin_box_size, edge),
                    AlignmentSafety::Safe => {
                        let edge = if margin_box_size > imcb_size { self.start_edge() } else { edge };
                        align_in_line(imcb, margin_box_size, edge)
                    }
                    // css-align-3 §4.4: stay within the IMCB, then within the containing block.
                    AlignmentSafety::Default => {
                        if margin_box_size <= imcb_size || self.alignment == OofAlignment::Normal {
                            align_in_line(imcb, margin_box_size, edge)
                        } else {
                            // The bounds of the IMCB and the containing block, unbounded in the
                            // scrollable direction of a scroll container.
                            let mut limit = Line { start: imcb.start.min(0.0), end: imcb.end.max(self.cb_size) };
                            if self.is_scroll_axis {
                                if self.reversed {
                                    limit.start = f32::NEG_INFINITY;
                                } else {
                                    limit.end = f32::INFINITY;
                                }
                            }
                            if margin_box_size <= limit.end - limit.start {
                                let start = align_in_line(imcb, margin_box_size, edge);
                                start.min(limit.end - margin_box_size).max(limit.start)
                            } else {
                                align_in_line(limit, margin_box_size, self.start_edge())
                            }
                        }
                    }
                };
                margin_box_start + margin.start
            }
        }
    }
}

/// A laid-out out-of-flow box.
struct Placed {
    /// Relative to the containing block's border box.
    location: Point<f32>,
    size: Size<f32>,
    output: LayoutOutput,
    overflow: Point<Overflow>,
    contain: crate::style::Contain,
}

/// Sizes and places one out-of-flow box against its containing block `cb` (`None`: the initial
/// containing block) and stores its layout, relative to its parent.
fn layout_oof_box<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    cb: Option<NodeId>,
    candidate: Candidate,
    geometry: &OofGeometry,
) -> Placed {
    let node = candidate.node;
    let direction = geometry.direction;
    #[allow(unused_mut)]
    let (mut area_size, mut area_offset) = (geometry.area_size, geometry.area_offset);
    // A grid containing block places the box against the grid area its placement names.
    #[cfg(feature = "grid")]
    if let (Some(grid), Some(cb)) = (geometry.grid.as_deref(), cb) {
        let area = Rect {
            left: area_offset.x,
            right: area_offset.x + area_size.width,
            top: area_offset.y,
            bottom: area_offset.y + area_size.height,
        };
        let style = tree.style(node);
        let (rows, columns) = (style.grid_row, style.grid_column);
        let grid_area = grid.resolve_absolute_grid_area(tree, cb, rows, columns, direction, area);
        area_size = Size { width: grid_area.right - grid_area.left, height: grid_area.bottom - grid_area.top };
        area_offset = Point { x: grid_area.left, y: grid_area.top };
    }
    let _ = cb;
    let area_width = area_size.width;
    let area_height = area_size.height;

    let style = tree.style(node);
    let calc = |id, basis| tree.resolve_calc(id, basis);
    let aspect_ratio = style.aspect_ratio;
    let margin = style.margin.map(|m| m.resolve(Some(area_width), &calc));
    let padding = style.padding.map(|p| p.resolve_or_zero(Some(area_width), &calc));
    let border = style.border.map(|b| b.resolve_or_zero(Some(area_width), &calc));
    let padding_border_sum = (padding + border).sum_axes();
    let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { padding_border_sum } else { Size::ZERO };

    let left = style.inset.left.resolve(Some(area_width), &calc);
    let right = style.inset.right.resolve(Some(area_width), &calc);
    let top = style.inset.top.resolve(Some(area_height), &calc);
    let bottom = style.inset.bottom.resolve(Some(area_height), &calc);

    let size_style = style.size;
    let area = area_size.map(Some);
    let resolve = |s: Size<crate::style::Length>| Size { width: s.width.resolve(area.width, &calc), height: s.height.resolve(area.height, &calc) };
    let style_size = resolve(size_style.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment);
    let mut min_size = resolve(style.min_size.map(|v| v.raw()))
        .maybe_apply_aspect_ratio(aspect_ratio)
        .maybe_add(box_sizing_adjustment)
        .or(padding_border_sum.map(Some))
        .maybe_max(padding_border_sum.map(Some));
    let max_size = resolve(style.max_size.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment);
    let mut known_dimensions = style_size.maybe_clamp(min_size, max_size);

    let is_replaced = style.item_is_replaced;
    let is_table = style.item_is_table;
    // `auto` behaves as `normal`: an out-of-flow box ignores its containing block's `*-items`.
    let justify_self = style.justify_self.map(|a| a.resolve_self_relative(style.direction, direction, true));
    let align_self = style.align_self.map(|a| a.resolve_self_relative(style.direction, direction, false));
    let overflow = style.overflow;
    let scrollbar_width = style.scrollbar_width;
    let contain = style.contain;

    // Static positions relative to the area.
    let mut static_position = candidate.static_position;
    static_position.x.translate(-area_offset.x);
    static_position.y.translate(-area_offset.y);

    let horizontal = OofAxis {
        cb_size: area_width,
        inset: Line { start: left, end: right },
        margin: Line { start: margin.left, end: margin.right },
        static_position: static_position.x,
        alignment: OofAlignment::from_resolved(justify_self),
        safety: justify_self.map_or(AlignmentSafety::Default, |a| a.safety),
        is_inline: true,
        reversed: direction == Direction::Rtl,
        is_scroll_axis: geometry.overflow.x.is_scroll_container(),
    };
    let vertical = OofAxis {
        cb_size: area_height,
        inset: Line { start: top, end: bottom },
        margin: Line { start: margin.top, end: margin.bottom },
        static_position: static_position.y,
        alignment: OofAlignment::from_resolved(align_self),
        safety: align_self.map_or(AlignmentSafety::Default, |a| a.safety),
        is_inline: false,
        reversed: false,
        is_scroll_axis: geometry.overflow.y.is_scroll_container(),
    };
    let stretch_fit_size = Size { width: horizontal.stretch_fit_size(), height: vertical.stretch_fit_size() };
    // A table's available space never exceeds the containing block less its margins.
    let mut available_space = stretch_fit_size;
    if is_table {
        let non_auto_margin = margin.map(|m| m.unwrap_or(0.0));
        available_space = available_space.f32_min(Size {
            width: (area_width - non_auto_margin.horizontal_axis_sum()).max(0.0),
            height: (area_height - non_auto_margin.vertical_axis_sum()).max(0.0),
        });
    }
    let available_space = available_space.maybe_clamp(min_size, max_size).map(AvailableSpace::Definite);

    // Sizing keywords: an explicitly sized axis takes precedence over the automatic size.
    if size_style.width.is_sizing_keyword() || size_style.height.is_sizing_keyword() {
        resolve_sizing_keywords(tree, node, &mut known_dimensions, size_style, area_size, Rect { left, right, top, bottom }, margin);
        known_dimensions = known_dimensions.maybe_apply_aspect_ratio(aspect_ratio).maybe_clamp(min_size, max_size);
    }

    // Automatic sizes (css-position-3 §4.3): stretch-fit where aligned `stretch` (or `normal`)
    // between two insets, first, so the other axis can follow through the aspect ratio; both
    // explicitly stretched ignore it.
    let stretch_width = known_dimensions.width.is_none() && horizontal.is_stretch_sized(is_replaced, is_table);
    let stretch_height = known_dimensions.height.is_none() && vertical.is_stretch_sized(is_replaced, is_table);
    let both_explicit_stretch = horizontal.alignment == OofAlignment::Stretch && vertical.alignment == OofAlignment::Stretch;
    let stretch_aspect_ratio = if stretch_width && stretch_height && both_explicit_stretch { None } else { aspect_ratio };
    if stretch_width {
        if is_table {
            // A table is never stretched below its content: stretch-fit is a minimum.
            min_size.width = Some(min_size.width.unwrap_or(0.0).max(stretch_fit_size.width));
        } else {
            known_dimensions.width = Some(stretch_fit_size.width);
            known_dimensions = known_dimensions.maybe_apply_aspect_ratio(stretch_aspect_ratio).maybe_clamp(min_size, max_size);
        }
    }
    if known_dimensions.height.is_none() && stretch_height {
        if is_table {
            min_size.height = Some(min_size.height.unwrap_or(0.0).max(stretch_fit_size.height));
        } else {
            known_dimensions.height = Some(stretch_fit_size.height);
            known_dimensions = known_dimensions.maybe_apply_aspect_ratio(stretch_aspect_ratio).maybe_clamp(min_size, max_size);
        }
    }

    let final_size = match (known_dimensions.width, known_dimensions.height) {
        (Some(width), Some(height)) => Size { width, height },
        // The width first (CSS 2.2 §10.3.7), then the height at that width (§10.6.4), which
        // percentages inside then resolve against.
        (None, None) => {
            let measured = tree.measure_child_size(
                node,
                known_dimensions,
                area,
                available_space,
                SizingMode::ContentSize,
                crate::geometry::AbsoluteAxis::Horizontal,
                Line::FALSE,
            );
            let width = measured.maybe_clamp(min_size.width, max_size.width);
            let height = tree.measure_child_size(
                node,
                Size { width: Some(width), height: None },
                area,
                available_space,
                SizingMode::ContentSize,
                crate::geometry::AbsoluteAxis::Vertical,
                Line::FALSE,
            );
            Size { width, height }
        }
        _ => {
            let measured =
                tree.measure_child_size_both(node, known_dimensions, area, available_space, SizingMode::ContentSize, Line::FALSE);
            known_dimensions.unwrap_or(measured)
        }
    }
    .maybe_clamp(min_size, max_size);

    let output = tree.compute_child_layout(
        node,
        LayoutInput {
            run_mode: RunMode::PerformLayout,
            sizing_mode: SizingMode::ContentSize,
            axis: RequestedAxis::Both,
            known_dimensions: final_size.map(Some),
            known_dimensions_are_definite: Size::TRUE,
            parent_size: area,
            available_space,
            vertical_margins_are_collapsible: Line::FALSE,
            context_key: 0,
        },
        None,
    );

    let horizontal_margin = horizontal.resolve_margins(final_size.width);
    let vertical_margin = vertical.resolve_margins(final_size.height);
    let resolved_margin = Rect { left: horizontal_margin.start, right: horizontal_margin.end, top: vertical_margin.start, bottom: vertical_margin.end };
    let location = Point {
        x: horizontal.position(final_size.width, horizontal_margin) + area_offset.x,
        y: vertical.position(final_size.height, vertical_margin) + area_offset.y,
    };
    let scrollbar_size = Size {
        width: if overflow.y == Overflow::Scroll { scrollbar_width } else { 0.0 },
        height: if overflow.x == Overflow::Scroll { scrollbar_width } else { 0.0 },
    };
    let origin = candidate.parent_origin;
    tree.set_unrounded_layout(
        node,
        &Layout {
            order: candidate.order,
            location: Point { x: location.x - origin.x, y: location.y - origin.y },
            size: final_size,
            scrollable_overflow_rect: output.scrollable_overflow_rect,
            scrollbar_size,
            border,
            padding,
            margin: resolved_margin,
        },
    );
    let _ = candidate.bit;
    Placed { location, size: final_size, output, overflow, contain }
}

/// `min-content`, `max-content`, `fit-content` and `stretch` sizes of an out-of-flow box.
fn resolve_sizing_keywords<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    known_dimensions: &mut Size<Option<f32>>,
    size_style: Size<Dimension>,
    area_size: Size<f32>,
    inset: Rect<Option<f32>>,
    margin: Rect<Option<f32>>,
) {
    let stretch = Size {
        width: (area_size.width - inset.left.unwrap_or(0.0) - inset.right.unwrap_or(0.0) - margin.left.unwrap_or(0.0) - margin.right.unwrap_or(0.0))
            .max(0.0),
        height: (area_size.height - inset.top.unwrap_or(0.0) - inset.bottom.unwrap_or(0.0) - margin.top.unwrap_or(0.0) - margin.bottom.unwrap_or(0.0))
            .max(0.0),
    };
    let area = area_size.map(Some);
    let keyword_width = if known_dimensions.width.is_none() {
        resolve_sizing_keyword(size_style.width.raw(), Some(stretch.width), Some(area_size.width))
    } else {
        None
    };
    let keyword_height = if known_dimensions.height.is_none() {
        resolve_sizing_keyword(size_style.height.raw(), Some(stretch.height), Some(area_size.height))
    } else {
        None
    };
    use SizingKeywordResolution::*;
    match (keyword_width, keyword_height) {
        // Both measured: one question.
        (Some(Measure(aw)), Some(Measure(ah))) => {
            let measured =
                tree.measure_child_size_both(node, Size::NONE, area, Size { width: aw, height: ah }, SizingMode::ContentSize, Line::FALSE);
            *known_dimensions = measured.map(Some);
        }
        (kw, kh) => {
            if let Some(r) = kw {
                known_dimensions.width = Some(match r {
                    Exact(w) => w,
                    Measure(aw) => tree.measure_child_size(
                        node,
                        *known_dimensions,
                        area,
                        Size { width: aw, height: AvailableSpace::Definite(stretch.height) },
                        SizingMode::ContentSize,
                        AbsoluteAxis::Horizontal,
                        Line::FALSE,
                    ),
                });
            }
            if let Some(r) = kh {
                known_dimensions.height = Some(match r {
                    Exact(h) => h,
                    Measure(ah) => tree.measure_child_size(
                        node,
                        *known_dimensions,
                        area,
                        Size { width: known_dimensions.width.map_or(AvailableSpace::Definite(stretch.width), AvailableSpace::Definite), height: ah },
                        SizingMode::ContentSize,
                        AbsoluteAxis::Vertical,
                        Line::FALSE,
                    ),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(cb_size: f32, inset: Line<Option<f32>>, sp: AxisStaticPosition) -> OofAxis {
        OofAxis {
            cb_size,
            inset,
            margin: Line { start: Some(0.0), end: Some(0.0) },
            static_position: sp,
            alignment: OofAlignment::Normal,
            safety: AlignmentSafety::Default,
            is_inline: true,
            reversed: false,
            is_scroll_axis: false,
        }
    }

    #[test]
    fn static_position_alignment_sets_the_imcb() {
        let start = axis(100.0, Line { start: None, end: None }, AxisStaticPosition::from_edge(40.0, AxisStaticEdge::Start));
        assert_eq!(start.inset_modified_containing_block(), Line { start: 40.0, end: 100.0 });
        assert_eq!(start.position(20.0, Line { start: 0.0, end: 0.0 }), 40.0);
        let end = axis(100.0, Line { start: None, end: None }, AxisStaticPosition::from_edge(40.0, AxisStaticEdge::End));
        assert_eq!(end.inset_modified_containing_block(), Line { start: 0.0, end: 40.0 });
        assert_eq!(end.position(20.0, Line { start: 0.0, end: 0.0 }), 20.0);
    }

    #[test]
    fn overconstrained_insets_shrink_the_weaker_side() {
        let a = axis(100.0, Line { start: Some(80.0), end: Some(50.0) }, AxisStaticPosition::from_edge(0.0, AxisStaticEdge::Start));
        assert_eq!(a.inset_modified_containing_block(), Line { start: 80.0, end: 80.0 });
    }

    #[test]
    fn default_safety_keeps_an_overflowing_box_within_the_containing_block() {
        let mut a = axis(100.0, Line { start: Some(40.0), end: Some(40.0) }, AxisStaticPosition::from_edge(0.0, AxisStaticEdge::Start));
        a.alignment = OofAlignment::Center;
        // 60 wide in a 20-wide IMCB centred at 50: unsafe would start at 20, the limit is [0, 100].
        assert_eq!(a.position(60.0, Line { start: 0.0, end: 0.0 }), 20.0);
        a.alignment = OofAlignment::End;
        // End-aligned to 60 would start at 0; still within the limit.
        assert_eq!(a.position(60.0, Line { start: 0.0, end: 0.0 }), 0.0);
        // 120 wide does not fit the limit: start-aligned to it.
        assert_eq!(a.position(120.0, Line { start: 0.0, end: 0.0 }), 0.0);
    }
}
