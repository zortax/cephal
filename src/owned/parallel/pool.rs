//! A persistent work queue; nested batches share its threads and waiting workers help.

use super::worker::Worker;
use super::{Config, Group, Log, SyncMeasureFn};
use crate::owned::Tree;
use crate::tree::ChildRequest;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

type Job = Box<dyn FnOnce(&Pool) + Send>;

/// Polls before parking: roughly the time a nested batch takes to arrive.
const SPIN_ROUNDS: usize = 2000;

pub(super) struct Pool {
    jobs: Mutex<(VecDeque<Job>, bool)>,
    /// Jobs in the queue, readable without the lock.
    pending: AtomicUsize,
    closing: AtomicBool,
    changed: Condvar,
    /// Set when a job panicked, so waiters fail instead of waiting forever.
    poisoned: AtomicBool,
    distributed: AtomicUsize,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

/// Flags the pool when the job running on this thread unwinds.
struct PanicGuard<'p>(&'p Pool);

impl Drop for PanicGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.poisoned.store(true, Ordering::Release);
            let _guard = self.0.jobs.lock();
            self.0.changed.notify_all();
        }
    }
}

impl Pool {
    /// Starts `helpers` threads that serve the queue until the pool is dropped.
    pub fn start(helpers: usize) -> Arc<Self> {
        let pool = Arc::new(Self {
            jobs: Mutex::new((VecDeque::new(), false)),
            pending: AtomicUsize::new(0),
            closing: AtomicBool::new(false),
            changed: Condvar::new(),
            poisoned: AtomicBool::new(false),
            distributed: AtomicUsize::new(0),
            threads: Mutex::new(Vec::new()),
        });
        let threads = (0..helpers)
            .map(|_| {
                let pool = pool.clone();
                std::thread::spawn(move || pool.serve())
            })
            .collect();
        *pool.threads.lock().unwrap() = threads;
        pool
    }

    pub fn distributed(&self) -> usize {
        self.distributed.load(Ordering::Relaxed)
    }

    pub fn count_distributed(&self) {
        self.distributed.fetch_add(1, Ordering::Relaxed);
    }

    /// Queues `jobs` and runs jobs from the queue until `remaining` reaches zero.
    fn run_until_done(&self, jobs: Vec<Job>, remaining: &AtomicUsize) {
        {
            let mut queue = self.jobs.lock().unwrap();
            queue.0.extend(jobs);
            self.pending.fetch_add(queue.0.len(), Ordering::Release);
        }
        self.changed.notify_all();
        loop {
            let job = {
                let mut queue = self.jobs.lock().unwrap();
                let mut spun = false;
                loop {
                    if remaining.load(Ordering::Acquire) == 0 {
                        break None;
                    }
                    assert!(!self.poisoned.load(Ordering::Acquire), "layout worker panicked");
                    if let Some(job) = queue.0.pop_front() {
                        self.pending.fetch_sub(1, Ordering::Release);
                        break Some(job);
                    }
                    if !spun {
                        // Nested batches arrive within microseconds; spin once before sleeping.
                        drop(queue);
                        self.spin(|| remaining.load(Ordering::Acquire) == 0 || self.pending.load(Ordering::Acquire) > 0);
                        queue = self.jobs.lock().unwrap();
                        spun = true;
                        continue;
                    }
                    spun = false;
                    queue = self.changed.wait(queue).unwrap();
                }
            };
            match job {
                Some(job) => self.run(job),
                None => return,
            }
        }
    }

    /// Spins until `ready` or the spin budget is spent.
    fn spin(&self, ready: impl Fn() -> bool) {
        for i in 0..SPIN_ROUNDS {
            if ready() {
                return;
            }
            if i < 64 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
    }

    fn run(&self, job: Job) {
        let _guard = PanicGuard(self);
        job(self);
    }

    /// Wakes every waiter; called when a batch completes.
    fn notify(&self) {
        let _guard = self.jobs.lock().unwrap();
        self.changed.notify_all();
    }

    /// A helper thread's life: run jobs until the pool closes.
    fn serve(&self) {
        loop {
            let job = {
                let mut queue = self.jobs.lock().unwrap();
                let mut spun = false;
                loop {
                    if let Some(job) = queue.0.pop_front() {
                        self.pending.fetch_sub(1, Ordering::Release);
                        break Some(job);
                    }
                    if queue.1 || self.poisoned.load(Ordering::Acquire) {
                        break None;
                    }
                    if !spun {
                        drop(queue);
                        self.spin(|| self.pending.load(Ordering::Acquire) > 0 || self.closing.load(Ordering::Acquire));
                        queue = self.jobs.lock().unwrap();
                        spun = true;
                        continue;
                    }
                    spun = false;
                    queue = self.changed.wait(queue).unwrap();
                }
            };
            match job {
                Some(job) => self.run(job),
                None => return,
            }
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        // Only the handle in `Parallel` and finished jobs reference the pool; nothing waits.
        self.jobs.get_mut().unwrap().1 = true;
    }
}

/// Stops the helper threads; called by `Parallel::drop`.
pub(super) fn shutdown(pool: &Pool) {
    pool.closing.store(true, Ordering::Release);
    pool.jobs.lock().unwrap().1 = true;
    pool.changed.notify_all();
    let threads = std::mem::take(&mut *pool.threads.lock().unwrap());
    for t in threads {
        let _ = t.join();
    }
}

/// One batch in flight: its inputs, and a result slot per job.
struct Batch<Ctx, F> {
    tree: Arc<Tree<Ctx>>,
    measure: Arc<F>,
    config: Config,
    requests: Vec<ChildRequest>,
    groups: Vec<Group>,
    /// Groups per job; small groups are chunked so a job carries real work.
    chunk: usize,
    results: Mutex<Vec<Option<Log>>>,
    remaining: AtomicUsize,
}

impl<Ctx: Send + Sync + 'static, F: SyncMeasureFn<Ctx>> Batch<Ctx, F> {
    /// Runs job `index`: the groups `[index * chunk, (index + 1) * chunk)` in order, in one log.
    fn run_job(self: &Arc<Self>, index: usize, pool: &Pool) {
        let groups = &self.groups[index * self.chunk..((index + 1) * self.chunk).min(self.groups.len())];
        let estimate = groups.iter().map(|g| g.estimate).sum();
        let mut worker = Worker::new(&self.tree, &self.measure, self.config, pool, estimate);
        for group in groups {
            for &r in &group.requests {
                let req = self.requests[r];
                let output = crate::compute::compute_child_layout(&mut worker, req.node, req.input, None);
                worker.answered(r, output);
            }
        }
        let log = worker.into_log();
        self.results.lock().unwrap()[index] = Some(log);
        if self.remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
            pool.notify();
        }
    }
}

/// Runs a batch on the pool, the calling thread included, and returns its logs in group order.
pub(super) fn run_batch<Ctx: Send + Sync + 'static, F: SyncMeasureFn<Ctx>>(
    pool: &Pool,
    tree: Arc<Tree<Ctx>>,
    measure: Arc<F>,
    config: Config,
    requests: &[ChildRequest],
    groups: Vec<Group>,
) -> Vec<Log> {
    // Enough jobs to balance the threads, each carrying several small groups where possible.
    let chunk = groups.len().div_ceil(config.threads * 2).max(1);
    let jobs = groups.len().div_ceil(chunk);
    let batch = Arc::new(Batch {
        tree,
        measure,
        config,
        requests: requests.to_vec(),
        chunk,
        results: Mutex::new((0..jobs).map(|_| None).collect()),
        remaining: AtomicUsize::new(jobs),
        groups,
    });
    let jobs: Vec<Job> = (0..jobs)
        .map(|j| {
            let batch = batch.clone();
            Box::new(move |pool: &Pool| batch.run_job(j, pool)) as Job
        })
        .collect();
    pool.run_until_done(jobs, &batch.remaining);
    // The thread that finished last may still hold its job's handle for an instant.
    while Arc::strong_count(&batch) > 1 {
        std::thread::yield_now();
    }
    let results = std::mem::take(&mut *batch.results.lock().unwrap());
    results.into_iter().flatten().collect()
}
