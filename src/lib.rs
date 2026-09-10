//! Retained, incremental CSS layout: block, flexbox and grid with spineless invalidation.
//!
//! Build a [`Tree`], give nodes a [`Style`], and call [`Tree::compute_layout`]. Later edits
//! recompute only what they can affect, and the result always equals a cold layout.
//!
//! ```
//! use cephal::{Tree, Style, Size, AvailableSpace};
//! use cephal::style::{Display, Dimension};
//!
//! let mut tree: Tree<()> = Tree::new();
//! let mut row = Style::DEFAULT;
//! row.display = Display::Flex;
//! row.size = Size { width: Dimension::length(300.0), height: Dimension::AUTO };
//! let mut item = Style::DEFAULT;
//! item.flex_grow = 1.0;
//! item.size.height = Dimension::length(40.0);
//!
//! let root = tree.new_leaf(row);
//! let a = tree.new_leaf(item.clone());
//! let b = tree.new_leaf(item);
//! tree.append_child(root, a);
//! tree.append_child(root, b);
//!
//! let viewport = Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::MaxContent };
//! tree.compute_layout(root, viewport);
//! assert_eq!(tree.layout(b).location.x, 150.0);
//!
//! tree.patch_style(a, |s| s.flex_grow = 3.0);
//! tree.compute_layout(root, viewport);
//! assert_eq!(tree.layout(b).location.x, 225.0);
//! assert_eq!(tree.changed().len(), 2);
//! ```
//!
//! [`Tree::compute_layout_parallel`] lays out wide subtrees on a [`Parallel`] pool of threads
//! with results identical to the serial call.
//!
//! Frameworks that own their tree implement [`tree::LayoutTree`], [`tree::CacheAccess`] and
//! [`schedule::Incremental`] and drive the algorithms in [`compute`] directly.
#![allow(clippy::too_many_arguments, clippy::self_named_constructors)]

pub mod compat;
pub mod compute;
pub mod geometry;
pub(crate) mod hash;
pub mod owned;
pub mod round;
pub mod schedule;
pub mod style;
pub mod tree;

pub use geometry::{AbsoluteAxis, AbstractAxis, AvailableSpace, Line, MaybeMath, Point, Rect, Size};
pub use owned::{Parallel, Tree};
pub use style::Style;
pub use tree::{
    Baselines, CacheAccess, DetailedGridInfo, Layout, LayoutInput, LayoutOutput, LayoutTree, MeasureInput,
    MeasureOutput, NodeCache, NodeId, RequestedAxis, RunMode, SizingMode,
};
