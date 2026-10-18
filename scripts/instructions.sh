#!/bin/sh
# User-space instructions per node of one cold layout (`clear_layout` included), per pangui
# workload. Unlike wall time, insensitive to machine load. Usage: scripts/instructions.sh [filter...]
set -e
cargo bench -q --bench pangui --no-run 2>/dev/null
bin=$(find target/release/build target/release/deps -name 'pangui-*' -type f -executable 2>/dev/null | xargs ls -t | head -1)
# On hybrid CPUs, count on one performance core only; the other PMU's numbers are extrapolated.
event=instructions:u
core=${CEPHAL_CORE:-2}
[ -d /sys/bus/event_source/devices/cpu_core ] && event=cpu_core/instructions/u
count() {
	CEPHAL_BENCH_ITERS=$2 taskset -c "$core" perf stat -x, -e "$event" "$bin" "$1" 2>&1 >/dev/null | grep instructions | cut -d, -f1
}
CEPHAL_BENCH_ITERS=0 "$bin" "$@" | while read -r name nodes _; do
	extra=$(( (2000000 + nodes - 1) / nodes ))
	a=$(count "$name" 2)
	b=$(count "$name" $((2 + extra)))
	echo "$name $nodes $a $b $extra" | awk '{printf "%-34s %8.0f instructions/node\n", $1, ($4 - $3) / $5 / $2}'
done
