# Profiling cephal

## Timing

`cargo bench --bench pangui [-- <filter>]` prints the median nanoseconds per node of one cold
layout per workload (see the bench's doc comment for the variables it reads). On hybrid CPUs such
as the i9-13900K, pin the run to one performance core, or results vary by ±20 %:

```sh
taskset -c 2 cargo bench --bench pangui
```

Other processes on the same core or its hyperthread sibling still disturb it; compare runs made
back to back, or compare instruction counts instead.

## Instruction counts

`scripts/instructions.sh [filter...]` counts user-space instructions per node of one cold layout
(`clear_layout` included). It runs the bench at two fixed iteration counts and takes the
difference, so tree construction drops out; the result is reproducible to the instruction and
insensitive to machine load, which makes it the right tool for small changes. On hybrid CPUs it
counts on one performance core only (`CEPHAL_CORE`, default 2), because the other PMU's counts are
extrapolated.

## Where the time goes

The `bench` profile keeps line tables and does not use LTO, so phase functions keep their own
symbols:

```sh
cargo bench --bench pangui --no-run
bin=$(find target/release -name 'pangui-*' -type f -executable | xargs ls -t | head -1)
CEPHAL_BENCH_ITERS=3000 taskset -c 2 perf record -e cpu_core/instructions/u -c 5000 "$bin" wide_no_wrap_simple_few
perf report --no-children --sort sym
perf annotate --stdio -l -s '<symbol>'
```

Sampling `instructions` attributes work; sampling `cycles` (the default event) attributes stalls,
which on wide trees are dominated by cache misses on per-node data.

## Query counts

`cargo bench --bench pangui --features stats -- <filter>` prints, per workload, the queries a cold
layout asks: cache hits and misses by run mode, cheap-leaf computations, measure calls, container
runs by requested axis, and `collision`, the size misses whose answer equals one already cached
for the node under another key. Collisions are what canonicalising questions
(`compute::normalize`, observed min/max-content axes) could still save. `flex_fused` and
`flex_not_fused` count the single-line flex container runs that took the fused path
(`src/compute/flex/fused.rs`) and those whose items needed the general algorithm.

## Fast paths

`CEPHAL_GENERIC_FLEX=1` (read once per process) turns the fused flex path off, so its effect can be
measured; `tests/fast_paths.rs` compares both paths bit for bit.

## Checking shortcuts

`cargo test --release --features verify-cache` recomputes every cache hit whose stored question
differs from the one asked and asserts the answers are bit-identical, and makes the axis a
single-axis question did not ask for NaN so that any caller reading it fails the tests.
`tests/observe.rs` is skipped under the feature, since verification observes extra answers.
