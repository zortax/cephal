//! Answers independent child questions on a persistent pool of threads.
//!
//! During a parallel frame the tree is shared through an `Arc`; workers read it, write into
//! private logs, and the caller commits the logs in request order once it is the sole owner
//! again. Results equal the serial computation bit for bit, cache contents included.

mod pool;
mod worker;

use super::Tree;
use crate::hash::FxHashMap;
use super::node::NONE;
use crate::style::Style;
use crate::tree::{
    AbsoluteContext, ChildRequest, DetailedGridInfo, Layout, LayoutOutput, LayoutTree, MeasureInput, MeasureOutput, NodeId,
};
use pool::Pool;
use std::sync::Arc;

/// Measures a leaf from shared state; required to distribute measured leaves.
pub trait SyncMeasureFn<Ctx>: Fn(&Ctx, MeasureInput, &Style) -> MeasureOutput + Send + Sync + 'static {}
impl<Ctx, F: Fn(&Ctx, MeasureInput, &Style) -> MeasureOutput + Send + Sync + 'static> SyncMeasureFn<Ctx> for F {}

/// How the layout call reaches its tree: directly, or through the `Arc` a parallel frame shares.
pub trait TreeSlot<Ctx> {
    fn get(&self) -> &Tree<Ctx>;
    fn get_mut(&mut self) -> &mut Tree<Ctx>;
    /// A handle workers may hold for the duration of a batch.
    fn shared(&self) -> Option<Arc<Tree<Ctx>>>;
}

impl<Ctx> TreeSlot<Ctx> for Tree<Ctx> {
    #[inline]
    fn get(&self) -> &Tree<Ctx> {
        self
    }
    #[inline]
    fn get_mut(&mut self) -> &mut Tree<Ctx> {
        self
    }
    fn shared(&self) -> Option<Arc<Tree<Ctx>>> {
        None
    }
}

impl<Ctx, A: TreeSlot<Ctx>> TreeSlot<Ctx> for &mut A {
    #[inline]
    fn get(&self) -> &Tree<Ctx> {
        (**self).get()
    }
    #[inline]
    fn get_mut(&mut self) -> &mut Tree<Ctx> {
        (**self).get_mut()
    }
    fn shared(&self) -> Option<Arc<Tree<Ctx>>> {
        (**self).shared()
    }
}

impl<Ctx> TreeSlot<Ctx> for Arc<Tree<Ctx>> {
    #[inline]
    fn get(&self) -> &Tree<Ctx> {
        self
    }
    #[inline]
    fn get_mut(&mut self) -> &mut Tree<Ctx> {
        Arc::get_mut(self).expect("no worker holds the tree between batches")
    }
    fn shared(&self) -> Option<Arc<Tree<Ctx>>> {
        Some(Arc::clone(self))
    }
}

/// Runs batches of independent child questions.
pub trait Executor<Ctx> {
    /// Answers `requests` ahead of the caller, returning logs to commit; `None` leaves all to the caller.
    fn run(&self, tree: &dyn TreeSlot<Ctx>, requests: &[ChildRequest]) -> Option<Vec<Log>>;

    /// Whether batches can run faster than the caller's own loop; gates work done only to batch.
    fn distributes(&self) -> bool {
        false
    }
}

/// Answers every question on the calling thread.
pub struct Inline;

impl<Ctx> Executor<Ctx> for Inline {
    #[inline]
    fn run(&self, _: &dyn TreeSlot<Ctx>, _: &[ChildRequest]) -> Option<Vec<Log>> {
        None
    }
}

/// Thresholds below which a batch stays on the calling thread.
#[derive(Clone, Copy)]
pub(super) struct Config {
    /// Threads in total, the caller included.
    pub threads: usize,
    /// Fewest cold requests worth distributing.
    pub min_requests: usize,
    /// Fewest estimated nodes to lay out before a batch is distributed.
    pub min_work: usize,
}

/// Distributes cold questions over a pool of threads once a batch is worth it.
///
/// Batches issued by workers deeper in the same subtree go into the same queue; a worker waiting
/// on its own batch runs other jobs meanwhile.
pub struct Parallel<F> {
    measure: Arc<F>,
    pool: Arc<Pool>,
    threads: usize,
    config: Config,
}

impl<F> Parallel<F> {
    /// A pool with one thread per available core, the caller's included.
    pub fn new(measure: F) -> Self {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        Self::with_threads(measure, threads)
    }

    /// A pool of `threads` threads in total, the caller's included.
    pub fn with_threads(measure: F, threads: usize) -> Self {
        let pool = Pool::start(threads.saturating_sub(1));
        Self { measure: Arc::new(measure), pool, threads, config: Config { threads, min_requests: 4, min_work: 512 } }
    }

    pub fn threads(&self) -> usize {
        self.threads
    }

    /// The measure function, shared with the workers.
    pub(super) fn measure_fn(&self) -> Arc<F> {
        self.measure.clone()
    }

    /// Fewest cold requests worth distributing.
    pub fn min_requests(&mut self, n: usize) -> &mut Self {
        self.config.min_requests = n;
        self
    }

    /// Fewest estimated nodes to lay out before a batch is distributed.
    pub fn min_work(&mut self, n: usize) -> &mut Self {
        self.config.min_work = n;
        self
    }

    /// Batches handed to worker threads so far.
    pub fn distributed(&self) -> usize {
        self.pool.distributed()
    }
}

impl<F> Drop for Parallel<F> {
    fn drop(&mut self) {
        pool::shutdown(&self.pool);
    }
}

impl<Ctx: Send + Sync + 'static, F: SyncMeasureFn<Ctx>> Executor<Ctx> for Parallel<F> {
    fn distributes(&self) -> bool {
        self.threads >= 2
    }

    fn run(&self, tree: &dyn TreeSlot<Ctx>, requests: &[ChildRequest]) -> Option<Vec<Log>> {
        if self.threads < 2 {
            return None;
        }
        let groups = plan(&self.pool, self.config, tree.get(), requests)?;
        let tree = tree.shared()?;
        #[cfg(feature = "stats")]
        let start = std::time::Instant::now();
        let logs = pool::run_batch(&self.pool, tree, self.measure.clone(), self.config, requests, groups);
        #[cfg(feature = "stats")]
        {
            use std::sync::atomic::Ordering::Relaxed;
            crate::compute::stats::BATCHES.fetch_add(1, Relaxed);
            crate::compute::stats::BATCH_NANOS.fetch_add(start.elapsed().as_micros() as usize, Relaxed);
            crate::compute::stats::BATCH_REQUESTS.fetch_add(requests.len(), Relaxed);
        }
        Some(logs)
    }
}

/// Groups a batch when it is worth distributing.
fn plan<Ctx>(pool: &Pool, config: Config, tree: &Tree<Ctx>, requests: &[ChildRequest]) -> Option<Vec<Group>> {
    if requests.len() < config.min_requests {
        #[cfg(feature = "stats")]
        crate::compute::stats::bump(&crate::compute::stats::PLAN_FEW_REQUESTS);
        return None;
    }
    let mut groups = cold_groups(tree, requests);
    if groups.len() < config.min_requests.max(2) {
        #[cfg(feature = "stats")]
        crate::compute::stats::bump(&crate::compute::stats::PLAN_FEW_GROUPS);
        return None;
    }
    let mut work = 0;
    for g in &mut groups {
        g.estimate = tree.subtree_estimate(g.node);
        work += g.estimate;
    }
    if work < config.min_work {
        #[cfg(feature = "stats")]
        crate::compute::stats::bump(&crate::compute::stats::PLAN_LITTLE_WORK);
        return None;
    }
    pool.count_distributed();
    Some(groups)
}

/// All requests of one child with at least one cold answer, replayed in order by one worker so
/// its cache sees the same hits and stores as the serial run.
pub(super) struct Group {
    node: NodeId,
    requests: Vec<usize>,
    cold: bool,
    /// Lower bound on the nodes a worker will touch; sizes its log.
    estimate: usize,
}

fn cold_groups<Ctx>(tree: &Tree<Ctx>, requests: &[ChildRequest]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut by_node: FxHashMap<u32, usize> = FxHashMap::default();
    // A warm frame is the common case; probing every request of a wide container would cost
    // more than the batch could save, so give up once the first few are all cached.
    const WARM_SAMPLE: usize = 16;
    let mut probed = 0;
    for (r, req) in requests.iter().enumerate() {
        let i = tree.index(req.node);
        let cheap_leaf = tree.links[i].child_count == 0 && tree.context_slot[i] == NONE;
        if cheap_leaf || tree.style(req.node).generates_no_box() {
            continue;
        }
        let mut input = req.input;
        crate::compute::normalize(&mut input, tree.query_meta(req.node));
        let cold = !tree.caches[i].holds(&input);
        probed += 1;
        if probed == WARM_SAMPLE && groups.iter().all(|g| !g.cold) && !cold {
            return Vec::new();
        }
        match by_node.get(&(i as u32)) {
            Some(&g) => {
                groups[g].requests.push(r);
                groups[g].cold |= cold;
            }
            None => {
                by_node.insert(i as u32, groups.len());
                groups.push(Group { node: req.node, requests: vec![r], cold, estimate: 0 });
            }
        }
    }
    groups.retain(|g| g.cold);
    groups
}

/// What a worker could not write into the tree directly.
#[derive(Default)]
pub struct Log {
    /// Nodes whose layout the worker rewrote, with the layout before the frame.
    pub(super) touched: Vec<(NodeId, Layout)>,
    pub(super) detailed: Vec<(u32, DetailedGridInfo)>,
    pub(super) absolute: Vec<(u32, AbsoluteContext)>,
    /// Outputs of the batch's requests, by request index; the caller must use these rather than
    /// ask the cache again, since a later store in the same group may have evicted them.
    pub(super) answers: Vec<(usize, LayoutOutput)>,
}

impl Log {
    fn detailed(&self, i: u32) -> Option<DetailedGridInfo> {
        self.detailed.iter().rev().find(|(n, _)| *n == i).map(|(_, d)| d.clone())
    }
    fn absolute(&self, i: u32) -> Option<AbsoluteContext> {
        self.absolute.iter().rev().find(|(n, _)| *n == i).map(|(_, c)| c.clone())
    }

    /// Folds a nested batch's results in, as if this worker had computed them.
    fn absorb(&mut self, other: Log) {
        self.touched.extend(other.touched);
        self.detailed.extend(other.detailed);
        self.absolute.extend(other.absolute);
    }
}

/// Answers a batch: worker answers where present, the caller's own computation elsewhere.
pub(super) fn finish_batch<T: LayoutTree + ?Sized>(
    tree: &mut T,
    requests: &[ChildRequest],
    logs: Option<Vec<Log>>,
    mut commit: impl FnMut(&mut T, Log),
    out: &mut Vec<LayoutOutput>,
) {
    out.clear();
    let mut answered: Vec<Option<LayoutOutput>> = Vec::new();
    if let Some(logs) = logs {
        answered.resize(requests.len(), None);
        for log in logs {
            for &(r, output) in &log.answers {
                answered[r] = Some(output);
            }
            commit(tree, log);
        }
    }
    for (r, req) in requests.iter().enumerate() {
        let output = match answered.get(r).copied().flatten() {
            Some(output) => output,
            None => tree.compute_child_layout(req.node, req.input, None),
        };
        out.push(output);
    }
}

impl<Ctx> Tree<Ctx> {
    /// Applies what a worker could not write directly.
    pub(super) fn commit(&mut self, log: Log) {
        self.touched.extend(log.touched);
        for (i, info) in log.detailed {
            self.detailed_grid.insert(i, info);
        }
        for (i, ctx) in log.absolute {
            self.absolute_contexts.insert(i, ctx);
        }
    }

    /// A lower bound on the nodes under `node`: itself, its children and grandchildren.
    fn subtree_estimate(&self, node: NodeId) -> usize {
        let mut count = 1 + self.child_count(node);
        for child in self.children(node) {
            count += self.links[self.index(child)].child_count as usize;
        }
        count
    }
}
