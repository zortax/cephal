//! Content-based container width.

use super::floats::FloatIntrinsicWidthCalculator;
use super::items::BlockItem;
use crate::compute::LayoutTreeExt;
use crate::compute::common::sizing_keyword::{SizingKeywordResolution, resolve_sizing_keyword};
use crate::geometry::{AbsoluteAxis, AvailableSpace, Line, MaybeMath, Size};
use crate::style::Position;
use crate::tree::{CacheAccess, LayoutTree, SizingMode};

/// The widest in-flow child, margins included; floats stack horizontally until cleared.
pub(super) fn determine_content_based_container_width<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    items: &[BlockItem],
    available_width: AvailableSpace,
) -> f32 {
    let available_space = Size { width: available_width, height: AvailableSpace::MinContent };
    let mut max_child_width = 0.0f32;
    let mut floats = FloatIntrinsicWidthCalculator::new(available_width);
    for item in items.iter().filter(|item| item.position != Position::Absolute) {
        let known_dimensions = item.size.maybe_clamp(item.min_size, item.max_size);
        let calc = |id, basis| tree.resolve_calc(id, basis);
        let item_x_margin_sum =
            item.margin.map(|m| m.resolve_or_zero(available_space.width.into_option(), &calc)).horizontal_axis_sum();
        let width = match known_dimensions.width {
            Some(w) => w,
            None => {
                let item_available_width = match resolve_sizing_keyword(item.size_style.width.raw(), None, None) {
                    Some(SizingKeywordResolution::Measure(a)) => a,
                    Some(SizingKeywordResolution::Exact(w)) => AvailableSpace::Definite(w),
                    None => available_space.width.maybe_sub(item_x_margin_sum),
                };
                tree.measure_child_size(
                    item.node,
                    known_dimensions,
                    Size::NONE,
                    Size { width: item_available_width, height: available_space.height },
                    SizingMode::InherentSize,
                    AbsoluteAxis::Horizontal,
                    Line::TRUE,
                )
            }
        };
        let width = width.max(item.padding_border_sum.width) + item_x_margin_sum;
        if let Some(direction) = item.float.direction() {
            floats.add_float(width, direction, item.clear);
            continue;
        }
        max_child_width = max_child_width.max(width);
    }
    max_child_width.max(floats.result())
}
