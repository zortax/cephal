//! Stored answers.

use super::CacheKey;
use crate::tree::LayoutOutput;
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug)]
pub(super) struct Entry {
    pub key: CacheKey,
    pub output: LayoutOutput,
    /// Frame the entry was last hit in; atomic so a hit needs only shared access.
    pub generation: AtomicU32,
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
        Self { key: self.key, output: self.output, generation: AtomicU32::new(self.generation()) }
    }
}
