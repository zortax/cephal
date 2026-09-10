//! The trait layer every algorithm is written against, plus the query cache.

pub mod cache;
mod input;
mod layout;
mod node;
mod traits;

pub use cache::NodeCache;
pub use input::{Baselines, CollapsibleMarginSet, LayoutInput, LayoutOutput, RequestedAxis, RunMode, SizingMode};
pub use layout::{DetailedGridInfo, DetailedGridItem, DetailedGridTracks, Layout};
pub use node::NodeId;
pub use crate::compute::reposition::{AbsoluteContext, AbsoluteGeometry};
pub use traits::{CacheAccess, ChildRequest, IdentSuffix, LayoutTree, MeasureInput, MeasureOutput, QueryMeta};
