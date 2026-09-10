//! Sibling-list links.

use crate::tree::NodeId;

pub(super) const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
pub(super) struct Links {
    pub parent: u32,
    pub first_child: u32,
    pub last_child: u32,
    pub prev: u32,
    pub next: u32,
    pub child_count: u32,
    pub depth: u32,
}

impl Links {
    pub const fn detached() -> Self {
        Self { parent: NONE, first_child: NONE, last_child: NONE, prev: NONE, next: NONE, child_count: 0, depth: 0 }
    }
}

/// Iterates a node's children in order.
pub struct Children<'a> {
    pub(super) links: &'a [Links],
    pub(super) generations: &'a [u16],
    pub(super) next: u32,
}

impl Iterator for Children<'_> {
    type Item = NodeId;
    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        if self.next == NONE {
            return None;
        }
        let i = self.next;
        self.next = self.links[i as usize].next;
        Some(super::id::make(i, self.generations[i as usize]))
    }
}
