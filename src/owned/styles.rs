//! Interned styles with reference counts, stored once and looked up by hash.

use crate::compute::leaf::LeafBox;
use crate::geometry::Size;
use crate::hash::{FxHashMap, FxHasher};
use crate::style::Style;
use core::hash::{Hash, Hasher};

const NONE: u32 = u32::MAX;

#[derive(Clone, Default)]
pub struct StyleTable {
    styles: Vec<Style>,
    refs: Vec<u32>,
    hashes: Vec<u64>,
    /// Next style with the same hash, or `NONE`.
    chain: Vec<u32>,
    heads: FxHashMap<u64, u32>,
    free: Vec<u32>,
    /// Bit 0: the width depends on the parent's width; bit 1: the height on its height.
    dependency: Vec<u8>,
    /// Resolved leaf boxes for styles without percentages.
    leaf_boxes: Vec<Option<LeafBox>>,
}

impl StyleTable {
    pub fn new() -> Self {
        Self::default()
    }

    fn hash_of(style: &Style) -> u64 {
        let mut h = FxHasher::default();
        style.hash(&mut h);
        h.finish()
    }

    pub fn intern(&mut self, style: Style) -> u32 {
        let hash = Self::hash_of(&style);
        let mut i = self.heads.get(&hash).copied().unwrap_or(NONE);
        while i != NONE {
            if self.styles[i as usize] == style {
                self.refs[i as usize] += 1;
                return i;
            }
            i = self.chain[i as usize];
        }
        let head = self.heads.get(&hash).copied().unwrap_or(NONE);
        let dep = style.parent_size_dependency();
        let dep = dep.width as u8 | (dep.height as u8) << 1;
        let leaf_box = LeafBox::from_style(&style);
        let id = match self.free.pop() {
            Some(id) => {
                self.styles[id as usize] = style;
                self.refs[id as usize] = 1;
                self.hashes[id as usize] = hash;
                self.chain[id as usize] = head;
                self.dependency[id as usize] = dep;
                self.leaf_boxes[id as usize] = leaf_box;
                id
            }
            None => {
                self.styles.push(style);
                self.refs.push(1);
                self.hashes.push(hash);
                self.chain.push(head);
                self.dependency.push(dep);
                self.leaf_boxes.push(leaf_box);
                self.styles.len() as u32 - 1
            }
        };
        self.heads.insert(hash, id);
        id
    }

    pub fn release(&mut self, id: u32) {
        let r = &mut self.refs[id as usize];
        *r -= 1;
        if *r != 0 {
            return;
        }
        let hash = self.hashes[id as usize];
        let next = self.chain[id as usize];
        let head = self.heads[&hash];
        if head == id {
            if next == NONE {
                self.heads.remove(&hash);
            } else {
                self.heads.insert(hash, next);
            }
        } else {
            let mut i = head;
            while self.chain[i as usize] != id {
                i = self.chain[i as usize];
            }
            self.chain[i as usize] = next;
        }
        self.free.push(id);
    }

    #[inline]
    pub fn get(&self, id: u32) -> &Style {
        &self.styles[id as usize]
    }

    #[inline]
    pub fn leaf_box(&self, id: u32) -> Option<LeafBox> {
        self.leaf_boxes[id as usize]
    }

    /// See [`Style::parent_size_dependency`], precomputed.
    #[inline]
    pub fn parent_size_dependency(&self, id: u32) -> Size<bool> {
        let d = self.dependency[id as usize];
        Size { width: d & 1 != 0, height: d & 2 != 0 }
    }

    pub fn len(&self) -> usize {
        self.styles.len() - self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Dimension;

    #[test]
    fn interns_equal_styles_once_and_frees_on_last_release() {
        let mut t = StyleTable::new();
        let a = t.intern(Style::DEFAULT);
        let b = t.intern(Style::DEFAULT);
        assert_eq!(a, b);
        let mut other = Style::DEFAULT;
        other.size.width = Dimension::length(3.0);
        let c = t.intern(other.clone());
        assert_ne!(a, c);
        t.release(a);
        assert_eq!(t.len(), 2);
        t.release(b);
        assert_eq!(t.len(), 1);
        assert_eq!(t.intern(other), c);
    }
}
