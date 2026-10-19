#!/bin/sh
# User-space instructions per node of one cold layout (`clear_layout` included), per suite workload
# and size. Unlike wall time, insensitive to machine load.
# Usage: scripts/instructions.sh [--csv path] [suite arguments: filters, --sizes, --tag ...]
set -e
csv=
if [ "$1" = "--csv" ]; then
	csv=$2
	shift 2
	echo "workload,size,nodes,instructions_per_node" >"$csv"
fi
cargo bench -q --bench suite --no-run 2>/dev/null
bin=$(find target/release/build target/release/deps -name 'suite-*' -type f -executable 2>/dev/null | xargs ls -t | head -1)
# On hybrid CPUs, count on one performance core only; the other PMU's numbers are extrapolated.
event=instructions:u
core=${CEPHAL_CORE:-2}
[ -d /sys/bus/event_source/devices/cpu_core ] && event=cpu_core/instructions/u
CEPHAL_BENCH_ITERS=0 "$bin" "$@" | while read -r id nodes; do
	name=${id%@*}
	size=${id#*@}
	extra=$(( (2000000 + nodes - 1) / nodes ))
	a=$(CEPHAL_BENCH_ITERS=2 taskset -c "$core" perf stat -x, -e "$event" "$bin" --exact "$name" --sizes "$size" 2>&1 >/dev/null | grep instructions | cut -d, -f1)
	b=$(CEPHAL_BENCH_ITERS=$((2 + extra)) taskset -c "$core" perf stat -x, -e "$event" "$bin" --exact "$name" --sizes "$size" 2>&1 >/dev/null | grep instructions | cut -d, -f1)
	per=$(echo "$a $b $extra $nodes" | awk '{printf "%.0f", ($2 - $1) / $3 / $4}')
	printf "%-40s %8s %12s instructions/node\n" "$id" "$nodes" "$per"
	[ -n "$csv" ] && echo "$name,$size,$nodes,$per" >>"$csv"
done
