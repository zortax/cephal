#!/usr/bin/env python3
"""Plots the scaling CSV written by `cargo run --release --example scaling`.

    python3 scripts/plot_bench.py target/bench/scaling.csv --out target/bench/plots
"""

import argparse
import csv
from collections import defaultdict
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
from matplotlib.ticker import FuncFormatter, LogLocator, NullFormatter  # noqa: E402

ENGINES = [
    ("cephal", "spineless, serial", "#1f77b4", "o"),
    ("cephal_parallel", "spineless, parallel", "#2ca02c", "s"),
    ("taffy", "taffy", "#d62728", "^"),
]
# (scenario, shape, title)
SCENARIOS = [
    ("hover_list_cell", "list", "list: cell width changes, row height fixed"),
    ("typing_list_cell", "list", "list: cell content resizes, row height fixed"),
    ("animate_absolute", "random", "random: absolute box moves"),
    ("idle_frame", "random", "random: layout called, nothing changed"),
    ("hover_leaf_width", "random", "random: leaf width changes"),
    ("typing_leaf_text", "random", "random: leaf content resizes"),
    ("toggle_display", "random", "random: container display none/flex"),
    ("resize", "random", "random: viewport width changes"),
]
FRAME_BUDGET_S = 1 / 60
UNITS = [(1.0, "s"), (1e-3, "ms"), (1e-6, "µs"), (1e-9, "ns")]


def load(path):
    """rows[(shape, scenario, engine)] -> sorted [(nodes, seconds)]"""
    rows = defaultdict(list)
    with open(path) as f:
        for r in csv.DictReader(f):
            rows[(r["shape"], r["scenario"], r["engine"])].append((int(r["nodes"]), float(r["seconds"])))
    for v in rows.values():
        v.sort()
    return rows


def series(rows, shape, scenario, engine):
    pts = rows.get((shape, scenario, engine), [])
    return [n for n, _ in pts], [s for _, s in pts]


def unit_for(max_seconds):
    """The largest unit in which the biggest value is at least 1."""
    for scale, name in UNITS:
        if max_seconds >= scale:
            return scale, name
    return UNITS[-1]


def plain(v, _pos=None):
    return f"{v:g}"


def nodes_axis(ax):
    ax.set_xscale("log")
    ax.set_xlabel("nodes in tree")
    ax.xaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v / 1000:g}k" if v >= 1000 else f"{v:g}"))
    ax.xaxis.set_minor_formatter(NullFormatter())
    ax.grid(True, which="major", alpha=0.3)


def time_lines(ax, rows, shape, scenario, title):
    """Frame time vs nodes, linear from zero, in the unit that fits the largest value."""
    data = [(engine, label, color, marker, *series(rows, shape, scenario, engine)) for engine, label, color, marker in ENGINES]
    peak = max((max(y) for *_, y in data if y), default=1e-3)
    scale, unit = unit_for(peak)
    for engine, label, color, marker, x, y in data:
        if x:
            ax.plot(x, [v / scale for v in y], marker=marker, color=color, label=label)
    if FRAME_BUDGET_S / scale <= peak / scale * 1.3:
        ax.axhline(FRAME_BUDGET_S / scale, color="gray", linestyle="--", linewidth=1)
        ax.text(ax.get_xlim()[0], FRAME_BUDGET_S / scale, " 60 fps budget", va="bottom", color="gray", fontsize=8)
    ax.set_ylim(bottom=0)
    ax.set_ylabel(f"frame time ({unit})")
    ax.yaxis.set_major_formatter(FuncFormatter(plain))
    ax.set_title(title)
    nodes_axis(ax)
    ax.legend(fontsize=8)


def cold_plot(rows, out):
    shapes = [s for s in ("random", "nested") if any(k[0] == s and k[1] == "cold" for k in rows)]
    fig, axes = plt.subplots(1, len(shapes), figsize=(6.5 * len(shapes), 4.5), squeeze=False)
    for ax, shape in zip(axes[0], shapes):
        time_lines(ax, rows, shape, "cold", f"cold layout, {shape} tree")
    fig.tight_layout()
    fig.savefig(out / "cold_scaling.png", dpi=150)
    plt.close(fig)


def incremental_plot(rows, out):
    fig, axes = plt.subplots(2, 4, figsize=(21, 9))
    for ax, (scenario, shape, title) in zip(axes.flat, SCENARIOS):
        time_lines(ax, rows, shape, scenario, title)
    fig.suptitle("incremental frame time vs tree size (one edit per frame, averaged over 8 edit sites)")
    fig.tight_layout()
    fig.savefig(out / "incremental_scaling.png", dpi=150)
    plt.close(fig)


def speedup_plot(rows, out):
    """Speed-up over taffy per scenario; a flat line means the same scaling, a rising one better."""
    fig, axes = plt.subplots(2, 4, figsize=(21, 9))
    for ax, (scenario, shape, title) in zip(axes.flat, SCENARIOS):
        tx, ty = series(rows, shape, scenario, "taffy")
        taffy = dict(zip(tx, ty))
        for engine, label, color, marker in ENGINES[:2]:
            x, y = series(rows, shape, scenario, engine)
            pts = [(n, taffy[n] / s) for n, s in zip(x, y) if n in taffy]
            if pts:
                ax.plot([n for n, _ in pts], [r for _, r in pts], marker=marker, color=color, label=label)
        ax.axhline(1.0, color="gray", linestyle="--", linewidth=1)
        ax.set_yscale("log")
        ax.yaxis.set_major_locator(LogLocator(base=10, subs=(1.0, 2.0, 5.0)))
        ax.yaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v:g}×"))
        ax.yaxis.set_minor_formatter(NullFormatter())
        ax.set_ylabel("speed-up over taffy")
        ax.set_title(title)
        nodes_axis(ax)
        ax.legend(fontsize=8)
    fig.suptitle("speed-up over taffy vs tree size (above the dashed line is faster than taffy)")
    fig.tight_layout()
    fig.savefig(out / "speedup_vs_taffy.png", dpi=150)
    plt.close(fig)


def bars_plot(rows, out):
    """Frame times at the largest tree, one panel per scenario so each has a readable unit."""
    largest = max(n for (shape, _, _), pts in rows.items() if shape == "random" for n, _ in pts)
    names = SCENARIOS + [("cold", "random", "random: cold layout")]
    fig, axes = plt.subplots(1, len(names), figsize=(3.0 * len(names), 4.5))
    for ax, (scenario, shape, title) in zip(axes, names):
        vals = []
        for engine, label, color, _ in ENGINES:
            pts = rows.get((shape, scenario, engine), [])
            vals.append((label, color, pts[-1][1] if pts else 0.0))
        scale, unit = unit_for(max(v for _, _, v in vals))
        ax.bar(range(len(vals)), [v / scale for _, _, v in vals], color=[c for _, c, _ in vals])
        for i, (_, _, v) in enumerate(vals):
            own_scale, own_unit = unit_for(v)
            ax.text(i, v / scale, f"{v / own_scale:.3g} {own_unit}", ha="center", va="bottom", fontsize=8)
        ax.set_xticks(range(len(vals)))
        ax.set_xticklabels(["serial", "parallel", "taffy"], fontsize=8)
        ax.set_ylim(0, max(v for _, _, v in vals) / scale * 1.15)
        ax.set_ylabel(f"frame time ({unit})")
        ax.yaxis.set_major_formatter(FuncFormatter(plain))
        ax.set_title(title, fontsize=9)
        ax.grid(True, axis="y", alpha=0.3)
    fig.suptitle(f"frame time per scenario at about {largest:,} nodes (spineless serial, spineless parallel, taffy)")
    fig.tight_layout()
    fig.savefig(out / "frame_times_largest.png", dpi=150)
    plt.close(fig)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("csv", nargs="?", default="target/bench/scaling.csv")
    p.add_argument("--out", default="target/bench/plots")
    args = p.parse_args()
    rows = load(args.csv)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    cold_plot(rows, out)
    incremental_plot(rows, out)
    speedup_plot(rows, out)
    bars_plot(rows, out)
    for name in ("cold_scaling", "incremental_scaling", "speedup_vs_taffy", "frame_times_largest"):
        print(out / f"{name}.png")


if __name__ == "__main__":
    main()
