//! Style properties consumed by layout.

pub mod alignment;
pub mod calc;
pub mod dimension;
pub mod display;
pub mod flex;
pub mod grid;
pub mod length;
pub mod parse;

pub use alignment::{
    AlignContent, AlignContentKeyword, AlignItems, AlignItemsKeyword, AlignSelf, AlignmentSafety, JustifyContent,
    JustifyItems, JustifySelf,
};
pub use calc::{CalcId, CalcOp, CalcTable};
pub use dimension::{Dimension, LengthPercentage, LengthPercentageAuto};
pub use display::{BoxSizing, Clear, Contain, Direction, Display, Float, Overflow, Position, TextAlign};
pub use flex::{FlexDirection, FlexWrap};
pub use grid::{
    GridAutoFlow, GridContainerStyle, GridLine, GridPlacement, GridTemplateArea, GridTemplateAreas,
    GridTemplateComponent, GridTemplateRepetition, MaxTrackSizingFunction, MinTrackSizingFunction, OriginZeroLine,
    RepetitionCount, TrackSizingFunction,
};
pub use length::{Length, LengthKind};

use core::hash::{Hash, Hasher};

use crate::geometry::{Line, Point, Rect, Size};

/// An interned grid line/area name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ident(pub u32);

/// Every property layout reads. Plain data: `Eq + Hash`, interned by the tree.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Style {
    pub display: Display,
    pub box_sizing: BoxSizing,
    pub direction: Direction,
    pub position: Position,
    pub overflow: Point<Overflow>,
    pub contain: Contain,
    pub float: Float,
    pub clear: Clear,
    pub text_align: TextAlign,
    pub item_is_table: bool,
    pub item_is_replaced: bool,
    pub scrollbar_width: f32,
    pub aspect_ratio: Option<f32>,

    pub size: Size<Dimension>,
    pub min_size: Size<LengthPercentageAuto>,
    pub max_size: Size<LengthPercentageAuto>,
    pub inset: Rect<LengthPercentageAuto>,
    pub margin: Rect<LengthPercentageAuto>,
    pub padding: Rect<LengthPercentage>,
    pub border: Rect<LengthPercentage>,
    pub gap: Size<LengthPercentage>,

    pub align_items: Option<AlignItems>,
    pub align_self: Option<AlignSelf>,
    pub justify_items: Option<JustifyItems>,
    pub justify_self: Option<JustifySelf>,
    pub align_content: Option<AlignContent>,
    pub justify_content: Option<JustifyContent>,

    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub flex_line_count: u16,
    pub flex_basis: Dimension,
    pub flex_grow: f32,
    pub flex_shrink: f32,

    pub grid_row: Line<GridPlacement>,
    pub grid_column: Line<GridPlacement>,
    pub grid: Option<std::sync::Arc<GridContainerStyle>>,
}

impl Style {
    pub const DEFAULT: Self = Self {
        display: Display::Block,
        box_sizing: BoxSizing::BorderBox,
        direction: Direction::Ltr,
        position: Position::Relative,
        overflow: Point { x: Overflow::Visible, y: Overflow::Visible },
        contain: Contain::NONE,
        float: Float::None,
        clear: Clear::None,
        text_align: TextAlign::Auto,
        item_is_table: false,
        item_is_replaced: false,
        scrollbar_width: 0.0,
        aspect_ratio: None,
        size: Size { width: Dimension::AUTO, height: Dimension::AUTO },
        min_size: Size { width: LengthPercentageAuto::AUTO, height: LengthPercentageAuto::AUTO },
        max_size: Size { width: LengthPercentageAuto::AUTO, height: LengthPercentageAuto::AUTO },
        inset: Rect::splat(LengthPercentageAuto::AUTO),
        margin: Rect::splat(LengthPercentageAuto::ZERO),
        padding: Rect::splat(LengthPercentage::ZERO),
        border: Rect::splat(LengthPercentage::ZERO),
        gap: Size { width: LengthPercentage::ZERO, height: LengthPercentage::ZERO },
        align_items: None,
        align_self: None,
        justify_items: None,
        justify_self: None,
        align_content: None,
        justify_content: None,
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::NoWrap,
        flex_line_count: 1,
        flex_basis: Dimension::AUTO,
        flex_grow: 0.0,
        flex_shrink: 1.0,
        grid_row: Line { start: GridPlacement::Auto, end: GridPlacement::Auto },
        grid_column: Line { start: GridPlacement::Auto, end: GridPlacement::Auto },
        grid: None,
    };

    /// Grid container properties, defaults when unset.
    #[inline]
    pub fn grid(&self) -> &GridContainerStyle {
        static EMPTY: GridContainerStyle = GridContainerStyle {
            template_rows: Vec::new(),
            template_columns: Vec::new(),
            template_row_names: Vec::new(),
            template_column_names: Vec::new(),
            auto_rows: Vec::new(),
            auto_columns: Vec::new(),
            auto_flow: GridAutoFlow::Row,
            template_areas: None,
        };
        self.grid.as_deref().unwrap_or(&EMPTY)
    }

    #[inline]
    pub fn grid_mut(&mut self) -> &mut GridContainerStyle {
        std::sync::Arc::make_mut(self.grid.get_or_insert_with(Default::default))
    }

    /// The grid payload as a shared handle; cloning it costs a reference count.
    pub fn grid_shared(&self) -> std::sync::Arc<GridContainerStyle> {
        static EMPTY: std::sync::OnceLock<std::sync::Arc<GridContainerStyle>> = std::sync::OnceLock::new();
        match &self.grid {
            Some(g) => g.clone(),
            None => EMPTY.get_or_init(|| std::sync::Arc::new(GridContainerStyle::default())).clone(),
        }
    }

    /// Per axis, whether any of this node's own lengths resolves against the parent's size on
    /// that axis. Where it does not, the parent's size on that axis cannot affect the answer.
    pub fn parent_size_dependency(&self) -> Size<bool> {
        let w = [
            self.size.width.raw(),
            self.min_size.width.raw(),
            self.max_size.width.raw(),
            self.inset.left.raw(),
            self.inset.right.raw(),
            self.margin.left.raw(),
            self.margin.right.raw(),
            self.margin.top.raw(),
            self.margin.bottom.raw(),
            self.padding.left.raw(),
            self.padding.right.raw(),
            self.padding.top.raw(),
            self.padding.bottom.raw(),
            self.border.left.raw(),
            self.border.right.raw(),
            self.border.top.raw(),
            self.border.bottom.raw(),
        ];
        let h = [self.size.height.raw(), self.min_size.height.raw(), self.max_size.height.raw(), self.inset.top.raw(), self.inset.bottom.raw()];
        Size { width: w.iter().any(|l| l.uses_percentage()), height: h.iter().any(|l| l.uses_percentage()) }
    }

    /// Whether `display: none`.
    #[inline]
    pub fn generates_no_box(&self) -> bool {
        self.display == Display::None
    }

    /// Whether the box takes part in block flow as a block-level box.
    #[inline]
    pub fn is_block(&self) -> bool {
        self.display == Display::Block
    }

    /// Whether the box is a scroll container on either axis.
    #[inline]
    pub fn is_scroll_container(&self) -> bool {
        self.overflow.x.is_scroll_container() || self.overflow.y.is_scroll_container()
    }
}

impl Default for Style {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Eq for Style {}

impl Hash for Style {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.display.hash(h);
        self.box_sizing.hash(h);
        self.direction.hash(h);
        self.position.hash(h);
        self.overflow.hash(h);
        self.contain.hash(h);
        self.float.hash(h);
        self.clear.hash(h);
        self.text_align.hash(h);
        self.item_is_table.hash(h);
        self.item_is_replaced.hash(h);
        self.scrollbar_width.to_bits().hash(h);
        self.aspect_ratio.map(f32::to_bits).hash(h);
        self.size.hash(h);
        self.min_size.hash(h);
        self.max_size.hash(h);
        self.inset.hash(h);
        self.margin.hash(h);
        self.padding.hash(h);
        self.border.hash(h);
        self.gap.hash(h);
        self.align_items.hash(h);
        self.align_self.hash(h);
        self.justify_items.hash(h);
        self.justify_self.hash(h);
        self.align_content.hash(h);
        self.justify_content.hash(h);
        self.flex_direction.hash(h);
        self.flex_wrap.hash(h);
        self.flex_line_count.hash(h);
        self.flex_basis.hash(h);
        self.flex_grow.to_bits().hash(h);
        self.flex_shrink.to_bits().hash(h);
        self.grid_row.hash(h);
        self.grid_column.hash(h);
        self.grid.hash(h);
    }
}
