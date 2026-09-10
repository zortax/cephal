//! Node ids carry an index and a generation so freed slots cannot be aliased.

use crate::tree::NodeId;

/// Convenience alias for consumers.
pub type NodeHandle = NodeId;

#[inline]
pub(super) fn make(index: u32, generation: u16) -> NodeId {
    NodeId::new(((generation as u64) << 32) | index as u64)
}

#[inline]
pub(super) fn split(id: NodeId) -> (u32, u16) {
    let raw = id.raw();
    (raw as u32, (raw >> 32) as u16)
}
