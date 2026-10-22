//! Stored answers.

use super::CacheKey;
use crate::geometry::{Rect, Size};
use crate::tree::{Baselines, CollapsibleMarginSet, LayoutOutput};
use core::sync::atomic::{AtomicU32, Ordering};

/// The final-layout answer.
#[derive(Debug)]
pub(super) struct Entry {
    pub key: CacheKey,
    pub output: LayoutOutput,
    /// Frame the entry was last hit in; atomic so a hit needs only shared access.
    pub generation: AtomicU32,
    /// Axes on which the answer told min-content from max-content available space.
    pub observed: u8,
}

impl Entry {
    #[inline]
    pub fn generation(&self) -> u32 {
        self.generation.load(Ordering::Relaxed)
    }
    #[inline]
    pub fn touch(&self, generation: u32) {
        self.generation.store(generation, Ordering::Relaxed);
    }
}

impl Clone for Entry {
    fn clone(&self) -> Self {
        Self { key: self.key, output: self.output, generation: AtomicU32::new(self.generation()), observed: self.observed }
    }
}

const FIRST_BASELINE: u8 = 1 << 0;
const LAST_BASELINE: u8 = 1 << 1;
const COLLAPSES_THROUGH: u8 = 1 << 2;

/// What a size answer specifies (see [`LayoutOutput::size_answer`]), packed: the size, the
/// baselines, and the collapsible margins block flow reads off its children's size answers.
#[derive(Clone, Copy, Debug)]
pub(super) struct SizeAnswer {
    size: Size<f32>,
    /// Bits of absent baselines are zero.
    baselines: [f32; 2],
    top_margin: CollapsibleMarginSet,
    bottom_margin: CollapsibleMarginSet,
    flags: u8,
}

impl SizeAnswer {
    pub const EMPTY: Self = Self {
        size: Size { width: 0.0, height: 0.0 },
        baselines: [0.0; 2],
        top_margin: CollapsibleMarginSet::ZERO,
        bottom_margin: CollapsibleMarginSet::ZERO,
        flags: 0,
    };

    #[inline]
    pub fn new(output: &LayoutOutput) -> Self {
        let Baselines { first, last } = output.baselines;
        let flags = (first.is_some() as u8 * FIRST_BASELINE)
            | (last.is_some() as u8 * LAST_BASELINE)
            | (output.margins_can_collapse_through as u8 * COLLAPSES_THROUGH);
        Self {
            size: output.size,
            baselines: [first.unwrap_or(0.0), last.unwrap_or(0.0)],
            top_margin: output.top_margin,
            bottom_margin: output.bottom_margin,
            flags,
        }
    }

    /// The answer as handed out: equal to [`LayoutOutput::size_answer`] of the stored output.
    #[inline]
    pub fn output(&self) -> LayoutOutput {
        let f = self.flags;
        LayoutOutput {
            size: self.size,
            scrollable_overflow_rect: Rect::ZERO,
            inflow_overflow_rect: Rect::ZERO,
            baselines: Baselines {
                first: (f & FIRST_BASELINE != 0).then_some(self.baselines[0]),
                last: (f & LAST_BASELINE != 0).then_some(self.baselines[1]),
            },
            top_margin: self.top_margin,
            bottom_margin: self.bottom_margin,
            margins_can_collapse_through: f & COLLAPSES_THROUGH != 0,
            oof: 0,
        }
    }
}

/// Size answers in parallel arrays, so a lookup scans keys alone; slots `..len` of the owner are
/// live.
pub(super) struct Slots<const N: usize> {
    pub keys: [CacheKey; N],
    pub answers: [SizeAnswer; N],
    /// Frame each entry was last hit in; atomic so a hit needs only shared access.
    pub generations: [AtomicU32; N],
    /// Axes on which each answer told min-content from max-content available space.
    pub observed: [u8; N],
}

impl<const N: usize> Slots<N> {
    pub const fn empty() -> Self {
        Self { keys: [CacheKey::EMPTY; N], answers: [SizeAnswer::EMPTY; N], generations: [const { AtomicU32::new(0) }; N], observed: [0; N] }
    }

    /// The first of slots `..len` whose entry answers `key`.
    #[inline(always)]
    pub fn find(&self, len: usize, key: &CacheKey) -> Option<usize> {
        let len = len.min(N);
        (0..len).find(|&i| self.keys[i].answers_unobserved(key, self.observed[i]))
    }

    #[inline]
    pub fn generation(&self, i: usize) -> u32 {
        self.generations[i].load(Ordering::Relaxed)
    }

    #[inline]
    pub fn touch(&self, i: usize, generation: u32) {
        self.generations[i].store(generation, Ordering::Relaxed);
    }

    #[inline]
    pub fn set(&mut self, i: usize, key: CacheKey, answer: SizeAnswer, generation: u32, observed: u8) {
        self.keys[i] = key;
        self.answers[i] = answer;
        *self.generations[i].get_mut() = generation;
        self.observed[i] = observed;
    }

    /// Slot `i`'s question and answer, with its observed axes.
    #[inline]
    pub fn get(&self, i: usize) -> (CacheKey, LayoutOutput, u8) {
        (self.keys[i], self.answers[i].output(), self.observed[i])
    }
}

impl<const N: usize> Clone for Slots<N> {
    fn clone(&self) -> Self {
        Self {
            keys: self.keys,
            answers: self.answers,
            generations: core::array::from_fn(|i| AtomicU32::new(self.generation(i))),
            observed: self.observed,
        }
    }
}
