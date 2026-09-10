//! Per-node memo of answers keyed by the exact question asked.

mod entry;
mod key;

pub use key::CacheKey;

use super::{LayoutInput, LayoutOutput, RunMode};
use entry::Entry;

const INLINE_SIZE_ENTRIES: usize = 3;
const SPILL_SIZE_ENTRIES: usize = 8;

/// One `PerformLayout` answer plus a few `ComputeSize` answers.
#[derive(Clone, Debug, Default)]
pub struct NodeCache {
    final_entry: Option<Entry>,
    inline: [Option<Entry>; INLINE_SIZE_ENTRIES],
    spill: Option<Box<[Option<Entry>; SPILL_SIZE_ENTRIES]>>,
    /// Set when a size entry was evicted since the last drain.
    evicted: bool,
}

impl NodeCache {
    pub const fn new() -> Self {
        Self { final_entry: None, inline: [const { None }; INLINE_SIZE_ENTRIES], spill: None, evicted: false }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.final_entry.is_none()
            && self.inline.iter().all(Option::is_none)
            && self.spill.as_ref().is_none_or(|s| s.iter().all(Option::is_none))
    }

    /// Looks up an answer, marking the entry live for `generation`; shared access suffices.
    #[inline]
    pub fn get(&self, input: &LayoutInput, generation: u32) -> Option<LayoutOutput> {
        self.get_keyed(&CacheKey::from_input(input), input.run_mode, generation)
    }

    /// [`Self::get`] with the key already built.
    #[inline]
    pub fn get_keyed(&self, key: &CacheKey, run_mode: RunMode, generation: u32) -> Option<LayoutOutput> {
        match run_mode {
            RunMode::PerformLayout => {
                let e = self.final_entry.as_ref()?;
                if e.key == *key {
                    e.touch(generation);
                    Some(e.output)
                } else {
                    None
                }
            }
            RunMode::ComputeSize => {
                self.inline[0].as_ref()?;
                for e in self.size_entries() {
                    if let Some(e) = e
                        && e.key.answers(key)
                    {
                        e.touch(generation);
                        return Some(e.output);
                    }
                }
                #[cfg(feature = "stats")]
                if self.size_entries().any(|e| e.as_ref().is_some_and(|e| e.key.answers_relaxed(key))) {
                    crate::compute::stats::bump(&crate::compute::stats::RELAXED_HIT);
                }
                None
            }
            RunMode::PerformHiddenLayout => None,
        }
    }

    /// Whether `input` would hit, without marking anything live.
    pub fn holds(&self, input: &LayoutInput) -> bool {
        let key = CacheKey::from_input(input);
        match input.run_mode {
            RunMode::PerformLayout => self.final_entry.as_ref().is_some_and(|e| e.key == key),
            RunMode::ComputeSize => self.size_entries().any(|e| e.as_ref().is_some_and(|e| e.key.answers(&key))),
            RunMode::PerformHiddenLayout => false,
        }
    }

    pub fn store(&mut self, input: &LayoutInput, output: LayoutOutput, generation: u32) {
        self.store_keyed(CacheKey::from_input(input), input.run_mode, output, generation);
    }

    /// [`Self::store`] with the key already built.
    pub fn store_keyed(&mut self, key: CacheKey, run_mode: RunMode, output: LayoutOutput, generation: u32) {
        let entry = Entry { key, output, generation: core::sync::atomic::AtomicU32::new(generation) };
        match run_mode {
            RunMode::PerformLayout => {
                // Two final-layout questions in one frame: the parent depends on both, only one survives.
                if let Some(e) = &self.final_entry
                    && e.generation() == generation
                    && e.key != key
                {
                    self.evicted = true;
                }
                self.final_entry = Some(entry);
            }
            RunMode::ComputeSize => {
                // Replace an identical key in place.
                if let Some(slot) = self.size_entries_mut().find(|s| s.as_ref().is_some_and(|e| e.key == key)) {
                    *slot = Some(entry);
                    return;
                }
                if let Some(slot) = self.inline.iter_mut().find(|s| s.is_none()) {
                    *slot = Some(entry);
                    return;
                }
                let spill = self.spill.get_or_insert_with(|| Box::new([const { None }; SPILL_SIZE_ENTRIES]));
                if let Some(slot) = spill.iter_mut().find(|s| s.is_none()) {
                    *slot = Some(entry);
                    return;
                }
                // Evict the least recently used entry.
                self.evicted = true;
                let victim = self
                    .size_entries_mut()
                    .min_by_key(|s| s.as_ref().map_or(0, |e| e.generation()))
                    .expect("cache has size slots");
                *victim = Some(entry);
            }
            RunMode::PerformHiddenLayout => {}
        }
    }

    /// Drops every answer.
    pub fn clear(&mut self) {
        self.final_entry = None;
        self.inline = [const { None }; INLINE_SIZE_ENTRIES];
        self.spill = None;
        self.evicted = false;
    }

    /// The cached final-layout answer, if any.
    #[inline]
    pub fn final_output(&self) -> Option<LayoutOutput> {
        self.final_entry.as_ref().map(|e| e.output)
    }

    /// Replaces the cached final-layout answer in place.
    #[inline]
    pub fn set_final_output(&mut self, output: LayoutOutput) {
        if let Some(e) = self.final_entry.as_mut() {
            e.output = output;
        }
    }

    /// Every cached answer, for inspection.
    pub fn entries(&self) -> Vec<(LayoutInput, LayoutOutput)> {
        let mut out = Vec::new();
        for e in self.inline.iter().chain(self.spill.iter().flat_map(|s| s.iter())).flatten() {
            out.push((e.key.into_input(RunMode::ComputeSize), e.output));
        }
        if let Some(e) = &self.final_entry {
            out.push((e.key.into_input(RunMode::PerformLayout), e.output));
        }
        out
    }

    /// Whether eviction may have removed an entry the parent relied on.
    #[inline]
    pub fn had_eviction(&self) -> bool {
        self.evicted
    }

    /// Removes every answer, handing them back in recomputation order: sizes first, the final
    /// layout last, as a cold layout would ask them.
    pub fn drain_all(&mut self, out: &mut Vec<(LayoutInput, LayoutOutput)>) {
        let final_entry = self.final_entry.take();
        for slot in self.inline.iter_mut() {
            if let Some(e) = slot.take() {
                out.push((e.key.into_input(RunMode::ComputeSize), e.output));
            }
        }
        if let Some(spill) = self.spill.take() {
            for e in spill.into_iter().flatten() {
                out.push((e.key.into_input(RunMode::ComputeSize), e.output));
            }
        }
        if let Some(e) = final_entry {
            out.push((e.key.into_input(RunMode::PerformLayout), e.output));
        }
        self.evicted = false;
    }

    #[inline]
    fn size_entries(&self) -> impl Iterator<Item = &Option<Entry>> {
        self.inline.iter().chain(self.spill.as_deref().into_iter().flat_map(|s| s.iter()))
    }

    fn size_entries_mut(&mut self) -> impl Iterator<Item = &mut Option<Entry>> {
        self.inline.iter_mut().chain(self.spill.as_deref_mut().into_iter().flat_map(|s| s.iter_mut()))
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
