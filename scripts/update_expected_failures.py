#!/usr/bin/env python3
"""Regenerates tests/fixtures/expected_failures.txt from the fixtures cephal fails now.

    scripts/update_expected_failures.py

Runs every fixture (`CEPHAL_FIXTURES_ALL=1`), keeps the category of each fixture that was already
listed, and gives newly failing fixtures the category `new`, to be triaged. Prints what changed.
"""

import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LIST = ROOT / "tests" / "fixtures" / "expected_failures.txt"
HEADER = "# group/fixture category -- fixtures cephal does not pass yet, with their cause"


def main():
    old = {}
    if LIST.exists():
        for line in LIST.read_text().splitlines():
            if line and not line.startswith("#"):
                path, _, category = line.partition(" ")
                old[path] = category
    env = dict(os.environ, CEPHAL_FIXTURES_ALL="1")
    out = subprocess.run(["cargo", "test", "--release", "--test", "fixtures", "--", "--test-threads=9"], cwd=ROOT, env=env,
                         capture_output=True, text=True)
    text = out.stdout + out.stderr
    failing = []
    for m in re.finditer(r"thread '(\w+)'.*?ALL FAILING:\n(.*?)\n\n", text, re.S):
        group = m.group(1)
        failing += [f"{group}/{name.strip()}" for name in m.group(2).strip().splitlines() if name.strip()]
    if out.returncode != 0 and not failing:
        sys.exit("fixture run failed without fixture failures:\n" + text[-4000:])
    new = {path: old.get(path, "new") for path in failing}
    LIST.write_text("\n".join([HEADER] + [f"{p} {c}" for p, c in sorted(new.items())]) + "\n")
    passing = sorted(set(old) - set(new))
    added = sorted(set(new) - set(old))
    print(f"{len(new)} failing ({len(passing)} now pass, {len(added)} newly failing)")
    for p in passing:
        print(f"  pass  {p} ({old[p]})")
    for p in added:
        print(f"  NEW   {p}")


if __name__ == "__main__":
    main()
