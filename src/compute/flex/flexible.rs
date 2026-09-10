//! §9.7 resolving flexible lengths.

use super::axis::{FlexAxisSize, FlexAxisSum, sum_axis_gaps};
use super::{AlgoConstants, FlexItem};
use crate::geometry::MaybeMath;

pub(super) fn resolve_flexible_lengths(items: &mut [FlexItem], c: &AlgoConstants) {
    let dir = c.dir;
    let total_main_axis_gap = sum_axis_gaps(c.gap.main(dir), items.len());
    let total_hypothetical_outer_main_size: f32 = items.iter().map(|ch| ch.hypothetical_outer_size.main(dir)).sum();
    let used_flex_factor = total_main_axis_gap + total_hypothetical_outer_main_size;
    let inner_main = c.node_inner_size.main(dir);
    let growing = used_flex_factor < inner_main.unwrap_or(0.0);
    let shrinking = used_flex_factor > inner_main.unwrap_or(0.0);
    let exactly_sized = !growing && !shrinking;

    for child in items.iter_mut() {
        let inner_target_size = child.hypothetical_inner_size.main(dir);
        child.target_size.set_main(dir, inner_target_size);
        if exactly_sized
            || (child.flex_grow == 0.0 && child.flex_shrink == 0.0)
            || (growing && child.flex_basis > child.hypothetical_inner_size.main(dir))
            || (shrinking && child.flex_basis < child.hypothetical_inner_size.main(dir))
        {
            child.frozen = true;
            child.outer_target_size.set_main(dir, inner_target_size + child.margin.main_axis_sum(dir));
        }
    }
    if exactly_sized {
        return;
    }

    let used_space = |items: &[FlexItem]| -> f32 {
        total_main_axis_gap
            + items
                .iter()
                .map(|ch| if ch.frozen { ch.outer_target_size.main(dir) } else { ch.flex_basis + ch.margin.main_axis_sum(dir) })
                .sum::<f32>()
    };
    let initial_free_space = inner_main.maybe_sub(used_space(items)).unwrap_or(0.0);

    loop {
        if items.iter().all(|ch| ch.frozen) {
            break;
        }
        let used = used_space(items);
        let (sum_flex_grow, sum_flex_shrink) = items
            .iter()
            .filter(|ch| !ch.frozen)
            .fold((0.0f32, 0.0f32), |(g, s), ch| (g + ch.flex_grow, s + ch.flex_shrink));
        let free_space = if growing && sum_flex_grow < 1.0 {
            (initial_free_space * sum_flex_grow - total_main_axis_gap).maybe_min(inner_main.maybe_sub(used))
        } else if shrinking && sum_flex_shrink < 1.0 {
            (initial_free_space * sum_flex_shrink - total_main_axis_gap).maybe_max(inner_main.maybe_sub(used))
        } else {
            inner_main.maybe_sub(used).unwrap_or(used_flex_factor - used)
        };

        if free_space.is_normal() {
            if growing && sum_flex_grow > 0.0 {
                for ch in items.iter_mut().filter(|ch| !ch.frozen) {
                    ch.target_size.set_main(dir, ch.flex_basis + free_space * (ch.flex_grow / sum_flex_grow));
                }
            } else if shrinking && sum_flex_shrink > 0.0 {
                let sum_scaled_shrink_factor: f32 =
                    items.iter().filter(|ch| !ch.frozen).map(|ch| ch.inner_flex_basis * ch.flex_shrink).sum();
                if sum_scaled_shrink_factor > 0.0 {
                    for ch in items.iter_mut().filter(|ch| !ch.frozen) {
                        let scaled_shrink_factor = ch.inner_flex_basis * ch.flex_shrink;
                        ch.target_size
                            .set_main(dir, ch.flex_basis + free_space * (scaled_shrink_factor / sum_scaled_shrink_factor));
                    }
                }
            }
        }

        let mut total_violation = 0.0f32;
        for ch in items.iter_mut().filter(|ch| !ch.frozen) {
            let clamped = ch.target_size.main(dir).maybe_clamp(Some(ch.resolved_minimum_main_size), ch.max_size.main(dir)).max(0.0);
            ch.violation = clamped - ch.target_size.main(dir);
            ch.target_size.set_main(dir, clamped);
            ch.outer_target_size.set_main(dir, clamped + ch.margin.main_axis_sum(dir));
            total_violation += ch.violation;
        }
        for ch in items.iter_mut().filter(|ch| !ch.frozen) {
            ch.frozen = if total_violation > 0.0 {
                ch.violation > 0.0
            } else if total_violation < 0.0 {
                ch.violation < 0.0
            } else {
                true
            };
        }
    }
}
