//! §9.2 step 4: the container's main size when it is not known.

use super::axis::{FlexAxisSize, FlexAxisSum, abs_main, sum_axis_gaps};
use super::{AlgoConstants, FlexItem, FlexLine};
use crate::compute::LayoutTreeExt;
use crate::geometry::{AvailableSpace, Line, MaybeMath, Size};
use crate::style::AlignSelf;
use crate::tree::{CacheAccess, LayoutTree, SizingMode};

#[inline]
fn item_main_length(child: &FlexItem, dir: crate::style::FlexDirection) -> f32 {
    let padding_border_sum = (child.padding + child.border).main_axis_sum(dir);
    (child.flex_basis.maybe_max(child.min_size.main(dir)) + child.margin.main_axis_sum(dir)).max(padding_border_sum)
}

pub(super) fn determine_container_main_size<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    available_space: Size<AvailableSpace>,
    lines: &mut [FlexLine],
    items: &mut [FlexItem],
    c: &mut AlgoConstants,
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
                        let clamping_basis = Some(item.flex_basis).maybe_max(style_preferred);
                        let flex_basis_min = clamping_basis.filter(|_| item.flex_shrink == 0.0);
                        let flex_basis_max = clamping_basis.filter(|_| item.flex_grow == 0.0);
                        let min_main_size = style_min
                            .maybe_max(flex_basis_min)
                            .or(flex_basis_min)
                            .unwrap_or(item.resolved_minimum_main_size)
                            .max(item.resolved_minimum_main_size);
                        let max_main_size = style_max.maybe_min(flex_basis_max).or(flex_basis_max).unwrap_or(f32::INFINITY);
                        let content_contribution = match (min_main_size, style_preferred, max_main_size) {
                            (min, Some(pref), max) if max <= min || max <= pref => {
                                pref.min(max).max(min) + item.margin.main_axis_sum(dir)
                            }
                            (min, _, max) if max <= min => min + item.margin.main_axis_sum(dir),
                            _ if item.is_scroll_container() => item.flex_basis + item.margin.main_axis_sum(dir),
                            (_, Some(pref), _) => {
                                let item_pb_main = item.padding.main_axis_sum(dir) + item.border.main_axis_sum(dir);
                                let inner_main_size = pref.max(item_pb_main);
                                if c.is_row {
                                    (inner_main_size + item.margin.main_axis_sum(dir)).maybe_clamp(style_min, style_max)
                                } else {
                                    (inner_main_size.max(item.flex_basis) + item.margin.main_axis_sum(dir))
                                        .maybe_clamp(style_min, style_max)
                                }
                            }
                            _ => {
                                let cross_axis_parent_size = c.node_inner_size.cross(dir);
                                let cross_axis_margin_sum = c.margin.cross_axis_sum(dir);
                                let child_min_cross = item.min_size.cross(dir).maybe_add(cross_axis_margin_sum);
                                let child_max_cross = item.max_size.cross(dir).maybe_add(cross_axis_margin_sum);
                                let cross_axis_available_space = available_space
                                    .cross(dir)
                                    .map_definite(|val| c.divided_cross_space(cross_axis_parent_size.unwrap_or(val)))
                                    .maybe_clamp(child_min_cross, child_max_cross);
                                let child_available_space = available_space.with_cross(dir, cross_axis_available_space);
                                let child_known_dimensions = {
                                    let mut ckd = item.size.with_main(dir, None);
                                    if item.align_self == AlignSelf::STRETCH && ckd.cross(dir).is_none() {
                                        ckd.set_cross(
                                            dir,
                                            cross_axis_available_space
                                                .into_option()
                                                .maybe_sub(item.margin.cross_axis_sum(dir))
                                                .maybe_max(0.0),
                                        );
                                    }
                                    ckd
                                };
                                let measured_main_size = tree.measure_child_size(
                                    item.node,
                                    child_known_dimensions,
                                    c.node_inner_size,
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
                                if c.is_row {
                                    (inner_main_size + item.margin.main_axis_sum(dir)).maybe_clamp(style_min, style_max)
                                } else {
                                    (inner_main_size.max(item.flex_basis) + item.margin.main_axis_sum(dir))
                                        .maybe_clamp(style_min, style_max)
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
