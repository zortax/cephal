//! §9.5–9.6 main-axis justification, cross-axis alignment and line alignment.

use super::axis::{FlexAxisRect, FlexAxisSize, sum_axis_gaps};
use super::{AlgoConstants, FlexItem, FlexLine};
use crate::compute::common::alignment::{apply_alignment_fallback, compute_alignment_offset};
use crate::style::{AlignItemsKeyword, Direction, JustifyContent};

pub(super) fn distribute_remaining_free_space(lines: &[FlexLine], items: &mut [FlexItem], c: &AlgoConstants) {
    let dir = c.dir;
    for line in lines {
        let line_items = line.items_mut(items);
        let total_main_axis_gap = sum_axis_gaps(c.gap.main(dir), line_items.len());
        let used_space: f32 = total_main_axis_gap + line_items.iter().map(|ch| ch.outer_target_size.main(dir)).sum::<f32>();
        let mut free_space = c.inner_container_size.main(dir) - used_space;
        let num_auto_margins: usize = line_items
            .iter()
            .map(|ch| ch.margin_is_auto.main_start(dir) as usize + ch.margin_is_auto.main_end(dir) as usize)
            .sum();
        if free_space > 0.0 && num_auto_margins > 0 {
            let margin = free_space / num_auto_margins as f32;
            for ch in line_items.iter_mut() {
                if ch.margin_is_auto.main_start(dir) {
                    if c.is_row { ch.margin.left = margin } else { ch.margin.top = margin }
                }
                if ch.margin_is_auto.main_end(dir) {
                    if c.is_row { ch.margin.right = margin } else { ch.margin.bottom = margin }
                }
            }
            free_space = 0.0;
        }
        let num_items = line_items.len();
        let layout_reverse = dir.is_reverse();
        let gap = c.gap.main(dir);
        let raw = c.justify_content.unwrap_or(JustifyContent::FLEX_START);
        let keyword = apply_alignment_fallback(free_space, num_items, raw);
        let justify = |(i, ch): (usize, &mut FlexItem)| {
            ch.offset_main = compute_alignment_offset(free_space, num_items, gap, keyword, i == 0, layout_reverse);
        };
        if layout_reverse {
            line_items.iter_mut().rev().enumerate().for_each(justify);
        } else {
            line_items.iter_mut().enumerate().for_each(justify);
        }
    }
}

pub(super) fn resolve_cross_axis_auto_margins(lines: &[FlexLine], items: &mut [FlexItem], c: &AlgoConstants) {
    let dir = c.dir;
    for line in lines {
        let line_cross_size = line.cross_size;
        let line_items = line.items_mut(items);
        let max_baseline = line_items.iter().map(|ch| ch.baseline).fold(0.0f32, f32::max);
        let max_baseline_to_bottom_distance = line_items
            .iter()
            .filter(|ch| ch.participates_in_baseline_alignment(dir))
            .map(|ch| ch.outer_target_size.cross(dir) - ch.baseline)
            .fold(0.0f32, f32::max);
        for ch in line_items.iter_mut() {
            let free_space = line_cross_size - ch.outer_target_size.cross(dir);
            if ch.margin_is_auto.cross_start(dir) && ch.margin_is_auto.cross_end(dir) {
                if c.is_row {
                    ch.margin.top = free_space / 2.0;
                    ch.margin.bottom = free_space / 2.0;
                } else {
                    ch.margin.left = free_space / 2.0;
                    ch.margin.right = free_space / 2.0;
                }
            } else if ch.margin_is_auto.cross_start(dir) {
                if c.is_row { ch.margin.top = free_space } else { ch.margin.left = free_space }
            } else if ch.margin_is_auto.cross_end(dir) {
                if c.is_row { ch.margin.bottom = free_space } else { ch.margin.right = free_space }
            } else {
                ch.offset_cross = align_flex_items_along_cross_axis(ch, free_space, max_baseline, max_baseline_to_bottom_distance, c);
            }
        }
    }
}

fn align_flex_items_along_cross_axis(
    ch: &FlexItem,
    free_space: f32,
    max_baseline: f32,
    max_baseline_to_bottom_distance: f32,
    c: &AlgoConstants,
) -> f32 {
    let dir = c.dir;
    let cross_axis_should_reverse = c.is_column && c.layout_direction == Direction::Rtl;
    let keyword = if ch.align_self.is_safe() && free_space < 0.0 { AlignItemsKeyword::Start } else { ch.align_self.keyword };
    match keyword {
        AlignItemsKeyword::Start => {
            if cross_axis_should_reverse { free_space } else { 0.0 }
        }
        AlignItemsKeyword::FlexStart | AlignItemsKeyword::Stretch => {
            if c.is_wrap_reverse ^ cross_axis_should_reverse { free_space } else { 0.0 }
        }
        AlignItemsKeyword::End => {
            if cross_axis_should_reverse { 0.0 } else { free_space }
        }
        AlignItemsKeyword::FlexEnd => {
            if c.is_wrap_reverse ^ cross_axis_should_reverse { 0.0 } else { free_space }
        }
        AlignItemsKeyword::Center => free_space / 2.0,
        AlignItemsKeyword::Baseline => {
            if c.is_row {
                if c.is_wrap_reverse {
                    let line_cross_size = free_space + ch.outer_target_size.cross(dir);
                    line_cross_size - max_baseline_to_bottom_distance - ch.baseline
                } else {
                    max_baseline - ch.baseline
                }
            } else {
                let baseline_column_should_reverse = cross_axis_should_reverse && !c.is_wrap;
                if c.is_wrap_reverse ^ baseline_column_should_reverse { free_space } else { 0.0 }
            }
        }
        AlignItemsKeyword::SelfStart | AlignItemsKeyword::SelfEnd => unreachable!("resolved at item creation"),
    }
}

pub(super) fn align_flex_lines_per_align_content(lines: &mut [FlexLine], c: &AlgoConstants, total_cross_size: f32) {
    let dir = c.dir;
    let num_lines = lines.len();
    let gap = c.gap.cross(dir);
    let total_cross_axis_gap = sum_axis_gaps(gap, num_lines);
    let free_space = c.inner_container_size.cross(dir) - total_cross_size - total_cross_axis_gap;
    let keyword = apply_alignment_fallback(free_space, num_lines, c.align_content);
    let align = |(i, line): (usize, &mut FlexLine)| {
        line.offset_cross = compute_alignment_offset(free_space, num_lines, gap, keyword, i == 0, c.is_wrap_reverse);
    };
    if c.is_wrap_reverse {
        lines.iter_mut().rev().enumerate().for_each(align);
    } else {
        lines.iter_mut().enumerate().for_each(align);
    }
}
