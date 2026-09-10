//! Stored results.

use crate::geometry::{Line, Point, Rect, Size};

/// The final geometry of one box, relative to its parent's border box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Paint order among siblings.
    pub order: u32,
    pub location: Point<f32>,
    pub size: Size<f32>,
    /// Scrollable overflow relative to the scroll origin; always contains the origin.
    pub scrollable_overflow_rect: Rect<f32>,
    pub scrollbar_size: Size<f32>,
    pub border: Rect<f32>,
    pub padding: Rect<f32>,
    pub margin: Rect<f32>,
}

impl Layout {
    pub const ZERO: Self = Self {
        order: 0,
        location: Point::ZERO,
        size: Size::ZERO,
        scrollable_overflow_rect: Rect::ZERO,
        scrollbar_size: Size::ZERO,
        border: Rect::ZERO,
        padding: Rect::ZERO,
        margin: Rect::ZERO,
    };

    #[inline]
    pub const fn with_order(order: u32) -> Self {
        Self { order, ..Self::ZERO }
    }

    #[inline]
    pub fn content_box_width(&self) -> f32 {
        self.size.width - self.padding.left - self.padding.right - self.border.left - self.border.right - self.scrollbar_size.width
    }
    #[inline]
    pub fn content_box_height(&self) -> f32 {
        self.size.height - self.padding.top - self.padding.bottom - self.border.top - self.border.bottom - self.scrollbar_size.height
    }
    #[inline]
    pub fn content_box_size(&self) -> Size<f32> {
        Size { width: self.content_box_width(), height: self.content_box_height() }
    }
    #[inline]
    pub fn content_box_x(&self) -> f32 {
        self.location.x + self.border.left + self.padding.left
    }
    #[inline]
    pub fn content_box_y(&self) -> f32 {
        self.location.y + self.border.top + self.padding.top
    }
    /// Maximum horizontal scroll offset: reachable content extent less the padding box, floored at zero.
    #[inline]
    pub fn scroll_width(&self) -> f32 {
        (self.scrollable_overflow_rect.right + self.scrollbar_size.width.min(self.size.width) - self.size.width
            + self.border.left
            + self.border.right)
            .max(0.0)
    }
    #[inline]
    pub fn scroll_height(&self) -> f32 {
        (self.scrollable_overflow_rect.bottom + self.scrollbar_size.height.min(self.size.height) - self.size.height
            + self.border.top
            + self.border.bottom)
            .max(0.0)
    }
}

impl Default for Layout {
    fn default() -> Self {
        Self::ZERO
    }
}

/// Resolved grid tracks and item placements, for consumers exposing used values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DetailedGridInfo {
    pub rows: DetailedGridTracks,
    pub columns: DetailedGridTracks,
    pub items: Vec<DetailedGridItem>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DetailedGridTracks {
    pub negative_implicit_tracks: u16,
    pub explicit_tracks: u16,
    pub positive_implicit_tracks: u16,
    /// Start/end offsets of every track, gutters excluded.
    pub positions: Vec<Line<f32>>,
    /// Names of each explicit line (line 1 first); empty when none are named.
    pub line_names: Vec<Vec<crate::style::Ident>>,
    /// Offsets of the interleaved gutter/track list (`2 * tracks + 1` entries).
    pub offsets: Vec<f32>,
    /// How many times an `auto-fill`/`auto-fit` repetition was expanded.
    pub auto_repetitions: u16,
}

/// 1-based grid lines of an in-flow item.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DetailedGridItem {
    pub row_start: u16,
    pub row_end: u16,
    pub column_start: u16,
    pub column_end: u16,
}

impl DetailedGridTracks {
    /// Names on the line before track `line_index` (0-based over all tracks).
    pub fn names_for_line(&self, line_index: usize) -> &[crate::style::Ident] {
        line_index
            .checked_sub(self.negative_implicit_tracks as usize)
            .and_then(|i| self.line_names.get(i))
            .map_or(&[], Vec::as_slice)
    }

    /// The resolved track list as CSS serialises it, e.g. `[a] 10px [b] 20px`.
    pub fn track_list_string(&self, name_of: impl Fn(crate::style::Ident) -> String) -> String {
        use core::fmt::Write as _;
        if self.positions.is_empty() {
            return "none".into();
        }
        let mut out = String::new();
        let write_names = |out: &mut String, names: &[crate::style::Ident]| {
            out.push('[');
            for (i, n) in names.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&name_of(*n));
            }
            out.push(']');
        };
        let mut needs_space = false;
        for (i, p) in self.positions.iter().enumerate() {
            let names = self.names_for_line(i);
            if !names.is_empty() {
                if needs_space {
                    out.push(' ');
                }
                write_names(&mut out, names);
                needs_space = true;
            }
            if needs_space {
                out.push(' ');
            }
            let _ = write!(out, "{}px", p.end - p.start);
            needs_space = true;
        }
        let trailing = self.names_for_line(self.positions.len());
        if !trailing.is_empty() {
            out.push(' ');
            write_names(&mut out, trailing);
        }
        out
    }
}
