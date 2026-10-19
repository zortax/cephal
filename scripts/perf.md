# Profiling cephal

## Timing

`cargo bench --bench suite [-- <filter>...]` prints the median, p10 and p90 of every measurement
and writes `target/bench/suite.csv` (`CEPHAL_BENCH_CSV`) with `suite.meta.json` beside it: the
CPU, core affinity, governor, compiler and the cephal and taffy versions. Options:

- `--engines cephal,cephal_parallel,taffy`, `--modes cold,incremental,build`, `--sizes 1000,10000`,
  `--tag <tag>` (e.g. `grid`, `hit`, `miss:baseline`), `--exact` (filters match whole names), `--quick`,
  `--list`;
- `CEPHAL_BENCH_SECS`: sampling time per measurement (default 1);
- `CEPHAL_BENCH_READ=1`: also read every node's layout inside the timed region. Judge every
  optimisation with reads included, so work moved from layout into reads still counts;
- `CEPHAL_NO_ROUNDING=1`: no pixel snapping, in any engine.

On hybrid CPUs such as the i9-13900K, pin the run to one performance core, or results vary by
±20 %:

```sh
taskset -c 2 cargo bench --bench suite
```

The parallel engine uses the cores the process may run on, so pinned to one core it is serial.
Time it on the performance cores: `taskset -c 0-15 cargo bench --bench suite -- --engines
cephal_parallel`. Other processes on the same core or its hyperthread sibling still disturb timings;
compare runs made back to back, or compare instruction counts instead.

Every cold measurement also checks the engine's layout against cephal's (`agree`: `exact`,
`close` within 0.01 px, or `differs` with `max_err`) and counts measure calls in one untimed run.

## Other engines

`scripts/external.sh [suite arguments]` fetches pinned releases of Yoga and Clay into
`target/external`, builds `benches/external` (`-O3`, no `-march=native`, no `-ffast-math`;
`CLAY_FAST_MATH=1` reproduces PanGui's Clay flags), dumps the suite's trees with
`CEPHAL_BENCH_DUMP` and times both engines on them pinned to `CEPHAL_CORE`, appending to the
suite CSV.

- Yoga: every property is set to its CSS meaning, and lengths only where they differ from Yoga's
  defaults. Cold layout dirties every node untimed; `build` builds a fresh tree. Incremental
  scenarios use Yoga's own dirty propagation.
- Clay: immediate-mode, so a sample times declaring the elements and laying them out. Only trees
  Clay can express are run.
- Yoga has no automatic minimum size, and Clay does not shrink fixed sizes or weight growth, so
  some workloads lay out differently. The agreement column records that, and the plots do not
  count those workloads as comparisons.

## Plots

```sh
python3 scripts/plot_bench.py suite target/bench/suite.csv [more.csv ...] \
    [--instructions target/bench/instructions.csv] [--stats target/bench/stats.csv] \
    [--baseline earlier.csv] --out target/bench/plots
```

This writes `overview_heatmap`, `category_dumbbells`, `scaling`, `fastpath_pairs`,
`incremental_frames`, `measure_calls`, `win_loss`, `ipc_scatter`, `query_breakdown`, `noise`,
`agreement`, `suite_vs_baseline` and `report.md`. `plot_bench.py pangui target/bench/suite.csv`
draws the PanGui workloads against PanGui's published numbers.

## Instruction counts

`scripts/instructions.sh [--csv path] [suite arguments]` counts user-space instructions per node
of one cold layout (`clear_layout` included). It runs the suite at two fixed iteration counts and
takes the difference, so tree construction drops out. The result is reproducible to the
instruction and insensitive to machine load, which makes it the right tool for small changes. On
hybrid CPUs it counts on one performance core only (`CEPHAL_CORE`, default 2), because the other
PMU's counts are extrapolated.

## Where the time goes

The `bench` profile keeps line tables and does not use LTO, so phase functions keep their own
symbols:

```sh
cargo bench --bench suite --no-run
bin=$(find target/release -name 'suite-*' -type f -executable | xargs ls -t | head -1)
CEPHAL_BENCH_ITERS=3000 taskset -c 2 perf record -e cpu_core/instructions/u -c 5000 "$bin" --exact row_fixed --sizes 1000
perf report --no-children --sort sym
perf annotate --stdio -l -s '<symbol>'
```

Sampling `instructions` attributes work; sampling `cycles` (the default event) attributes stalls,
which on wide trees are dominated by cache misses on per-node data. Add `/upp` to an event for
precise attribution.

## Query counts

`cargo bench --bench suite --features stats -- <filter>` prints, per workload, the queries a cold
layout asks, and appends them to `target/bench/stats.csv` (`CEPHAL_BENCH_STATS`):

- cache hits and misses by run mode;
- cheap-leaf computations and measure calls;
- container runs by requested axis;
- `collision`: size misses whose answer equals one already cached for the node under another key.
  Collisions are what canonicalising questions (`compute::normalize`, observed min/max-content
  axes) could still save.
- `flex_fused` and `flex_not_fused`: flex container runs that took the fused path
  (`src/compute/flex/fused.rs`) and those that needed the general algorithm, with the reasons
  `flex_miss_*`: `wrap` (`wrap-reverse` or `balance`), `no_plan` (an item's lengths are relative),
  `aspect_ratio`, `auto_margin`, `basis` (`content`, keyword or relative), `size_keyword`,
  `baseline` and `empty`; `flex_wrap` counts the multi-line runs, fused or not.

## Fast paths

`CEPHAL_GENERIC_FLEX=1` (read once per process) turns the fused flex path off, so its effect can be
measured; `tests/fast_paths.rs` compares both paths bit for bit.

## Checking shortcuts

`cargo test --release --features verify-cache` recomputes every cache hit whose stored question
differs from the one asked and asserts the answers are bit-identical. It also makes the axis a
single-axis question did not ask for NaN, so that any caller reading it fails the tests.
`tests/observe.rs` is skipped under the feature, since verification observes extra answers.

## Fixtures

`scripts/update_fixtures.sh [tag]` regenerates `tests/fixtures/*.xml` from taffy's Chrome-generated
fixtures at a release tag. Fixtures cephal does not pass yet are listed with their cause in
`tests/fixtures/expected_failures.txt`. A listed fixture that starts passing fails the run, so
the list only shrinks on purpose. `CEPHAL_FIXTURES_ALL=1` runs them all as ordinary fixtures.
`tests/differential.rs` keeps the same kind of list for random trees in `differential_known.txt`;
its `census` test regenerates it, attributing each seed to the feature whose removal makes cephal
and taffy agree.
