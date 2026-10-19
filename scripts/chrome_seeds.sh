#!/bin/sh
# Lets Chrome adjudicate where cephal and taffy differ on the random trees of tests/differential.rs:
# writes the trees of the given seeds as gentest fixtures, renders them in Chrome for Testing and
# compares both engines with Chrome's layout.
#
#   scripts/chrome_seeds.sh [SEEDS]        # e.g. 721,940 or 1-300; default: differential_known.txt
#
# SEEDS may also be @FILE, a file of seeds. CEPHAL_VERBOSE=1 lists every box, not only the wrong
# ones. Fixtures go to target/diff_seeds/html, Chrome's layouts to target/diff_seeds/chrome.xml.
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
case "$1" in
	@*) CEPHAL_SEEDS=$(tr -s ' \n' ',' <"${1#@}" | sed 's/^,//; s/,$//'); export CEPHAL_SEEDS ;;
	?*) export CEPHAL_SEEDS="$1" ;;
esac
rm -rf target/diff_seeds/html
cargo test -q -j "${CARGO_BUILD_JOBS:-8}" --release --test differential to_html -- --ignored >/dev/null
CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-8}" scripts/gen_fixtures.sh target/diff_seeds/html target/diff_seeds/chrome.xml >/dev/null
cargo test -q -j "${CARGO_BUILD_JOBS:-8}" --release --test differential adjudicate -- --ignored --nocapture 2>/dev/null | grep -v '^running\|^test \|^$'
