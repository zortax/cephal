#!/bin/sh
# Builds the Yoga and Clay harnesses (benches/external) against pinned releases, dumps the suite's
# trees and times both engines on them, appending rows to the suite's CSV.
#
#   scripts/external.sh [suite arguments: filters, --sizes, --tag ...]
#
# CEPHAL_BENCH_CSV (default target/bench/suite.csv) receives the rows; CEPHAL_BENCH_SECS sets the
# sampling time; CLAY_FAST_MATH=1 builds Clay with -ffast-math as PanGui did; CEPHAL_CORE (default 2)
# is the core the engines are pinned to.
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
ext="$root/target/external"
yoga_tag=v3.2.1
yoga_sha=042f5013152eb81c1552dec945b88f7b95ca350f
clay_tag=v0.14
clay_sha=b25a31c1a152915cd7dd6796e6592273e5a10aac
fetch() { # name url tag sha
	dir="$ext/src/$1"
	[ -d "$dir" ] || git clone -q --depth 1 --branch "$3" "$2" "$dir"
	got=$(git -C "$dir" rev-parse HEAD)
	[ "$got" = "$4" ] || { echo "$1: expected $4, found $got" >&2; exit 1; }
}
fetch yoga https://github.com/facebook/yoga $yoga_tag $yoga_sha
fetch clay https://github.com/nicbarker/clay $clay_tag $clay_sha
fast=OFF
[ -n "$CLAY_FAST_MATH" ] && fast=ON
cmake -S "$root/benches/external" -B "$ext/build" -G Ninja -DCMAKE_BUILD_TYPE=Release \
	-DYOGA_SRC="$ext/src/yoga" -DCLAY_SRC="$ext/src/clay" -DCLAY_FAST_MATH=$fast >"$ext/cmake.log" 2>&1 || { cat "$ext/cmake.log"; exit 1; }
cmake --build "$ext/build" >"$ext/build.log" 2>&1 || { cat "$ext/build.log"; exit 1; }
trees="$ext/trees"
rm -rf "$trees"
CEPHAL_BENCH_DUMP="$trees" cargo bench -q --bench suite -- "$@"
csv=${CEPHAL_BENCH_CSV:-$root/target/bench/suite.csv}
[ -f "$csv" ] || echo "workload,family,tags,size,nodes,engine,mode,scenario,ns_per_node,median_ms,p10_ms,p90_ms,samples,agree,max_err,measure_calls" >"$csv"
secs=${CEPHAL_BENCH_SECS:-1}
rounding=
[ -n "$CEPHAL_NO_ROUNDING" ] && rounding=--no-rounding
core=${CEPHAL_CORE:-2}
taskset -c "$core" "$ext/build/yoga_bench" --secs "$secs" --csv "$csv" $rounding "$trees"/*.tree
taskset -c "$core" "$ext/build/clay_bench" --secs "$secs" --csv "$csv" "$trees"/*.tree
