//! §9.7 resolving flexible lengths.

use super::axis::FlexDir;
use super::axis::{FlexAxisSize, FlexAxisSum, sum_axis_gaps};
use super::{AlgoConstants, FlexItem};
use crate::geometry::MaybeMath;

/// What resolving flexible lengths reads and writes of an item, so every item representation
/// runs the same arithmetic.
pub(super) trait Flexible {
    fn hypothetical_inner_main(&self, dir: impl FlexDir) -> f32;
    fn hypothetical_outer_main(&self, dir: impl FlexDir) -> f32;
    fn flex_basis(&self) -> f32;
    fn inner_flex_basis(&self) -> f32;
    fn flex_grow(&self) -> f32;
    fn flex_shrink(&self) -> f32;
    fn margin_main_sum(&self, dir: impl FlexDir) -> f32;
    fn resolved_minimum_main_size(&self) -> f32;
    fn max_main(&self, dir: impl FlexDir) -> Option<f32>;
    fn frozen(&self) -> bool;
    fn set_frozen(&mut self, frozen: bool);
    fn target_main(&self, dir: impl FlexDir) -> f32;
    fn set_target_main(&mut self, dir: impl FlexDir, v: f32);
    fn outer_target_main(&self, dir: impl FlexDir) -> f32;
    fn set_outer_target_main(&mut self, dir: impl FlexDir, v: f32);
    fn violation(&self) -> f32;
    fn set_violation(&mut self, v: f32);
}

impl Flexible for FlexItem {
    #[inline(always)]
    fn hypothetical_inner_main(&self, dir: impl FlexDir) -> f32 {
        self.hypothetical_inner_size.main(dir)
    }
    #[inline(always)]
    fn hypothetical_outer_main(&self, dir: impl FlexDir) -> f32 {
        self.hypothetical_outer_size.main(dir)
    }
    #[inline(always)]
    fn flex_basis(&self) -> f32 {
        self.flex_basis
    }
    #[inline(always)]
    fn inner_flex_basis(&self) -> f32 {
        self.inner_flex_basis
    }
    #[inline(always)]
    fn flex_grow(&self) -> f32 {
        self.flex_grow
    }
    #[inline(always)]
    fn flex_shrink(&self) -> f32 {
        self.flex_shrink
    }
    #[inline(always)]
    fn margin_main_sum(&self, dir: impl FlexDir) -> f32 {
        self.margin.main_axis_sum(dir)
    }
    #[inline(always)]
    fn resolved_minimum_main_size(&self) -> f32 {
        self.resolved_minimum_main_size
    }
    #[inline(always)]
    fn max_main(&self, dir: impl FlexDir) -> Option<f32> {
        self.max_size.main(dir)
    }
    #[inline(always)]
    fn frozen(&self) -> bool {
        self.frozen
    }
    #[inline(always)]
    fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
    }
    #[inline(always)]
    fn target_main(&self, dir: impl FlexDir) -> f32 {
        self.target_size.main(dir)
    }
    #[inline(always)]
    fn set_target_main(&mut self, dir: impl FlexDir, v: f32) {
        self.target_size.set_main(dir, v);
    }
    #[inline(always)]
    fn outer_target_main(&self, dir: impl FlexDir) -> f32 {
        self.outer_target_size.main(dir)
    }
    #[inline(always)]
    fn set_outer_target_main(&mut self, dir: impl FlexDir, v: f32) {
        self.outer_target_size.set_main(dir, v);
    }
    #[inline(always)]
    fn violation(&self) -> f32 {
        self.violation
    }
    #[inline(always)]
    fn set_violation(&mut self, v: f32) {
        self.violation = v;
    }
}

/// What one pass over a line gathers for the next step: the used space (without gaps) and the
/// unfrozen items' factors. Terms are added item by item in line order, starting where `sum` and
/// `fold` would, so each total is the one a pass of its own computes.
struct Gathered {
    used: f32,
    unfrozen: bool,
    grow: f32,
    shrink: f32,
    scaled_shrink: f32,
}

impl Gathered {
    #[inline(always)]
    fn new() -> Self {
        Self { used: -0.0, unfrozen: false, grow: 0.0, shrink: 0.0, scaled_shrink: -0.0 }
    }

    #[inline(always)]
    fn add<I: Flexible>(&mut self, ch: &I, dir: impl FlexDir) {
        if ch.frozen() {
            self.used += ch.outer_target_main(dir);
        } else {
            self.used += ch.flex_basis() + ch.margin_main_sum(dir);
            self.unfrozen = true;
            self.grow += ch.flex_grow();
            self.shrink += ch.flex_shrink();
            self.scaled_shrink += ch.inner_flex_basis() * ch.flex_shrink();
        }
    }
}

/// How a round distributes free space over the unfrozen items.
#[derive(Clone, Copy)]
enum Distribute {
    Nothing,
    Grow(f32),
    Shrink(f32),
}

pub(super) fn resolve_flexible_lengths<D: FlexDir, I: Flexible>(items: &mut [I], c: &AlgoConstants<D>) {
    let dir = c.dir;
    let total_main_axis_gap = sum_axis_gaps(c.gap.main(dir), items.len());
    let total_hypothetical_outer_main_size: f32 = items.iter().map(|ch| ch.hypothetical_outer_main(dir)).sum();
    let used_flex_factor = total_main_axis_gap + total_hypothetical_outer_main_size;
    let inner_main = c.node_inner_size.main(dir);
    let growing = used_flex_factor < inner_main.unwrap_or(0.0);
    let shrinking = used_flex_factor > inner_main.unwrap_or(0.0);
    let exactly_sized = !growing && !shrinking;

    if exactly_sized {
        for child in items.iter_mut() {
            let inner_target_size = child.hypothetical_inner_main(dir);
            child.set_target_main(dir, inner_target_size);
            child.set_frozen(true);
            child.set_outer_target_main(dir, inner_target_size + child.margin_main_sum(dir));
        }
        return;
    }
    let mut gathered = Gathered::new();
    for child in items.iter_mut() {
        let inner_target_size = child.hypothetical_inner_main(dir);
        child.set_target_main(dir, inner_target_size);
        if (child.flex_grow() == 0.0 && child.flex_shrink() == 0.0)
            || (growing && child.flex_basis() > child.hypothetical_inner_main(dir))
            || (shrinking && child.flex_basis() < child.hypothetical_inner_main(dir))
        {
            child.set_frozen(true);
            child.set_outer_target_main(dir, inner_target_size + child.margin_main_sum(dir));
        }
        gathered.add(child, dir);
    }

    let initial_free_space = inner_main.maybe_sub(total_main_axis_gap + gathered.used).unwrap_or(0.0);
    while gathered.unfrozen {
        let used = total_main_axis_gap + gathered.used;
        let (sum_flex_grow, sum_flex_shrink) = (gathered.grow, gathered.shrink);
        let free_space = if growing && sum_flex_grow < 1.0 {
            (initial_free_space * sum_flex_grow - total_main_axis_gap).maybe_min(inner_main.maybe_sub(used))
        } else if shrinking && sum_flex_shrink < 1.0 {
            (initial_free_space * sum_flex_shrink - total_main_axis_gap).maybe_max(inner_main.maybe_sub(used))
        } else {
            inner_main.maybe_sub(used).unwrap_or(used_flex_factor - used)
        };

        let distribute = if !free_space.is_normal() {
            Distribute::Nothing
        } else if growing && sum_flex_grow > 0.0 {
            Distribute::Grow(sum_flex_grow)
        } else if shrinking && sum_flex_shrink > 0.0 && gathered.scaled_shrink > 0.0 {
            Distribute::Shrink(gathered.scaled_shrink)
        } else {
            Distribute::Nothing
        };

        let mut total_violation = 0.0f32;
        for ch in items.iter_mut().filter(|ch| !ch.frozen()) {
            match distribute {
                Distribute::Nothing => {}
                Distribute::Grow(sum_flex_grow) => {
                    let target = ch.flex_basis() + free_space * (ch.flex_grow() / sum_flex_grow);
                    ch.set_target_main(dir, target);
                }
                Distribute::Shrink(sum_scaled_shrink_factor) => {
                    let scaled_shrink_factor = ch.inner_flex_basis() * ch.flex_shrink();
                    let target = ch.flex_basis() + free_space * (scaled_shrink_factor / sum_scaled_shrink_factor);
                    ch.set_target_main(dir, target);
                }
            }
            let clamped =
                ch.target_main(dir).maybe_clamp(Some(ch.resolved_minimum_main_size()), ch.max_main(dir)).max(0.0);
            let violation = clamped - ch.target_main(dir);
            ch.set_violation(violation);
            ch.set_target_main(dir, clamped);
            ch.set_outer_target_main(dir, clamped + ch.margin_main_sum(dir));
            total_violation += violation;
        }
        gathered = Gathered::new();
        for ch in items.iter_mut() {
            if !ch.frozen() {
                let frozen = if total_violation > 0.0 {
                    ch.violation() > 0.0
                } else if total_violation < 0.0 {
                    ch.violation() < 0.0
                } else {
                    true
                };
                ch.set_frozen(frozen);
            }
            gathered.add(ch, dir);
        }
    }
}
