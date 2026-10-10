//! State shared along one block formatting context.

use super::floats::{BfcSlot, FloatContext, FloatDirection};
use crate::geometry::{Point, Size};
use crate::style::{Clear, Direction};

/// Owned by the box that establishes the BFC.
#[derive(Debug, Default)]
pub struct BlockFormattingContext {
    floats: FloatContext,
}

impl BlockFormattingContext {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn root_block_context(&mut self) -> BlockContext<'_> {
        BlockContext {
            bfc: self,
            y_offset: 0.0,
            insets: [0.0, 0.0],
            content_box_insets: [0.0, 0.0],
            float_content_contribution: f32::NEG_INFINITY,
            is_root: true,
            adjoining_floats: [false, false],
            top_adjoining_floats: None,
        }
    }
}

/// A block's view of its BFC, in the block's own coordinates.
#[derive(Debug)]
pub struct BlockContext<'bfc> {
    bfc: &'bfc mut BlockFormattingContext,
    y_offset: f32,
    insets: [f32; 2],
    content_box_insets: [f32; 2],
    float_content_contribution: f32,
    is_root: bool,
    /// Floats placed while the current margin-collapse strut was unresolved.
    adjoining_floats: [bool; 2],
    top_adjoining_floats: Option<[bool; 2]>,
}

impl BlockContext<'_> {
    pub fn sub_context(&mut self, additional_y_offset: f32, insets: [f32; 2]) -> BlockContext<'_> {
        let insets = [self.insets[0] + insets[0], self.insets[1] + insets[1]];
        BlockContext {
            bfc: self.bfc,
            y_offset: self.y_offset + additional_y_offset,
            insets,
            content_box_insets: insets,
            float_content_contribution: f32::NEG_INFINITY,
            is_root: false,
            adjoining_floats: self.adjoining_floats,
            top_adjoining_floats: None,
        }
    }
    #[inline]
    pub fn is_bfc_root(&self) -> bool {
        self.is_root
    }
    /// Cache key covering the float state a child's answer depends on; `0` without floats.
    #[inline]
    pub fn key(&self) -> u32 {
        if self.bfc.floats.has_floats() {
            // Any float state makes answers position dependent; never share them.
            self.bfc.floats.version().max(1).wrapping_mul(2_654_435_761) | 1
        } else {
            0
        }
    }
    #[inline]
    pub fn set_width(&mut self, available_width: f32) {
        self.bfc.floats.set_width(available_width);
    }
    #[inline]
    pub fn apply_content_box_inset(&mut self, content_box_x_insets: [f32; 2]) {
        self.content_box_insets[0] = self.insets[0] + content_box_x_insets[0];
        self.content_box_insets[1] = self.insets[1] + content_box_x_insets[1];
    }
    /// Whether any float was placed in the formatting context, including floats placed by
    /// ancestors and earlier siblings sharing it, not only this box's own.
    #[inline]
    pub fn has_floats(&self) -> bool {
        self.bfc.floats.has_floats()
    }
    #[inline]
    pub fn has_active_floats(&self, min_y: f32) -> bool {
        self.bfc.floats.has_active_floats(min_y + self.y_offset)
    }
    pub fn place_floated_box(
        &mut self,
        floated_box: Size<f32>,
        min_y: f32,
        direction: FloatDirection,
        clear: Clear,
        adjoins_unresolved_strut: bool,
    ) -> Point<f32> {
        if adjoins_unresolved_strut {
            self.adjoining_floats[direction as usize] = true;
        }
        let mut pos = self.bfc.floats.place_floated_box(floated_box, min_y + self.y_offset, self.content_box_insets, direction, clear);
        pos.y -= self.y_offset;
        pos.x -= self.insets[0];
        self.float_content_contribution = self.float_content_contribution.max(pos.y + floated_box.height);
        pos
    }
    pub fn find_bfc_slot(&self, min_y: f32, margins: [f32; 2], direction: Direction, clear: Clear, after: Option<usize>) -> BfcSlot {
        let mut slot = self.bfc.floats.find_bfc_slot(min_y + self.y_offset, self.content_box_insets, margins, direction, clear, after);
        slot.y -= self.y_offset;
        slot.x -= self.insets[0];
        slot
    }
    #[inline]
    pub fn cleared_threshold(&self, clear: Clear) -> Option<f32> {
        self.bfc.floats.cleared_threshold(clear).map(|t| t - self.y_offset)
    }
    #[inline]
    pub fn has_adjoining_float(&self, clear: Clear) -> bool {
        match clear {
            Clear::Left => self.adjoining_floats[0],
            Clear::Right => self.adjoining_floats[1],
            Clear::Both => self.adjoining_floats[0] || self.adjoining_floats[1],
            Clear::None => false,
        }
    }
    #[inline]
    pub fn merge_adjoining_floats(&mut self, flags: [bool; 2]) {
        self.adjoining_floats[0] |= flags[0];
        self.adjoining_floats[1] |= flags[1];
    }
    /// Resolves the current strut: later floats no longer adjoin it.
    #[inline]
    pub fn commit_strut(&mut self) {
        if self.top_adjoining_floats.is_none() {
            self.top_adjoining_floats = Some(self.adjoining_floats);
        }
        self.adjoining_floats = [false, false];
    }
    #[inline]
    pub fn top_adjoining_floats(&self) -> [bool; 2] {
        self.top_adjoining_floats.unwrap_or(self.adjoining_floats)
    }
    #[inline]
    pub fn add_child_floated_content_height_contribution(&mut self, v: f32) {
        self.float_content_contribution = self.float_content_contribution.max(v);
    }
    #[inline]
    pub fn floated_content_height_contribution(&self) -> f32 {
        self.float_content_contribution
    }
}
