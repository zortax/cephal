//! Per-node memo of answers keyed by the exact question asked.

mod entry;
mod key;

pub use key::CacheKey;

use super::{LayoutInput, LayoutOutput, RunMode};
use entry::Entry;

const INLINE_SIZE_ENTRIES: usize = 3;
const SPILL_SIZE_ENTRIES: usize = 8;

/// One `PerformLayout` answer plus a few `ComputeSize` answers.
///
/// Allocated on the first store, so nodes that never store (cheap leaves) cost one word.
#[derive(Clone, Debug, Default)]
pub struct NodeCache(Option<Box<Inner>>);

#[derive(Clone, Debug)]
struct Inner {
    final_entry: Option<Entry>,
    inline: [Option<Entry>; INLINE_SIZE_ENTRIES],
    spill: Option<Box<[Option<Entry>; SPILL_SIZE_ENTRIES]>>,
    /// Set when a size entry was evicted since the last drain.
    evicted: bool,
}

impl Inner {
    const EMPTY: Self = Self { final_entry: None, inline: [const { None }; INLINE_SIZE_ENTRIES], spill: None, evicted: false };

    #[inline]
    fn size_entries(&self) -> impl Iterator<Item = &Option<Entry>> {
        self.inline.iter().chain(self.spill.as_deref().into_iter().flat_map(|s| s.iter()))
    }

    fn size_entries_mut(&mut self) -> impl Iterator<Item = &mut Option<Entry>> {
        self.inline.iter_mut().chain(self.spill.as_deref_mut().into_iter().flat_map(|s| s.iter_mut()))
    }
}

impl NodeCache {
    pub const fn new() -> Self {
        Self(None)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.as_deref().is_none_or(|c| {
            c.final_entry.is_none()
                && c.inline.iter().all(Option::is_none)
                && c.spill.as_ref().is_none_or(|s| s.iter().all(Option::is_none))
        })
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
                c.inline[0].as_ref()?;
                for e in c.size_entries() {
                    if let Some(e) = e
                        && e.key.answers_unobserved(key, e.observed)
                    {
                        e.touch(generation);
                        return Some((e.output, e.observed));
                    }
                }
                #[cfg(feature = "stats")]
                if c.size_entries().any(|e| e.as_ref().is_some_and(|e| e.key.answers_relaxed(key))) {
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
            RunMode::ComputeSize => c.size_entries().any(|e| e.as_ref().is_some_and(|e| e.key.answers_unobserved(&key, e.observed))),
            RunMode::PerformHiddenLayout => false,
        }
    }

    /// The key of the entry that answers `key`, without marking it live.
    pub fn stored_key(&self, key: &CacheKey, run_mode: RunMode) -> Option<CacheKey> {
        let c = self.0.as_deref()?;
        match run_mode {
            RunMode::PerformLayout => c.final_entry.as_ref().filter(|e| e.key == *key).map(|e| e.key),
            RunMode::ComputeSize => c.size_entries().flatten().find(|e| e.key.answers_unobserved(key, e.observed)).map(|e| e.key),
            RunMode::PerformHiddenLayout => None,
        }
    }

    /// The observed axes of the entry that answers `input`, without marking it live.
    pub fn observed(&self, input: &LayoutInput) -> Option<u8> {
        let c = self.0.as_deref()?;
        let key = CacheKey::from_input(input);
        match input.run_mode {
            RunMode::PerformLayout => c.final_entry.as_ref().filter(|e| e.key == key).map(|e| e.observed),
            RunMode::ComputeSize => c.size_entries().flatten().find(|e| e.key.answers_unobserved(&key, e.observed)).map(|e| e.observed),
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
    /// only on the `observed` axes.
    pub fn store_observed(&mut self, key: CacheKey, run_mode: RunMode, output: LayoutOutput, generation: u32, observed: u8) {
        if run_mode == RunMode::PerformHiddenLayout {
            return;
        }
        let c = self.0.get_or_insert_with(|| Box::new(Inner::EMPTY));
        let entry = Entry { key, output, generation: core::sync::atomic::AtomicU32::new(generation), observed };
        match run_mode {
            RunMode::PerformLayout => {
                // Two final-layout questions in one frame: the parent depends on both, only one survives.
                if let Some(e) = &c.final_entry
                    && e.generation() == generation
                    && e.key != key
                {
                    c.evicted = true;
                }
                c.final_entry = Some(entry);
            }
            RunMode::ComputeSize => {
                // Replace an identical key in place.
                if let Some(slot) = c.size_entries_mut().find(|s| s.as_ref().is_some_and(|e| e.key == key)) {
                    *slot = Some(entry);
                    return;
                }
                if let Some(slot) = c.inline.iter_mut().find(|s| s.is_none()) {
                    *slot = Some(entry);
                    return;
                }
                let spill = c.spill.get_or_insert_with(|| Box::new([const { None }; SPILL_SIZE_ENTRIES]));
                if let Some(slot) = spill.iter_mut().find(|s| s.is_none()) {
                    *slot = Some(entry);
                    return;
                }
                // Evict the least recently used entry.
                c.evicted = true;
                let victim = c
                    .size_entries_mut()
                    .min_by_key(|s| s.as_ref().map_or(0, |e| e.generation()))
                    .expect("cache has size slots");
                *victim = Some(entry);
            }
            RunMode::PerformHiddenLayout => {}
        }
    }

    /// Drops every answer, keeping the allocation for the next store.
    pub fn clear(&mut self) {
        if let Some(c) = self.0.as_deref_mut() {
            *c = Inner::EMPTY;
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
        for e in c.size_entries().flatten() {
            out.push((e.key.into_input(RunMode::ComputeSize), e.output));
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
        let final_entry = c.final_entry.take();
        for slot in c.inline.iter_mut() {
            if let Some(e) = slot.take() {
                out.push((e.key.into_input(RunMode::ComputeSize), e.output, e.observed));
            }
        }
        if let Some(spill) = c.spill.take() {
            for e in spill.into_iter().flatten() {
                out.push((e.key.into_input(RunMode::ComputeSize), e.output, e.observed));
            }
        }
        if let Some(e) = final_entry {
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
