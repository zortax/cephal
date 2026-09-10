//! Per-thread reusable buffers, so container passes allocate nothing in steady state.

use core::ops::{Deref, DerefMut};

/// Buffers kept per element type; more than this many idle ones are freed.
const KEEP: usize = 32;
/// Buffers larger than this are freed rather than kept.
const MAX_KEPT_CAPACITY: usize = 1 << 16;

/// An element type with its own thread-local pool of vectors.
pub trait Pooled: Sized + 'static {
    fn with_pool<R>(f: impl FnOnce(&mut Vec<Vec<Self>>) -> R) -> R;
}

/// Gives each listed type a pool.
macro_rules! pooled {
    ($($t:ty),* $(,)?) => {$(
        impl $crate::compute::scratch::Pooled for $t {
            #[inline]
            fn with_pool<R>(f: impl FnOnce(&mut Vec<Vec<Self>>) -> R) -> R {
                thread_local!(static POOL: ::std::cell::RefCell<Vec<Vec<$t>>> = const { ::std::cell::RefCell::new(Vec::new()) });
                POOL.with(|p| f(&mut p.borrow_mut()))
            }
        }
    )*};
}
pub(crate) use pooled;

/// A vector borrowed from the thread's pool; cleared and returned on drop.
pub struct Scratch<T: Pooled>(Vec<T>);

impl<T: Pooled> Scratch<T> {
    #[inline]
    pub fn take() -> Self {
        Self(T::with_pool(|pool| pool.pop()).unwrap_or_default())
    }

    #[inline]
    pub fn with_capacity(n: usize) -> Self {
        let mut s = Self::take();
        s.0.reserve(n);
        s
    }

    /// The buffer, filled from `iter`.
    #[inline]
    pub fn collect(iter: impl IntoIterator<Item = T>) -> Self {
        let mut s = Self::take();
        s.0.extend(iter);
        s
    }
}

impl<T: Pooled> Drop for Scratch<T> {
    #[inline]
    fn drop(&mut self) {
        let mut vec = core::mem::take(&mut self.0);
        vec.clear();
        if vec.capacity() == 0 || vec.capacity() > MAX_KEPT_CAPACITY {
            return;
        }
        T::with_pool(|pool| {
            if pool.len() < KEEP {
                pool.push(vec);
            }
        });
    }
}

impl<'a, T: Pooled> IntoIterator for &'a Scratch<T> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a, T: Pooled> IntoIterator for &'a mut Scratch<T> {
    type Item = &'a mut T;
    type IntoIter = core::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

impl<T: Pooled> Deref for Scratch<T> {
    type Target = Vec<T>;
    #[inline]
    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T: Pooled> DerefMut for Scratch<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}

pooled!(crate::tree::ChildRequest, crate::tree::LayoutOutput, crate::tree::NodeId, (usize, crate::tree::NodeId));
