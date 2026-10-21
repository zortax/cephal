//! §9.2 step 4: the container's main size when it is not known.

use super::axis::FlexDir;
use super::axis::{FlexAxisSize, FlexAxisSum, abs_main, sum_axis_gaps};
use super::{AlgoConstants, FlexItem, FlexLine};
use crate::compute::LayoutTreeExt;
use crate::compute::common::sizing_keyword::SizingKeywordResolution;
use crate::geometry::{AvailableSpace, Line, MaybeMath, Size};
use crate::tree::{CacheAccess, LayoutTree, SizingMode};

/// The item's hypothetical outer main size: its flex base size clamped by its min and max sizes.
#[inline]
fn item_main_length(child: &FlexItem, dir: impl FlexDir) -> f32 {
    child.hypothetical_outer_size.main(dir)
}

pub(super) fn determine_container_main_size<D: FlexDir, T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    available_space: Size<AvailableSpace>,
    lines: &mut [FlexLine],
    items: &mut [FlexItem],
    c: &mut AlgoConstants<D>,
) {
    let dir = c.dir;
    let main_content_box_inset = c.content_box_inset.main_axis_sum(dir);
    let outer_main_size: f32 = match c.node_outer_size.main(dir) {
        Some(v) => v,
        None => match available_space.main(dir) {
            AvailableSpace::Definite(main_axis_available_space) => {
                let gap = c.gap.main(dir);
                let longest_line_length = lines
                    .iter()
                    .map(|line| {
                        line.items(items).iter().map(|ch| item_main_length(ch, dir)).sum::<f32>() + sum_axis_gaps(gap, line.len())
                    })
                    .fold(f32::NEG_INFINITY, f32::max)
                    .max(0.0);
                let size = longest_line_length + main_content_box_inset;
                if c.is_balance {
                    let min_line_count = c.line_count.unwrap_or(1);
                    let item_count: usize = lines.iter().map(FlexLine::len).sum();
                    if item_count == 0 {
                        size
                    } else {
                        let item_lengths: Vec<f32> =
                            lines.iter().flat_map(|line| line.items(items).iter().map(|ch| item_main_length(ch, dir))).collect();
                        let counts = super::balance::balanced_line_item_counts(
                            item_lengths.iter().copied(),
                            f32::INFINITY,
                            gap,
                            min_line_count.max(1) as usize,
                        );
                        let mut widest = 0.0f32;
                        let mut index = 0;
                        for count in counts {
                            let line_length = item_lengths[index..index + count].iter().sum::<f32>() + sum_axis_gaps(gap, count);
                            widest = widest.max(line_length);
                            index += count;
                        }
                        let max_content_size = widest + main_content_box_inset;
                        size.max(max_content_size.min(main_axis_available_space))
                    }
                } else if lines.len() > 1 {
                    size.max(main_axis_available_space)
                } else {
                    size
                }
            }
            AvailableSpace::MinContent if c.is_wrap => {
                let gap = c.gap.main(dir);
                let longest_line_length = lines
                    .iter()
                    .map(|line| {
                        line.items(items).iter().map(|ch| item_main_length(ch, dir)).sum::<f32>() + sum_axis_gaps(gap, line.len())
                    })
                    .fold(f32::NEG_INFINITY, f32::max)
                    .max(0.0);
                longest_line_length + main_content_box_inset
            }
            AvailableSpace::MinContent | AvailableSpace::MaxContent => {
                let mut main_size = 0.0f32;
                for line in lines.iter_mut() {
                    for item in line.items_mut(items) {
                        let style_min = item.min_size.main(dir);
                        let style_preferred = item.size.main(dir);
                        let style_max = item.max_size.main(dir);
                        // A column item's `height` does not raise its contribution past its flex base size
                        // (§9.9.3); a row item's `width` does, as in Chrome and Firefox.
                        let clamping_basis =
                            if c.is_row { Some(item.flex_basis).maybe_max(style_preferred) } else { Some(item.flex_basis) };
                        let flex_basis_min = clamping_basis.filter(|_| item.flex_shrink == 0.0);
                        let flex_basis_max = clamping_basis.filter(|_| item.flex_grow == 0.0);
                        let min_main_size = style_min
                            .maybe_max(flex_basis_min)
                            .or(flex_basis_min)
                            .unwrap_or(item.resolved_minimum_main_size)
                            .max(item.resolved_minimum_main_size);
                        let max_main_size = style_max.maybe_min(flex_basis_max).or(flex_basis_max).unwrap_or(f32::INFINITY);
                        let content_contribution = match (min_main_size, style_preferred, max_main_size) {
                            (min, Some(pref), max) if c.is_row && (max <= min || max <= pref) => {
                                pref.min(max).max(min) + item.margin.main_axis_sum(dir)
                            }
                            (min, _, max) if max <= min => min + item.margin.main_axis_sum(dir),
                            (_, Some(pref), _) => {
                                let item_pb_main = item.padding.main_axis_sum(dir) + item.border.main_axis_sum(dir);
                                let inner_main_size = pref.max(item_pb_main);
                                if c.is_row {
                                    (inner_main_size + item.margin.main_axis_sum(dir)).maybe_clamp(style_min, style_max)
                                } else {
                                    // A column item contributes its hypothetical main size: its
                                    // flex base size clamped, whatever its `height` (as Chrome).
                                    (item.flex_basis.max(min_main_size).min(max_main_size) + item.margin.main_axis_sum(dir))
                                        .maybe_clamp(style_min, style_max)
                                }
                            }
                            _ => {
                                let cross_axis_parent_size = c.node_inner_size.cross(dir);
                                let child_min_cross = item.min_size.cross(dir);
                                let child_max_cross = item.max_size.cross(dir);
                                // The space for the item's border box.
                                let mut cross_axis_available_space = available_space
                                    .cross(dir)
                                    .map_definite(|val| {
                                        (c.divided_cross_space(cross_axis_parent_size.unwrap_or(val)) - item.margin.cross_axis_sum(dir)).max(0.0)
                                    })
                                    .maybe_clamp(child_min_cross, child_max_cross);
                                let child_known_dimensions = {
                                    let mut ckd = item.size.with_main(dir, None);
                                    // Measured at its clamped cross size, as for its flex base size.
                                    let min_cross = item.min_size.maybe_apply_aspect_ratio(item.aspect_ratio).cross(dir);
                                    let max_cross = item.max_size.maybe_apply_aspect_ratio(item.aspect_ratio).cross(dir);
                                    ckd.set_cross(dir, ckd.cross(dir).maybe_clamp(min_cross, max_cross));
                                    let keyword = match ckd.cross(dir) {
                                        Some(_) => None,
                                        None => super::base_size::cross_sizing_keyword(c, item),
                                    };
                                    match keyword {
                                        Some(SizingKeywordResolution::Exact(size)) => ckd.set_cross(dir, Some(size)),
                                        Some(SizingKeywordResolution::Measure(space)) => cross_axis_available_space = space,
                                        None => {
                                            if item.align_self.keyword == crate::style::AlignItemsKeyword::Stretch && ckd.cross(dir).is_none() {
                                                let stretched = match c.indefinite_stretch_cross {
                                                    Some(cross) => Some((cross - item.margin.cross_axis_sum(dir)).max(0.0)),
                                                    None => cross_axis_available_space.into_option(),
                                                };
                                                ckd.set_cross(dir, stretched);
                                            }
                                        }
                                    }
                                    ckd
                                };
                                // A content-sized main size keyword contributes that size.
                                let main_space = match crate::compute::common::sizing_keyword::resolve_sizing_keyword(
                                    item.size_style.main(dir).raw(),
                                    None,
                                    None,
                                ) {
                                    Some(SizingKeywordResolution::Measure(space)) => space,
                                    _ => available_space.main(dir),
                                };
                                let child_available_space = available_space.with_cross(dir, cross_axis_available_space).with_main(dir, main_space);
                                let measured_main_size = tree.measure_child_size(
                                    item.node,
                                    child_known_dimensions,
                                    c.percent_basis(),
                                    child_available_space,
                                    SizingMode::ContentSize,
                                    abs_main(dir),
                                    Line::FALSE,
                                );
                                let transferred_main_size = item
                                    .aspect_ratio
                                    .zip(child_known_dimensions.cross(dir))
                                    .map(|(ratio, cross)| if c.is_row { cross * ratio } else { cross / ratio });
                                let inner_main_size = measured_main_size.maybe_max(transferred_main_size);
                                // A row item contributes its content, a column item its hypothetical
                                // size; within its flex base size where it cannot grow or shrink.
                                if c.is_row {
                                    inner_main_size.min(max_main_size).max(min_main_size) + item.margin.main_axis_sum(dir)
                                } else {
                                    item.flex_basis.min(max_main_size).max(min_main_size) + item.margin.main_axis_sum(dir)
                                }
                            }
                        };
                        item.content_flex_fraction = {
                            let diff = content_contribution - item.flex_basis;
                            if diff > 0.0 {
                                diff / item.flex_grow.max(1.0)
                            } else if diff < 0.0 {
                                let scaled_shrink_factor = item.flex_shrink.max(1.0) * item.inner_flex_basis;
                                diff / scaled_shrink_factor
                            } else {
                                0.0
                            }
                        };
                    }
                    let item_main_size_sum: f32 = line
                        .items_mut(items)
                        .iter_mut()
                        .map(|item| {
                            let flex_fraction = item.content_flex_fraction;
                            let flex_contribution = if flex_fraction > 0.0 {
                                item.flex_grow.max(1.0) * flex_fraction
                            } else if flex_fraction < 0.0 {
                                let scaled_shrink_factor = item.flex_shrink.max(1.0) * item.inner_flex_basis;
                                if scaled_shrink_factor == 0.0 { 0.0 } else { scaled_shrink_factor * flex_fraction }
                            } else {
                                0.0
                            };
                            let size = item.flex_basis + flex_contribution;
                            item.outer_target_size.set_main(dir, size);
                            item.target_size.set_main(dir, size);
                            size
                        })
                        .sum();
                    let gap_sum = sum_axis_gaps(c.gap.main(dir), line.len());
                    main_size = main_size.max(item_main_size_sum + gap_sum);
                }
                main_size + main_content_box_inset
            }
        },
    };
    let outer_main_size = outer_main_size
        .maybe_clamp(c.min_size.main(dir), c.max_size.main(dir))
        .max(main_content_box_inset - c.scrollbar_gutter.main(dir));
    let inner_main_size = (outer_main_size - main_content_box_inset).max(0.0);
    c.container_size.set_main(dir, outer_main_size);
    c.inner_container_size.set_main(dir, inner_main_size);
    c.node_inner_size.set_main(dir, Some(inner_main_size));
}
