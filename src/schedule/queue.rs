//! Dirty nodes bucketed by depth; pops always come from the deepest bucket.

use crate::tree::NodeId;

#[derive(Default)]
pub(super) struct DepthQueue {
    buckets: Vec<Vec<NodeId>>,
    /// Upper bound on the deepest non-empty bucket.
    highest: usize,
    len: usize,
}

impl DepthQueue {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn push(&mut self, depth: u32, node: NodeId) {
        let depth = depth as usize;
        if depth >= self.buckets.len() {
            self.buckets.resize_with(depth + 1, Vec::new);
        }
        self.buckets[depth].push(node);
        self.highest = self.highest.max(depth);
        self.len += 1;
    }

    pub fn pop_deepest(&mut self) -> Option<(u32, NodeId)> {
        if self.len == 0 {
            return None;
        }
        loop {
            if let Some(node) = self.buckets[self.highest].pop() {
                self.len -= 1;
                return Some((self.highest as u32, node));
            }
            if self.highest == 0 {
                return None;
            }
            self.highest -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pops_deepest_first_even_after_shallow_pushes() {
        let mut q = DepthQueue::default();
        q.push(1, NodeId::new(1));
        q.push(3, NodeId::new(3));
        q.push(2, NodeId::new(2));
        assert_eq!(q.pop_deepest(), Some((3, NodeId::new(3))));
        q.push(5, NodeId::new(5));
        assert_eq!(q.pop_deepest(), Some((5, NodeId::new(5))));
        assert_eq!(q.pop_deepest(), Some((2, NodeId::new(2))));
        assert_eq!(q.pop_deepest(), Some((1, NodeId::new(1))));
        assert_eq!(q.pop_deepest(), None);
    }
}
