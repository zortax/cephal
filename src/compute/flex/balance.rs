//! `flex-wrap: balance`: minimise the sum of squared line sizes (css-flexbox-2).

const INFEASIBLE: f64 = f64::INFINITY;

#[inline]
fn to_size(v: f32) -> f64 {
    (v as f64).clamp(0.0, f32::MAX as f64)
}

struct LineSizes {
    /// Prefix sums of `size + gap`, with a flag for zero-sized items.
    sums: Vec<(f64, bool)>,
    gap: f64,
    limit: f64,
}

impl LineSizes {
    #[inline]
    fn item_count(&self) -> usize {
        self.sums.len()
    }
    #[inline]
    fn is_zero_item(&self, index: usize) -> bool {
        self.sums[index].1
    }
    #[inline]
    fn line_size(&self, start: usize, end: usize) -> f64 {
        let start_sum = if start == 0 { 0.0 } else { self.sums[start - 1].0 };
        self.sums[end].0 - start_sum - self.gap
    }
    #[inline]
    fn line_cost(&self, start: usize, end: usize) -> f64 {
        let size = self.line_size(start, end);
        if end > start && size > self.limit {
            return INFEASIBLE;
        }
        size * size
    }
    /// Fewest lines needed for each suffix.
    fn suffix_greedy_line_counts(&self) -> Vec<u32> {
        let n = self.item_count();
        let mut counts = vec![0u32; n + 1];
        let mut line_end = n;
        for start in (0..n).rev() {
            while line_end > start + 1 && self.line_size(start, line_end - 1) > self.limit {
                line_end -= 1;
            }
            counts[start] = 1 + counts[line_end];
        }
        counts
    }
    /// Furthest item that still fits on a line starting at each index.
    fn fit_ends(&self) -> Vec<u32> {
        let n = self.item_count();
        let mut out = Vec::with_capacity(n);
        let mut fit_end = 0;
        for start in 0..n {
            if fit_end < start {
                fit_end = start;
            }
            while fit_end + 1 < n && self.line_size(start, fit_end + 1) <= self.limit {
                fit_end += 1;
            }
            out.push(fit_end as u32);
        }
        out
    }
}

struct Row<'a> {
    sizes: &'a LineSizes,
    fit_ends: &'a [u32],
    prev: &'a [f64],
    nonzero_ends: &'a [u32],
    max_end: usize,
}

/// Divide-and-conquer DP fill exploiting monotone optimal split points.
fn fill_row(row: &Row, cur: &mut [f64], opts: &mut [u32], start_lo: usize, start_hi: usize, col_lo: usize, col_hi: usize) {
    if start_lo > start_hi {
        return;
    }
    let start = start_lo + (start_hi - start_lo) / 2;
    let first_col = col_lo + row.nonzero_ends[col_lo..col_hi].partition_point(|&end| (end as usize) < start);
    let mut min_cost = INFEASIBLE;
    let mut min_col = None;
    for col in first_col..col_hi {
        let end = row.nonzero_ends[col] as usize;
        let line_cost = row.sizes.line_cost(start, end);
        if line_cost == INFEASIBLE {
            break;
        }
        let cost = line_cost + row.prev[end + 1];
        if cost <= min_cost {
            min_cost = cost;
            min_col = Some(col);
        }
    }
    let mut min_end = min_col.map(|col| row.nonzero_ends[col] as usize);
    let zero_end = (row.fit_ends[start] as usize).min(row.max_end);
    if row.sizes.is_zero_item(zero_end + 1) {
        let cost = row.sizes.line_cost(start, zero_end) + row.prev[zero_end + 1];
        if cost < min_cost || (cost == min_cost && min_end.is_none_or(|end| end < zero_end)) {
            min_cost = cost;
            min_end = Some(zero_end);
        }
    }
    debug_assert!(min_cost.is_finite());
    cur[start] = min_cost;
    opts[start] = min_end.unwrap_or(start) as u32;
    if start > start_lo {
        let col_hi = min_col.map_or(col_hi, |col| col + 1);
        fill_row(row, cur, opts, start_lo, start - 1, col_lo, col_hi);
    }
    if start < start_hi {
        let col_lo = min_col.unwrap_or(first_col);
        fill_row(row, cur, opts, start + 1, start_hi, col_lo, col_hi);
    }
}

/// Items per line for the balanced breaking of `item_sizes` under `line_limit`.
pub(super) fn balanced_line_item_counts(
    item_sizes: impl ExactSizeIterator<Item = f32>,
    line_limit: f32,
    gap_between_items: f32,
    min_line_count: usize,
) -> Vec<usize> {
    let item_count = item_sizes.len();
    debug_assert!(item_count > 0);
    let gap = to_size(gap_between_items);
    let limit = line_limit as f64;
    let mut sums = Vec::with_capacity(item_count);
    let mut sum = 0.0;
    for size in item_sizes {
        let size = to_size(size);
        sum += size + gap;
        sums.push((sum, size == 0.0));
    }
    let sizes = LineSizes { sums, gap, limit };
    let suffix_greedy = sizes.suffix_greedy_line_counts();
    let line_count = (suffix_greedy[0] as usize).max(min_line_count.clamp(1, item_count));
    let mut item_counts = Vec::with_capacity(line_count);
    if line_count == item_count {
        item_counts.resize(item_count, 1);
        return item_counts;
    }
    let fit_ends = sizes.fit_ends();
    let nonzero_ends: Vec<u32> = (0..item_count - 1).filter(|&end| !sizes.is_zero_item(end + 1)).map(|e| e as u32).collect();
    let mut prev: Vec<f64> = (0..item_count).map(|start| sizes.line_cost(start, item_count - 1)).collect();
    let mut cur = vec![INFEASIBLE; item_count];
    let mut opts = vec![0u32; (line_count - 1) * item_count];
    for lines in 2..=line_count {
        let max_end = item_count - lines;
        let mut first_start = 0;
        while first_start < max_end && suffix_greedy[first_start] as usize > lines {
            cur[first_start] = INFEASIBLE;
            first_start += 1;
        }
        let col_hi = nonzero_ends.partition_point(|&end| (end as usize) <= max_end);
        let row = Row { sizes: &sizes, fit_ends: &fit_ends, prev: &prev, nonzero_ends: &nonzero_ends, max_end };
        let opts_row = &mut opts[(lines - 2) * item_count..(lines - 1) * item_count];
        fill_row(&row, &mut cur, opts_row, first_start, max_end, 0, col_hi);
        core::mem::swap(&mut prev, &mut cur);
    }
    debug_assert!(prev[0].is_finite());
    let mut start = 0;
    for lines in (2..=line_count).rev() {
        let end = opts[(lines - 2) * item_count + start] as usize;
        item_counts.push(end - start + 1);
        start = end + 1;
    }
    item_counts.push(item_count - start);
    item_counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balances_equal_items() {
        assert_eq!(balanced_line_item_counts([10.0; 6].into_iter(), 35.0, 0.0, 1), vec![3, 3]);
        assert_eq!(balanced_line_item_counts([10.0; 4].into_iter(), 100.0, 0.0, 2), vec![2, 2]);
        // The fewest lines that fit win; balance only distributes among them.
        assert_eq!(balanced_line_item_counts([30.0, 10.0, 10.0, 30.0].into_iter(), 40.0, 0.0, 1), vec![2, 2]);
        assert_eq!(balanced_line_item_counts([30.0, 10.0, 10.0, 30.0].into_iter(), 40.0, 0.0, 3), vec![1, 2, 1]);
    }
}
