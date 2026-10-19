#!/bin/sh
# Regenerates tests/fixtures/<group>.xml from taffy's Chrome-generated XML fixtures at a release tag.
# Usage: scripts/update_fixtures.sh [tag]   (default v0.15.0)
set -e
tag=${1:-v0.15.0}
root=$(cd "$(dirname "$0")/.." && pwd)
src="$root/target/fixtures-src/taffy-$tag"
if [ ! -d "$src" ]; then
	mkdir -p "$root/target/fixtures-src"
	git clone -q --depth 1 --branch "$tag" https://github.com/DioxusLabs/taffy "$src"
fi
for dir in "$src"/tests/xml/*/; do
	group=$(basename "$dir")
	out="$root/tests/fixtures/$group.xml"
	{
		echo "<tests>"
		LC_ALL=C ls "$dir" | grep '\.xml$' | while read -r f; do cat "$dir/$f"; done
		echo "</tests>"
	} >"$out"
	echo "$group: $(grep -c '<test ' "$out") tests"
done
echo "taffy $tag $(git -C "$src" rev-parse HEAD)" >"$root/tests/fixtures/SOURCE"
