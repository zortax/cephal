//! A node column that workers may write element-wise through a shared reference.
//!
//! During a parallel batch the tree is shared, and each worker writes only nodes inside the
//! subtrees its batch group owns; groups are children of one container, so their subtrees are
//! disjoint and no element is ever touched by two threads at once. The main thread holds the
//! tree exclusively between batches and uses ordinary indexing.

use core::cell::UnsafeCell;
use core::ops::{Index, IndexMut};

pub(super) struct Column<T>(UnsafeCell<Vec<T>>);

// SAFETY: element-wise access is coordinated by the batch partition described above; the
// vector itself is only resized through `&mut self`.
unsafe impl<T: Send + Sync> Sync for Column<T> {}
unsafe impl<T: Send> Send for Column<T> {}

impl<T> Column<T> {
    pub fn with_capacity(n: usize) -> Self {
        Self(UnsafeCell::new(Vec::with_capacity(n)))
    }

    #[inline]
    fn vec(&self) -> &Vec<T> {
        // SAFETY: the vector header is only changed through `&mut self`.
        unsafe { &*self.0.get() }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.vec().len()
    }

    /// Element `i` without materialising a reference to the whole slice, so other elements may
    /// be written concurrently.
    #[inline]
    pub fn get(&self, i: usize) -> &T {
        let v = self.vec();
        assert!(i < v.len(), "column index out of range");
        // SAFETY: bounds checked; no `&mut` to this element exists while this borrow lives.
        unsafe { &*v.as_ptr().add(i) }
    }

    /// Mutable access to element `i` through a shared reference.
    ///
    /// # Safety
    /// The caller must be the only thread accessing element `i` for the lifetime of the borrow,
    /// which batch groups guarantee for nodes in their own subtrees.
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn slot(&self, i: usize) -> &mut T {
        let v = self.vec();
        assert!(i < v.len(), "column index out of range");
        unsafe { &mut *v.as_ptr().cast_mut().add(i) }
    }

    #[inline]
    pub fn push(&mut self, value: T) {
        self.0.get_mut().push(value);
    }

    pub fn clear(&mut self) {
        self.0.get_mut().clear();
    }

    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, T> {
        self.0.get_mut().iter_mut()
    }

    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.vec().clone()
    }
}

impl<T> From<Vec<T>> for Column<T> {
    fn from(v: Vec<T>) -> Self {
        Self(UnsafeCell::new(v))
    }
}

impl<T> Index<usize> for Column<T> {
    type Output = T;
    #[inline]
    fn index(&self, i: usize) -> &T {
        self.get(i)
    }
}

impl<T> IndexMut<usize> for Column<T> {
    #[inline]
    fn index_mut(&mut self, i: usize) -> &mut T {
        &mut self.0.get_mut()[i]
    }
}
