//! Shared random tree generation for differential and incremental tests.
#![allow(dead_code)]

use cephal::geometry::{AvailableSpace, Line, Point, Rect, Size};
use cephal::style::{
    AlignContent, AlignItems, Dimension, Display, FlexDirection, FlexWrap, GridPlacement, GridTemplateComponent,
    LengthPercentage, LengthPercentageAuto, Overflow, Position, Style, TrackSizingFunction,
};
use cephal::tree::{MeasureInput, MeasureOutput, NodeId};
use cephal::Tree;

/// Deterministic xorshift generator.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }
    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }
}

/// Leaf content: `n` glyphs of 10px that wrap at the available width.
#[derive(Clone, Copy)]
pub struct Text(pub u32);

pub fn measure(ctx: &mut Text, input: MeasureInput, _: &Style) -> MeasureOutput {
    let glyph = 10.0;
    let total = ctx.0 as f32 * glyph;
    let width = input.known_dimensions.width.unwrap_or_else(|| match input.available_space.width {
        AvailableSpace::MinContent => glyph,
        AvailableSpace::MaxContent => total,
        AvailableSpace::Definite(w) => w.min(total).max(glyph),
    });
    let per_line = (width / glyph).floor().max(1.0);
    let lines = (ctx.0 as f32 / per_line).ceil().max(1.0);
    let height = input.known_dimensions.height.unwrap_or(lines * glyph);
    MeasureOutput::from_size(Size { width, height })
}

pub fn dimension(rng: &mut Rng) -> Dimension {
    match rng.below(6) {
        0 | 1 => Dimension::AUTO,
        2 => Dimension::length(rng.below(200) as f32),
        3 => Dimension::percent(rng.below(100) as f32 / 100.0),
        4 => rng.pick(&[Dimension::MIN_CONTENT, Dimension::MAX_CONTENT, Dimension::FIT_CONTENT, Dimension::STRETCH]),
        _ => Dimension::fit_content_px(rng.below(150) as f32),
    }
}

pub fn lpa(rng: &mut Rng) -> LengthPercentageAuto {
    match rng.below(4) {
        0 => LengthPercentageAuto::AUTO,
        1 => LengthPercentageAuto::length(rng.below(30) as f32),
        2 => LengthPercentageAuto::percent(rng.below(20) as f32 / 100.0),
        _ => LengthPercentageAuto::ZERO,
    }
}

pub fn lp(rng: &mut Rng) -> LengthPercentage {
    match rng.below(3) {
        0 => LengthPercentage::length(rng.below(20) as f32),
        1 => LengthPercentage::percent(rng.below(10) as f32 / 100.0),
        _ => LengthPercentage::ZERO,
    }
}

pub fn track(rng: &mut Rng) -> TrackSizingFunction {
    match rng.below(5) {
        0 => TrackSizingFunction::AUTO,
        1 => TrackSizingFunction::length(rng.below(120) as f32 + 10.0),
        2 => TrackSizingFunction::fr(rng.below(3) as f32 + 1.0),
        3 => TrackSizingFunction::MIN_CONTENT,
        _ => TrackSizingFunction::percent(rng.below(50) as f32 / 100.0),
    }
}

pub fn random_style(rng: &mut Rng, container: bool) -> Style {
    let mut s = Style::DEFAULT;
    s.display = if container { rng.pick(&[Display::Block, Display::Flex, Display::Flex, Display::Grid]) } else { Display::Block };
    if rng.chance(1, 20) {
        s.display = Display::None;
    }
    s.size = Size { width: dimension(rng), height: dimension(rng) };
    if rng.chance(1, 4) {
        s.min_size = Size { width: lpa(rng), height: lpa(rng) };
    }
    if rng.chance(1, 4) {
        s.max_size = Size { width: lpa(rng), height: lpa(rng) };
    }
    if rng.chance(1, 3) {
        s.margin = Rect { left: lpa(rng), right: lpa(rng), top: lpa(rng), bottom: lpa(rng) };
    }
    if rng.chance(1, 3) {
        s.padding = Rect { left: lp(rng), right: lp(rng), top: lp(rng), bottom: lp(rng) };
    }
    if rng.chance(1, 4) {
        s.border = Rect::splat(LengthPercentage::length(rng.below(5) as f32));
    }
    if rng.chance(1, 5) {
        s.gap = Size { width: lp(rng), height: lp(rng) };
    }
    if rng.chance(1, 8) {
        s.aspect_ratio = Some(rng.pick(&[0.5, 1.0, 2.0]));
    }
    // Out-of-flow boxes under static, relative and sticky containers, so containing blocks are
    // parents, further ancestors, other out-of-flow boxes and the initial containing block.
    match rng.below(16) {
        0 | 1 => s.position = Position::Absolute,
        2 => s.position = Position::Fixed,
        3 | 4 if container => s.position = Position::Relative,
        5 if container => s.position = Position::Sticky,
        _ => {}
    }
    if s.position != Position::Static {
        s.inset = Rect { left: lpa(rng), right: lpa(rng), top: lpa(rng), bottom: lpa(rng) };
    }
    if rng.chance(1, 8) {
        s.overflow = Point { x: rng.pick(&[Overflow::Visible, Overflow::Hidden, Overflow::Scroll]), y: Overflow::Scroll };
        s.scrollbar_width = 15.0;
    }
    s.flex_direction = rng.pick(&[FlexDirection::Row, FlexDirection::Column, FlexDirection::RowReverse]);
    s.flex_wrap = rng.pick(&[FlexWrap::NoWrap, FlexWrap::NoWrap, FlexWrap::Wrap, FlexWrap::WrapReverse]);
    s.flex_grow = rng.pick(&[0.0, 0.0, 1.0, 2.0]);
    s.flex_shrink = rng.pick(&[1.0, 1.0, 0.0]);
    s.flex_basis = if rng.chance(1, 4) { dimension(rng) } else { Dimension::AUTO };
    s.align_items = rng.pick(&[None, None, Some(AlignItems::CENTER), Some(AlignItems::FLEX_END), Some(AlignItems::BASELINE)]);
    s.align_self = rng.pick(&[None, None, None, Some(AlignItems::STRETCH), Some(AlignItems::CENTER)]);
    s.justify_content = rng.pick(&[None, None, Some(AlignContent::CENTER), Some(AlignContent::SPACE_BETWEEN), Some(AlignContent::END)]);
    s.align_content = rng.pick(&[None, None, Some(AlignContent::SPACE_AROUND), Some(AlignContent::CENTER)]);
    if s.display == Display::Grid {
        let g = s.grid_mut();
        g.template_columns = (0..rng.below(3) + 1).map(|_| GridTemplateComponent::Single(track(rng))).collect();
        g.template_rows = (0..rng.below(3)).map(|_| GridTemplateComponent::Single(track(rng))).collect();
        g.auto_rows = vec![track(rng)];
    }
    if rng.chance(1, 4) {
        s.grid_column = Line { start: GridPlacement::Span(rng.below(2) as u16 + 1), end: GridPlacement::Auto };
    }
    if rng.chance(1, 6) {
        s.grid_row = Line { start: GridPlacement::Line(rng.below(3) as i16 + 1), end: GridPlacement::Auto };
    }
    s
}

pub fn build_subtree(rng: &mut Rng, tree: &mut Tree<Text>, depth: u32) -> NodeId {
    let children = if depth >= 4 { 0 } else { rng.below(4) };
    if children == 0 {
        let style = random_style(rng, false);
        if rng.chance(2, 3) { tree.new_leaf_with_context(style, Text(rng.below(12) as u32 + 1)) } else { tree.new_leaf(style) }
    } else {
        let node = tree.new_leaf(random_style(rng, true));
        for _ in 0..children {
            let child = build_subtree(rng, tree, depth + 1);
            tree.append_child(node, child);
        }
        node
    }
}


pub fn all_nodes(tree: &Tree<Text>, root: NodeId, out: &mut Vec<NodeId>) {
    out.push(root);
    for c in tree.children(root) {
        all_nodes(tree, c, out);
    }
}

/// Builds the same tree in taffy; returns the taffy root and the node mapping in `all_nodes` order.
pub fn mirror_in_taffy(tree: &Tree<Text>, root: NodeId) -> (taffy::TaffyTree<Text>, taffy::NodeId, Vec<(NodeId, taffy::NodeId)>) {
    let mut t: taffy::TaffyTree<Text> = taffy::TaffyTree::new();
    let mut map = Vec::new();
    fn go(tree: &Tree<Text>, node: NodeId, t: &mut taffy::TaffyTree<Text>, map: &mut Vec<(NodeId, taffy::NodeId)>) -> taffy::NodeId {
        let style = cephal::compat::taffy::to_taffy(tree.style(node), &|i| tree.ident_name(i).to_owned());
        let id = match tree.context(node) {
            Some(ctx) => t.new_leaf_with_context(style, *ctx).unwrap(),
            None => t.new_leaf(style).unwrap(),
        };
        map.push((node, id));
        for c in tree.children(node) {
            let child = go(tree, c, t, map);
            t.add_child(id, child).unwrap();
        }
        id
    }
    let troot = go(tree, root, &mut t, &mut map);
    (t, troot, map)
}

/// The same leaf measurement, for taffy.
pub fn taffy_measure(
    inputs: taffy::LayoutInput,
    _node: taffy::NodeId,
    context: Option<&mut Text>,
    style: &taffy::Style,
) -> taffy::LayoutOutput {
    taffy::compute_leaf_layout(inputs, style, |_, _| 0.0, |known, available| {
        let Some(ctx) = context else { return known.map(|d| d.unwrap_or(0.0)) };
        let glyph = 10.0;
        let total = ctx.0 as f32 * glyph;
        let width = known.width.unwrap_or_else(|| match available.width {
            taffy::AvailableSpace::MinContent => glyph,
            taffy::AvailableSpace::MaxContent => total,
            taffy::AvailableSpace::Definite(w) => w.min(total).max(glyph),
        });
        let per_line = (width / glyph).floor().max(1.0);
        let lines = (ctx.0 as f32 / per_line).ceil().max(1.0);
        let height = known.height.unwrap_or(lines * glyph);
        taffy::Size { width, height }
    })
}

pub fn to_taffy_space(s: Size<AvailableSpace>) -> taffy::Size<taffy::AvailableSpace> {
    let conv = |a: AvailableSpace| match a {
        AvailableSpace::Definite(v) => taffy::AvailableSpace::Definite(v),
        AvailableSpace::MinContent => taffy::AvailableSpace::MinContent,
        AvailableSpace::MaxContent => taffy::AvailableSpace::MaxContent,
    };
    taffy::Size { width: conv(s.width), height: conv(s.height) }
}
