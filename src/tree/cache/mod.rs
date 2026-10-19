//! Per-node memo of answers keyed by the exact question asked.

mod entry;
mod key;

pub use key::CacheKey;

use super::{LayoutInput, LayoutOutput, RunMode};
use entry::{Entry, SizeAnswer, Slots};

/// Size answers kept per node, the first few without a second allocation.
const SIZE_ENTRIES: usize = 11;
const INLINE_SIZE_ENTRIES: usize = 3;
const SPILL_SIZE_ENTRIES: usize = SIZE_ENTRIES - INLINE_SIZE_ENTRIES;

/// One `PerformLayout` answer plus a few `ComputeSize` answers.
///
/// Allocated on the first store, so nodes that never store (cheap leaves) cost one word. A size
/// answer keeps only what [`LayoutOutput::size_answer`] specifies.
#[derive(Clone, Default)]
pub struct NodeCache(Option<Box<Inner>>);

#[derive(Clone)]
struct Inner {
    final_entry: Option<Entry>,
    /// Live size entries: the first `len` slots of `inline`, then of `spill`. Entries are never
    /// removed one by one, only all at once.
    len: u8,
    /// Set when a size entry was evicted since the last drain.
    evicted: bool,
    inline: Slots<INLINE_SIZE_ENTRIES>,
    /// Kept when the entries are cleared, for the next stores.
    spill: Option<Box<Slots<SPILL_SIZE_ENTRIES>>>,
}

impl Inner {
    const fn empty() -> Self {
        Self { final_entry: None, len: 0, evicted: false, inline: Slots::empty(), spill: None }
    }

    /// The first live size entry that answers `key`.
    #[inline]
    fn find(&self, key: &CacheKey) -> Option<usize> {
        let len = self.len as usize;
        if let Some(i) = self.inline.find(len, key) {
            return Some(i);
        }
        if len <= INLINE_SIZE_ENTRIES {
            return None;
        }
        let spill = self.spill.as_deref()?;
        spill.find(len - INLINE_SIZE_ENTRIES, key).map(|i| i + INLINE_SIZE_ENTRIES)
    }

    /// The live size entry stored under exactly `key`.
    fn find_exact(&self, key: &CacheKey) -> Option<usize> {
        (0..self.len as usize).find(|&i| self.key(i) == *key)
    }

    #[inline]
    fn key(&self, i: usize) -> CacheKey {
        match i.checked_sub(INLINE_SIZE_ENTRIES) {
            None => self.inline.keys[i],
            Some(j) => self.spill.as_deref().expect("live spill slot").keys[j],
        }
    }

    /// Size entry `i`'s question, answer and observed axes.
    #[inline]
    fn get(&self, i: usize) -> (CacheKey, LayoutOutput, u8) {
        match i.checked_sub(INLINE_SIZE_ENTRIES) {
            None => self.inline.get(i),
            Some(j) => self.spill.as_deref().expect("live spill slot").get(j),
        }
    }

    #[inline]
    fn generation(&self, i: usize) -> u32 {
        match i.checked_sub(INLINE_SIZE_ENTRIES) {
            None => self.inline.generation(i),
            Some(j) => self.spill.as_deref().expect("live spill slot").generation(j),
        }
    }

    #[inline]
    fn touch(&self, i: usize, generation: u32) {
        match i.checked_sub(INLINE_SIZE_ENTRIES) {
            None => self.inline.touch(i, generation),
            Some(j) => self.spill.as_deref().expect("live spill slot").touch(j, generation),
        }
    }

    fn set(&mut self, i: usize, key: CacheKey, answer: SizeAnswer, generation: u32, observed: u8) {
        match i.checked_sub(INLINE_SIZE_ENTRIES) {
            None => self.inline.set(i, key, answer, generation, observed),
            Some(j) => self.spill.get_or_insert_with(|| Box::new(Slots::empty())).set(j, key, answer, generation, observed),
        }
    }
}

impl core::fmt::Debug for NodeCache {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.entries()).finish()
    }
}

impl NodeCache {
    pub const fn new() -> Self {
        Self(None)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.as_deref().is_none_or(|c| c.final_entry.is_none() && c.len == 0)
    }

    /// Looks up an answer, marking the entry live for `generation`; shared access suffices.
    #[inline]
    pub fn get(&self, input: &LayoutInput, generation: u32) -> Option<LayoutOutput> {
        self.get_keyed(&CacheKey::from_input(input), input.run_mode, generation)
    }

    /// [`Self::get`] with the key already built.
    #[inline]
    pub fn get_keyed(&self, key: &CacheKey, run_mode: RunMode, generation: u32) -> Option<LayoutOutput> {
        self.lookup(key, run_mode, generation).map(|(output, _)| output)
    }

    /// [`Self::get_keyed`], also returning the entry's observed axes.
    ///
    /// A size entry answers a question that differs only between min-content and max-content
    /// available space on axes the answer never told apart.
    #[inline]
    pub fn lookup(&self, key: &CacheKey, run_mode: RunMode, generation: u32) -> Option<(LayoutOutput, u8)> {
        let c = self.0.as_deref()?;
        match run_mode {
            RunMode::PerformLayout => {
                let e = c.final_entry.as_ref()?;
                if e.key == *key {
                    e.touch(generation);
                    Some((e.output, e.observed))
                } else {
                    None
                }
            }
            RunMode::ComputeSize => {
                if c.len == 0 {
                    return None;
                }
                if let Some(i) = c.find(key) {
                    c.touch(i, generation);
                    let (_, output, observed) = c.get(i);
                    return Some((output, observed));
                }
                #[cfg(feature = "stats")]
                if (0..c.len as usize).any(|i| c.key(i).answers_relaxed(key)) {
                    crate::compute::stats::bump(&crate::compute::stats::RELAXED_HIT);
                }
                None
            }
            RunMode::PerformHiddenLayout => None,
        }
    }

    /// Whether `input` would hit, without marking anything live.
    pub fn holds(&self, input: &LayoutInput) -> bool {
        let Some(c) = self.0.as_deref() else { return false };
        let key = CacheKey::from_input(input);
        match input.run_mode {
            RunMode::PerformLayout => c.final_entry.as_ref().is_some_and(|e| e.key == key),
            RunMode::ComputeSize => c.find(&key).is_some(),
            RunMode::PerformHiddenLayout => false,
        }
    }

    /// The key of the entry that answers `key`, without marking it live.
    pub fn stored_key(&self, key: &CacheKey, run_mode: RunMode) -> Option<CacheKey> {
        let c = self.0.as_deref()?;
        match run_mode {
            RunMode::PerformLayout => c.final_entry.as_ref().filter(|e| e.key == *key).map(|e| e.key),
            RunMode::ComputeSize => c.find(key).map(|i| c.key(i)),
            RunMode::PerformHiddenLayout => None,
        }
    }

    /// The observed axes of the entry that answers `input`, without marking it live.
    pub fn observed(&self, input: &LayoutInput) -> Option<u8> {
        let c = self.0.as_deref()?;
        let key = CacheKey::from_input(input);
        match input.run_mode {
            RunMode::PerformLayout => c.final_entry.as_ref().filter(|e| e.key == key).map(|e| e.observed),
            RunMode::ComputeSize => c.find(&key).map(|i| c.get(i).2),
            RunMode::PerformHiddenLayout => None,
        }
    }

    pub fn store(&mut self, input: &LayoutInput, output: LayoutOutput, generation: u32) {
        self.store_keyed(CacheKey::from_input(input), input.run_mode, output, generation);
    }

    /// [`Self::store`] with the key already built.
    pub fn store_keyed(&mut self, key: CacheKey, run_mode: RunMode, output: LayoutOutput, generation: u32) {
        self.store_observed(key, run_mode, output, generation, crate::compute::OBSERVED_ALL);
    }

    /// [`Self::store_keyed`] for an answer that told min-content from max-content available space
    /// only on the `observed` axes. A size answer keeps only what [`LayoutOutput::size_answer`]
    /// specifies.
    pub fn store_observed(&mut self, key: CacheKey, run_mode: RunMode, output: LayoutOutput, generation: u32, observed: u8) {
        if run_mode == RunMode::PerformHiddenLayout {
            return;
        }
        let c = self.0.get_or_insert_with(|| Box::new(Inner::empty()));
        match run_mode {
            RunMode::PerformLayout => {
                // Two final-layout questions in one frame: the parent depends on both, only one survives.
                if let Some(e) = &c.final_entry
                    && e.generation() == generation
                    && e.key != key
                {
                    c.evicted = true;
                }
                c.final_entry = Some(Entry { key, output, generation: core::sync::atomic::AtomicU32::new(generation), observed });
            }
            RunMode::ComputeSize => {
                let answer = SizeAnswer::new(&output);
                // Replace an identical key in place.
                if let Some(i) = c.find_exact(&key) {
                    c.set(i, key, answer, generation, observed);
                    return;
                }
                let len = c.len as usize;
                if len < SIZE_ENTRIES {
                    c.set(len, key, answer, generation, observed);
                    c.len += 1;
                    return;
                }
                // Evict the least recently used entry.
                c.evicted = true;
                let victim = (0..SIZE_ENTRIES).min_by_key(|&i| c.generation(i)).expect("cache has size slots");
                c.set(victim, key, answer, generation, observed);
            }
            RunMode::PerformHiddenLayout => {}
        }
    }

    /// Drops every answer, keeping the allocations for the next stores.
    pub fn clear(&mut self) {
        if let Some(c) = self.0.as_deref_mut() {
            c.final_entry = None;
            c.len = 0;
            c.evicted = false;
        }
    }

    /// The cached final-layout answer, if any.
    #[inline]
    pub fn final_output(&self) -> Option<LayoutOutput> {
        self.0.as_deref()?.final_entry.as_ref().map(|e| e.output)
    }

    /// Replaces the cached final-layout answer in place.
    #[inline]
    pub fn set_final_output(&mut self, output: LayoutOutput) {
        if let Some(e) = self.0.as_deref_mut().and_then(|c| c.final_entry.as_mut()) {
            e.output = output;
        }
    }

    /// Every cached answer, for inspection.
    pub fn entries(&self) -> Vec<(LayoutInput, LayoutOutput)> {
        let mut out = Vec::new();
        let Some(c) = self.0.as_deref() else { return out };
        for i in 0..c.len as usize {
            let (key, output, _) = c.get(i);
            out.push((key.into_input(RunMode::ComputeSize), output));
        }
        if let Some(e) = &c.final_entry {
            out.push((e.key.into_input(RunMode::PerformLayout), e.output));
        }
        out
    }

    /// Whether eviction may have removed an entry the parent relied on.
    #[inline]
    pub fn had_eviction(&self) -> bool {
        self.0.as_deref().is_some_and(|c| c.evicted)
    }

    /// Removes every answer, handing them back in recomputation order: sizes first, the final
    /// layout last, as a cold layout would ask them.
    pub fn drain_all(&mut self, out: &mut Vec<(LayoutInput, LayoutOutput)>) {
        let mut all = Vec::new();
        self.drain_observed(&mut all);
        out.extend(all.into_iter().map(|(input, output, _)| (input, output)));
    }

    /// [`Self::drain_all`] with each answer's observed axes.
    pub fn drain_observed(&mut self, out: &mut Vec<(LayoutInput, LayoutOutput, u8)>) {
        let Some(c) = self.0.as_deref_mut() else { return };
        for i in 0..c.len as usize {
            let (key, output, observed) = c.get(i);
            out.push((key.into_input(RunMode::ComputeSize), output, observed));
        }
        c.len = 0;
        if let Some(e) = c.final_entry.take() {
            out.push((e.key.into_input(RunMode::PerformLayout), e.output, e.observed));
        }
        c.evicted = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{AvailableSpace, Line, Size};
    use crate::tree::{RequestedAxis, SizingMode};

    fn input(run_mode: RunMode, width: Option<f32>, avail: AvailableSpace, axis: RequestedAxis) -> LayoutInput {
        LayoutInput {
            run_mode,
            sizing_mode: SizingMode::InherentSize,
            axis,
            known_dimensions: Size { width, height: None },
            known_dimensions_are_definite: Size::TRUE,
            parent_size: Size { width: Some(100.0), height: None },
            available_space: Size { width: avail, height: AvailableSpace::MaxContent },
            vertical_margins_are_collapsible: Line::FALSE,
            context_key: 0,
        }
    }

    #[test]
    fn size_entries_keep_what_size_answers_specify() {
        use crate::geometry::Rect;
        use crate::tree::{Baselines, CollapsibleMarginSet};
        let mut c = NodeCache::new();
        let q = input(RunMode::ComputeSize, None, AvailableSpace::Definite(40.0), RequestedAxis::Both);
        let rect = Rect { left: -1.0, right: 7.0, top: 0.5, bottom: 9.0 };
        let full = LayoutOutput {
            size: Size { width: 3.0, height: -0.0 },
            scrollable_overflow_rect: rect,
            inflow_overflow_rect: rect,
            baselines: Baselines { first: Some(f32::from_bits(0x7fc0_1234)), last: None },
            top_margin: CollapsibleMarginSet { positive: 2.0, negative: -0.0 },
            bottom_margin: CollapsibleMarginSet { positive: 0.0, negative: -3.0 },
            margins_can_collapse_through: true,
            oof: 3,
        };
        c.store(&q, full, 1);
        let got = c.get(&q, 1).unwrap();
        let bits = |o: LayoutOutput| format!("{o:?} {:x?}", o.baselines.first.map(f32::to_bits));
        assert_eq!(bits(got), bits(full.size_answer()));
        let last_only = LayoutOutput { baselines: Baselines { first: None, last: Some(0.0) }, ..full };
        c.store(&q, last_only, 2);
        assert_eq!(c.get(&q, 2).unwrap().baselines, last_only.baselines);
    }

    #[test]
    fn keys_round_trip_through_drain() {
        let mut c = NodeCache::new();
        let q = input(RunMode::ComputeSize, None, AvailableSpace::Definite(40.0), RequestedAxis::Both);
        let out = LayoutOutput::from_outer_size(Size { width: 3.0, height: 4.0 });
        c.store(&q, out, 7);
        assert_eq!(c.get(&q, 8), Some(out));
        let mut all = Vec::new();
        c.drain_all(&mut all);
        assert_eq!(all, vec![(q, out)]);
        assert!(c.is_empty());
    }

    #[test]
    fn both_axis_entry_answers_single_axis_query() {
        let mut c = NodeCache::new();
        let both = input(RunMode::ComputeSize, Some(10.0), AvailableSpace::MinContent, RequestedAxis::Both);
        let horizontal = input(RunMode::ComputeSize, Some(10.0), AvailableSpace::MinContent, RequestedAxis::Horizontal);
        let out = LayoutOutput::from_outer_size(Size { width: 10.0, height: 20.0 });
        c.store(&horizontal, out, 1);
        assert_eq!(c.get(&both, 1), None);
        c.clear();
        c.store(&both, out, 1);
        assert_eq!(c.get(&horizontal, 1), Some(out));
    }

    #[test]
    fn eviction_is_reported_and_evicts_least_recently_used() {
        let mut c = NodeCache::new();
        let capacity = (INLINE_SIZE_ENTRIES + SPILL_SIZE_ENTRIES) as u32;
        for i in 0..=capacity {
            let q = input(RunMode::ComputeSize, None, AvailableSpace::Definite(i as f32), RequestedAxis::Both);
            c.store(&q, LayoutOutput::from_outer_size(Size::ZERO), i);
        }
        assert!(c.had_eviction());
        let oldest = input(RunMode::ComputeSize, None, AvailableSpace::Definite(0.0), RequestedAxis::Both);
        assert_eq!(c.get(&oldest, capacity + 1), None);
        let mut all = Vec::new();
        c.drain_all(&mut all);
        assert_eq!(all.len(), capacity as usize);
    }
}
