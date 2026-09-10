//! Safe/unsafe alignment resolution, funnelled through two functions.

use crate::style::{AlignContent, AlignContentKeyword, AlignItems, AlignItemsKeyword};

/// Safe alignment falls back to `start` when the item overflows.
#[inline]
pub fn resolve_self_alignment_safety(alignment: AlignItems, overflows: bool) -> AlignItemsKeyword {
    if alignment.is_safe() && overflows { AlignItemsKeyword::Start } else { alignment.keyword }
}

/// Distribution keywords degrade when there is nothing to distribute, and safe alignment
/// falls back to `start` on overflow.
#[inline]
pub fn apply_alignment_fallback(free_space: f32, num_items: usize, alignment: AlignContent) -> AlignContentKeyword {
    let mut keyword = alignment.keyword;
    let mut safe = alignment.is_safe();
    if free_space <= 0.0 || num_items <= 1 {
        keyword = match keyword {
            AlignContentKeyword::SpaceBetween => {
                safe = true;
                AlignContentKeyword::FlexStart
            }
            AlignContentKeyword::SpaceAround | AlignContentKeyword::SpaceEvenly => {
                safe = true;
                AlignContentKeyword::Center
            }
            other => other,
        };
    }
    if safe && free_space < 0.0 { AlignContentKeyword::Start } else { keyword }
}

/// The offset of the first item, or the gap added before a later one, for a resolved keyword.
pub fn compute_alignment_offset(
    free_space: f32,
    num_items: usize,
    gap: f32,
    keyword: AlignContentKeyword,
    is_first: bool,
    layout_is_reversed: bool,
) -> f32 {
    if is_first {
        match keyword {
            AlignContentKeyword::Start => 0.0,
            AlignContentKeyword::FlexStart => {
                if layout_is_reversed { free_space } else { 0.0 }
            }
            AlignContentKeyword::End => free_space,
            AlignContentKeyword::FlexEnd => {
                if layout_is_reversed { 0.0 } else { free_space }
            }
            AlignContentKeyword::Center => free_space / 2.0,
            AlignContentKeyword::Stretch => 0.0,
            AlignContentKeyword::SpaceBetween => 0.0,
            AlignContentKeyword::SpaceEvenly => free_space / (num_items + 1) as f32,
            AlignContentKeyword::SpaceAround => (free_space / num_items as f32) / 2.0,
        }
    } else {
        gap + match keyword {
            AlignContentKeyword::SpaceBetween => free_space / (num_items - 1) as f32,
            AlignContentKeyword::SpaceEvenly => free_space / (num_items + 1) as f32,
            AlignContentKeyword::SpaceAround => free_space / num_items as f32,
            _ => 0.0,
        }
    }
}
