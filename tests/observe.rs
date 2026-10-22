//! A host tree sees every answer the engine hands out, hidden ones excepted.

use cephal::compute::block::BlockContext;
use cephal::compute::compute_root_layout;
use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{Dimension, Display};
use cephal::tree::{AnswerSource, CacheAccess, Layout, LayoutInput, LayoutOutput, LayoutTree, NodeCache, NodeId, RunMode};
use cephal::Style;

struct Node {
    style: Style,
    children: Vec<NodeId>,
    cache: NodeCache,
    layout: Layout,
}

#[derive(Default)]
struct Host {
    nodes: Vec<Node>,
    cheap_leaves: bool,
    speculation: u32,
    seen: Vec<(NodeId, RunMode, AnswerSource, LayoutOutput)>,
}

impl Host {
    fn add(&mut self, style: Style, children: &[NodeId]) -> NodeId {
        self.nodes.push(Node { style, children: children.to_vec(), cache: NodeCache::default(), layout: Layout::ZERO });
        NodeId::from(self.nodes.len() - 1)
    }
    fn node(&self, id: NodeId) -> &Node {
        &self.nodes[usize::from(id)]
    }
}

impl LayoutTree for Host {
    type ChildIter<'a> = std::iter::Copied<std::slice::Iter<'a, NodeId>>;

    fn children(&self, node: NodeId) -> Self::ChildIter<'_> {
        self.node(node).children.iter().copied()
    }
    fn child_count(&self, node: NodeId) -> usize {
        self.node(node).children.len()
    }
    fn child_at(&self, node: NodeId, index: usize) -> NodeId {
        self.node(node).children[index]
    }
    fn style(&self, node: NodeId) -> &Style {
        &self.node(node).style
    }
    fn leaf_is_cheap(&self, _node: NodeId) -> bool {
        self.cheap_leaves
    }
    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        if self.speculation == 0 {
            self.nodes[usize::from(node)].layout = *layout;
        }
    }
    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput, block_ctx: Option<&mut BlockContext<'_>>) -> LayoutOutput {
        cephal::compute::compute_child_layout(self, node, inputs, block_ctx)
    }
    fn observe_answer(&mut self, node: NodeId, inputs: &LayoutInput, output: &LayoutOutput, source: AnswerSource) {
        self.seen.push((node, inputs.run_mode, source, *output));
    }
}

impl CacheAccess for Host {
    fn cache(&self, node: NodeId) -> &NodeCache {
        &self.node(node).cache
    }
    fn cache_mut(&mut self, node: NodeId) -> &mut NodeCache {
        &mut self.nodes[usize::from(node)].cache
    }
    fn generation(&self) -> u32 {
        1
    }
    fn speculation(&self) -> u32 {
        self.speculation
    }
    fn set_speculation(&mut self, depth: u32) {
        self.speculation = depth;
    }
}

const VIEWPORT: Size<AvailableSpace> = Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::MaxContent };

/// A flex row of two sized leaves and a hidden one.
fn row(host: &mut Host) -> (NodeId, [NodeId; 3]) {
    let mut leaf = Style::DEFAULT;
    leaf.size = Size { width: Dimension::length(40.0), height: Dimension::length(20.0) };
    let mut hidden = leaf.clone();
    hidden.display = Display::None;
    let a = host.add(leaf.clone(), &[]);
    let b = host.add(leaf, &[]);
    let h = host.add(hidden, &[]);
    let mut style = Style::DEFAULT;
    style.display = Display::Flex;
    (host.add(style, &[a, b, h]), [a, b, h])
}

#[test]
// Verification recomputes cache hits, which the hook observes too.
#[cfg_attr(feature = "verify-cache", ignore)]
fn every_answer_is_seen_once_computed_and_then_from_the_cache() {
    let mut host = Host::default();
    let (root, [a, b, hidden]) = row(&mut host);
    compute_root_layout(&mut host, root, VIEWPORT);
    for node in [root, a, b] {
        assert!(
            host.seen.iter().any(|&(n, mode, source, _)| n == node && mode == RunMode::PerformLayout && source == AnswerSource::Computed),
            "{node:?}: final answer computed"
        );
    }
    assert!(host.seen.iter().all(|&(n, ..)| n != hidden), "hidden answers are not observed");
    let computed = host.seen.clone();

    host.seen.clear();
    compute_root_layout(&mut host, root, VIEWPORT);
    let [(node, RunMode::PerformLayout, AnswerSource::Cached, output)] = host.seen[..] else {
        panic!("an unchanged root is served from its cache alone: {:?}", host.seen)
    };
    assert_eq!(node, root);
    assert!(computed.iter().any(|&(n, mode, _, o)| n == root && mode == RunMode::PerformLayout && o == output), "the cached answer is the computed one");
}

#[test]
fn cheap_leaves_are_reported_as_such() {
    let mut host = Host { cheap_leaves: true, ..Host::default() };
    let (root, [a, b, _]) = row(&mut host);
    compute_root_layout(&mut host, root, VIEWPORT);
    for node in [a, b] {
        assert!(host.seen.iter().filter(|&&(n, ..)| n == node).all(|&(_, _, source, _)| source == AnswerSource::CheapLeaf), "{node:?}");
    }
    assert!(host.seen.iter().any(|&(n, _, source, _)| n == root && source == AnswerSource::Computed));
}
