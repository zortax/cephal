//! Sizing shared by every container's absolutely positioned children.

use crate::compute::LayoutTreeExt;
use crate::compute::common::sizing_keyword::{SizingKeywordResolution, resolve_sizing_keyword};
use crate::geometry::{AbsoluteAxis, AvailableSpace, Line, MaybeMath, Rect, Size};
use crate::style::BoxSizing;
use crate::tree::{CacheAccess, LayoutTree, NodeId, SizingMode};

pub(crate) struct AbsoluteItemSizing {
    pub inset: Rect<Option<f32>>,
    pub margin: Rect<Option<f32>>,
    pub padding: Rect<f32>,
    pub border: Rect<f32>,
    pub min_size: Size<Option<f32>>,
    pub max_size: Size<Option<f32>>,
    pub final_size: Size<f32>,
}

/// Resolves insets, margins and the final border-box size of an absolutely positioned child
/// against its containing block `area_size`.
pub(crate) fn resolve_absolute_item_size<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    node: NodeId,
    area_size: Size<f32>,
) -> AbsoluteItemSizing {
    let style = tree.style(node);
    let calc = |id, basis| tree.resolve_calc(id, basis);
    let area_width = area_size.width;
    let area_height = area_size.height;
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
    let resolve = |s: Size<crate::style::Length>| Size {
        width: s.width.resolve(area.width, &calc),
        height: s.height.resolve(area.height, &calc),
    };
    let style_size = resolve(size_style.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment);
    let min_size = resolve(style.min_size.map(|v| v.raw()))
        .maybe_apply_aspect_ratio(aspect_ratio)
        .maybe_add(box_sizing_adjustment)
        .or(padding_border_sum.map(Some))
        .maybe_max(padding_border_sum.map(Some));
    let max_size = resolve(style.max_size.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment);
    let mut known_dimensions = style_size.maybe_clamp(min_size, max_size);

    // Sizing keywords: `stretch` fills the inset-reduced area, intrinsic keywords measure.
    if size_style.width.is_sizing_keyword() || size_style.height.is_sizing_keyword() {
        let stretch = Size {
            width: (area_width - left.unwrap_or(0.0) - right.unwrap_or(0.0) - margin.left.unwrap_or(0.0) - margin.right.unwrap_or(0.0))
                .max(0.0),
            height: (area_height - top.unwrap_or(0.0) - bottom.unwrap_or(0.0) - margin.top.unwrap_or(0.0) - margin.bottom.unwrap_or(0.0))
                .max(0.0),
        };
        let keyword_width = if known_dimensions.width.is_none() {
            resolve_sizing_keyword(size_style.width.raw(), Some(stretch.width), Some(area_width))
        } else {
            None
        };
        let keyword_height = if known_dimensions.height.is_none() {
            resolve_sizing_keyword(size_style.height.raw(), Some(stretch.height), Some(area_height))
        } else {
            None
        };
        use SizingKeywordResolution::*;
        match (keyword_width, keyword_height) {
            (Some(Measure(aw)), Some(Measure(ah))) => {
                let measured = tree.measure_child_size_both(
                    node,
                    Size::NONE,
                    area,
                    Size { width: aw, height: ah },
                    SizingMode::ContentSize,
                    Line::FALSE,
                );
                known_dimensions = measured.map(Some);
            }
            (kw, kh) => {
                if let Some(r) = kw {
                    known_dimensions.width = Some(match r {
                        Exact(w) => w,
                        Measure(aw) => tree.measure_child_size(
                            node,
                            known_dimensions,
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
                            known_dimensions,
                            area,
                            Size {
                                width: known_dimensions.width.map_or(AvailableSpace::Definite(stretch.width), AvailableSpace::Definite),
                                height: ah,
                            },
                            SizingMode::ContentSize,
                            AbsoluteAxis::Vertical,
                            Line::FALSE,
                        ),
                    });
                }
            }
        }
        known_dimensions = known_dimensions.maybe_apply_aspect_ratio(aspect_ratio).maybe_clamp(min_size, max_size);
    }

    if let (None, Some(l), Some(r)) = (known_dimensions.width, left, right) {
        let w = area_width.maybe_sub(margin.left).maybe_sub(margin.right) - l - r;
        known_dimensions.width = Some(w.max(0.0));
        known_dimensions = known_dimensions.maybe_apply_aspect_ratio(aspect_ratio).maybe_clamp(min_size, max_size);
    }
    if let (None, Some(t), Some(b)) = (known_dimensions.height, top, bottom) {
        let h = area_height.maybe_sub(margin.top).maybe_sub(margin.bottom) - t - b;
        known_dimensions.height = Some(h.max(0.0));
        known_dimensions = known_dimensions.maybe_apply_aspect_ratio(aspect_ratio).maybe_clamp(min_size, max_size);
    }

    let final_size = match (known_dimensions.width, known_dimensions.height) {
        (Some(width), Some(height)) => Size { width, height },
        _ => {
            let measured = tree.measure_child_size_both(
                node,
                known_dimensions,
                area,
                Size {
                    width: AvailableSpace::Definite(area_width.maybe_clamp(min_size.width, max_size.width)),
                    height: AvailableSpace::Definite(area_height.maybe_clamp(min_size.height, max_size.height)),
                },
                SizingMode::ContentSize,
                Line::FALSE,
            );
            known_dimensions.unwrap_or(measured)
        }
    }
    .maybe_clamp(min_size, max_size);

    AbsoluteItemSizing { inset: Rect { left, right, top, bottom }, margin, padding, border, min_size, max_size, final_size }
}
