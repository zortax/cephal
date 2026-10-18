//! A style's lengths resolved once, for styles none of whose lengths resolve against a parent.

use super::leaf::LeafBox;
use crate::geometry::{Rect, Size};
use crate::style::Style;

/// The style's lengths as every algorithm resolves them, valid against any percentage basis.
///
/// Values are raw: box sizing and aspect ratio are not applied, since algorithms combine them in
/// different orders. Exists only when no size, inset, margin, padding or border length is a
/// percentage or `calc()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StylePlan {
    pub size: Size<Option<f32>>,
    pub min_size: Size<Option<f32>>,
    pub max_size: Size<Option<f32>>,
    pub inset: Rect<Option<f32>>,
    pub margin: Rect<f32>,
    pub padding: Rect<f32>,
    pub border: Rect<f32>,
    /// `flex-basis` when it is a length; `None` for keywords, `auto` and relative values.
    pub flex_basis: Option<f32>,
    /// `flex-basis` is a percentage or `calc()`; `flex_basis` does not hold it.
    pub flex_basis_is_relative: bool,
    /// The leaf algorithm's box.
    pub leaf: LeafBox,
}

impl StylePlan {
    /// Resolves `style`; `None` when any of its lengths needs a parent size.
    pub fn from_style(style: &Style) -> Option<Self> {
        let leaf = LeafBox::from_style(style)?;
        let calc = |_, _| 0.0;
        let raw = |s: Size<crate::style::Length>| Size { width: s.width.resolve(None, &calc), height: s.height.resolve(None, &calc) };
        let basis = style.flex_basis.raw();
        Some(Self {
            size: raw(style.size.map(|v| v.raw())),
            min_size: raw(style.min_size.map(|v| v.raw())),
            max_size: raw(style.max_size.map(|v| v.raw())),
            inset: style.inset.map(|i| i.resolve(None, &calc)),
            margin: style.margin.map(|m| m.resolve_or_zero(None, &calc)),
            padding: style.padding.map(|p| p.resolve_or_zero(None, &calc)),
            border: style.border.map(|b| b.resolve_or_zero(None, &calc)),
            flex_basis: if basis.uses_percentage() { None } else { basis.resolve(None, &calc) },
            flex_basis_is_relative: basis.uses_percentage(),
            leaf,
        })
    }
}
