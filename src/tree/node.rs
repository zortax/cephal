/// Identifies a node in any tree implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct NodeId(u64);

impl NodeId {
    #[inline]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }
    #[inline]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl From<usize> for NodeId {
    #[inline]
    fn from(v: usize) -> Self {
        Self(v as u64)
    }
}
impl From<u64> for NodeId {
    #[inline]
    fn from(v: u64) -> Self {
        Self(v)
    }
}
impl From<NodeId> for usize {
    #[inline]
    fn from(v: NodeId) -> usize {
        v.0 as usize
    }
}
impl From<NodeId> for u64 {
    #[inline]
    fn from(v: NodeId) -> u64 {
        v.0
    }
}
