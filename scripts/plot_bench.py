#!/usr/bin/env python3
"""Plots benchmark results.

The comparative suite (`cargo bench --bench suite`, plus `scripts/external.sh` for Yoga and Clay):

    python3 scripts/plot_bench.py suite target/bench/suite.csv [more.csv ...] \
        [--instructions target/bench/instructions.csv] [--stats target/bench/stats.csv] \
        [--baseline earlier.csv] --out target/bench/plots

The scaling CSV written by `cargo run --release --example scaling`:

    python3 scripts/plot_bench.py target/bench/scaling.csv --out target/bench/plots

With `pangui`, plots the PanGui workloads of a suite CSV against
the numbers published with PanGui's layout benchmarks (see `PUBLISHED`):

    python3 scripts/plot_bench.py pangui target/bench/suite.csv [more.csv ...] \
        [--baseline target/bench/pangui-main.csv] --out target/bench/plots
"""

import argparse
import csv
import sys
from collections import defaultdict
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
from matplotlib.lines import Line2D  # noqa: E402
from matplotlib.ticker import FixedLocator, FuncFormatter, LogLocator, NullFormatter  # noqa: E402

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


# PanGui's layout benchmarks, ns/node as published in
# https://www.pangui.io/blog/05-layout-rework-and-benchmarks/ (Ryzen 7 7800X3D; mean per iteration
# after 100 warm-ups and >= 5 s of sampling; Clay and Yoga GCC 13.1 -O3, Clay with -ffast-math;
# Taffy Rust 1.92, opt-level 3, LTO, one codegen unit; PanGui .NET Release). None: unsupported,
# incorrect (Yoga's min constraint) or over Clay's uint16 element limit.
PUBLISHED_MACHINE = "Ryzen 7 7800X3D"
PUBLISHED_ENGINES = ["PanGui", "Clay", "Taffy", "Yoga"]
# workload: (nodes, PanGui, Clay, Taffy, Yoga)
PUBLISHED = {
    "expand_with_max_constraint": (3001, 69.21, None, 540.55, 551.78),
    "expand_with_min_constraint": (3001, 68.64, None, 538.29, None),
    "fit_nesting": (101111, 65.63, 39.57, 1097.56, 593.79),
    "flex_expand_equal_weights": (15001, 71.06, 41.46, 358.16, 323.03),
    "flex_expand_weights": (15001, 69.36, None, 501.77, 385.09),
    "nested_vertical_stack": (10001, 64.60, 35.51, 378.63, 292.11),
    "padding_and_margin": (101, 85.15, None, 147.52, 328.71),
    "percentage_and_ratio": (10001, 43.98, None, 331.30, 296.19),
    "perpendicular_expand_with_wrap": (12001, 70.83, None, 378.39, 4896.04),
    # min-width: Expand has no CSS equivalent; the pangui bench leaves it out.
    "pixels_with_min_expand_constraint": (30001, 81.41, None, None, None),
    "wide_no_wrap_simple_few": (1001, 41.16, 34.27, 250.45, 191.91),
    "wide_no_wrap_simple_many": (100001, 46.86, None, 486.87, 285.49),
    "wide_wrapping": (10001, 56.71, None, 382.35, 1652.73),
}
# Colours follow the engine, not the machine: measured taffy is filled, published taffy hollow, in
# the same red. Markers repeat the identity for readers who cannot tell the hues apart.
STYLE = {
    "cephal": ("#2a78d6", "o"),
    "cephal_min0": ("#1baf7a", "D"),
    "taffy": ("#e34948", "^"),
    "taffy_min0": ("#e87ba4", "v"),
    "PanGui": ("#eb6834", "s"),
    "Clay": ("#eda100", "P"),
    "Taffy": ("#e34948", "^"),
    "Yoga": ("#4a3aa7", "X"),
    "yoga": ("#4a3aa7", "X"),
    "clay": ("#eda100", "P"),
}
FASTER, SLOWER = "#2a78d6", "#e34948"
HERE = "measured here"
THERE = f"published, {PUBLISHED_MACHINE}"


def load_pangui(paths):
    """rows[(workload, engine)] -> (nodes, ns_per_node) of cold layouts; later files override earlier ones."""
    rows = {}
    for path in paths:
        with open(path) as f:
            for r in csv.DictReader(f):
                if r.get("mode", "cold") != "cold" or r["engine"] == "cephal_parallel":
                    continue
                rows[(r["workload"], r["engine"])] = (int(r["nodes"]), float(r["ns_per_node"]))
    return rows


def engines_of(rows):
    """Measured engines in a fixed order: cephal before taffy, plain before `_no_rounding`."""
    return sorted({e for _, e in rows}, key=lambda e: (not e.startswith("cephal"), e.endswith("_no_rounding"), e))


def series_style(engine, min0=False):
    """(colour, marker, label) of a measured engine, optionally on the `_min0` variants."""
    plain = engine.removesuffix("_no_rounding")
    color, marker = STYLE.get(plain + ("_min0" if min0 else ""), STYLE.get(plain, ("#52514e", "o")))
    label = plain + (", min-size 0" if min0 else "")
    if plain != engine:
        label += ", no rounding"
        marker = {"o": "h", "D": "d", "^": ">", "v": "<"}.get(marker, marker)
    return color, marker, label


def ratio_axis(ax, lo, hi, label):
    """Log x axis for ratios around 1, with plain `n×` ticks."""
    lo, hi = min(lo, 0.9) / 1.35, max(hi, 1.1) * 1.6
    ax.set_xscale("log")
    ax.set_xlim(lo, hi)
    ax.xaxis.set_major_locator(FixedLocator([t for t in (0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 100) if lo <= t <= hi]))
    ax.xaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v:g}×"))
    ax.xaxis.set_minor_formatter(NullFormatter())
    ax.axvline(1.0, color="gray", linewidth=1)
    ax.set_xlabel(label)
    ax.grid(True, axis="x", which="major", alpha=0.3)


def ns_axis(ax, values):
    lo, hi = min(values) / 1.4, max(values) * 1.4
    ax.set_xscale("log")
    ax.set_xlim(lo, hi)
    ax.xaxis.set_major_locator(FixedLocator([t for t in (10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000) if lo <= t <= hi]))
    ax.xaxis.set_major_formatter(FuncFormatter(plain))
    ax.xaxis.set_minor_formatter(NullFormatter())
    ax.set_xlabel("ns per node, cold layout (log scale, lower is better)")
    ax.grid(True, axis="x", which="major", alpha=0.3)


def row_labels(ax, labels):
    """One row per label, top to bottom, with alternate rows shaded."""
    ax.set_yticks(range(len(labels)))
    ax.set_yticklabels(labels, fontsize=9)
    ax.set_ylim(len(labels) - 0.5, -0.5)
    for y in range(1, len(labels), 2):
        ax.axhspan(y - 0.5, y + 0.5, color="#f0efec", zorder=0, linewidth=0)


def dots(ax, rows_of, series, spread):
    """One marker per (series, row): filled for measured here, hollow for published."""
    offsets = [(i - (len(series) - 1) / 2) * spread for i in range(len(series))]
    for (_, color, marker, filled, vals), dy in zip(series, offsets):
        ax.scatter(list(vals.values()), [rows_of.index(w) + dy for w in vals], s=46, marker=marker, zorder=3,
                   linewidths=1.6, facecolors=color if filled else "white", edgecolors=color)
    return offsets


def handles(series, label=lambda lab, filled: lab):
    return [
        Line2D([], [], linestyle="none", marker=m, markersize=7, markeredgewidth=1.6, color=c,
               markerfacecolor=c if f else "white", label=label(lab, f))
        for lab, c, m, f, _ in series
    ]


def pangui_ns_plot(rows, engines, out):
    """ns/node per PanGui workload: engines measured here (filled) and published (hollow)."""
    workloads = [w for w in PUBLISHED if any((w, e) in rows for e in engines)]
    if not workloads:
        return None
    first = engines[0]
    workloads.sort(key=lambda w: rows.get((w, first), (0, float("inf")))[1])
    # (label, colour, marker, filled, {workload: ns/node})
    series = []
    for engine in engines:
        color, marker, label = series_style(engine)
        series.append((label, color, marker, True, {w: rows[(w, engine)][1] for w in workloads if (w, engine) in rows}))
        if engine.startswith("cephal"):
            vals = {w: rows[(w + "_min0", engine)][1] for w in workloads if (w + "_min0", engine) in rows}
            if vals:
                color, marker, label = series_style(engine, min0=True)
                series.append((label, color, marker, True, vals))
    for i, name in enumerate(PUBLISHED_ENGINES):
        vals = {w: PUBLISHED[w][1 + i] for w in workloads if PUBLISHED[w][1 + i]}
        series.append((name, *STYLE[name], False, vals))

    fig, ax = plt.subplots(figsize=(11.5, 0.55 * len(workloads) + 1.8))
    row_labels(ax, [f"{w}  ({PUBLISHED[w][0]:,})" for w in workloads])
    offsets = dots(ax, workloads, series, 0.08)
    # Direct labels on the first measured series only, the one the chart is about.
    for w, v in series[0][4].items():
        ax.annotate(f"{v:.0f}", (v, workloads.index(w) + offsets[0]), xytext=(-7, 0), textcoords="offset points",
                    ha="right", va="center", fontsize=7, color="#52514e")
    ns_axis(ax, [v for *_, vals in series for v in vals.values()])
    measured = [s for s in series if s[3]]
    published = [s for s in series if not s[3]]
    legend = ax.legend(handles=handles(measured), title=f"{HERE} (median)", loc="upper left",
                       bbox_to_anchor=(1.01, 1.0), fontsize=8, title_fontsize=8, alignment="left")
    ax.add_artist(legend)
    ax.legend(handles=handles(published), title=f"{THERE} (mean)", loc="upper left",
              bbox_to_anchor=(1.01, 0.45), fontsize=8, title_fontsize=8, alignment="left")
    ax.set_title("PanGui layout benchmarks: cold layout cost per node (node count in parentheses)\n"
                 "filled: measured on this machine; hollow: published, measured on another machine", fontsize=10)
    fig.tight_layout()
    path = out / "pangui_ns_per_node.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def ratio_bars(ax, items, label):
    """Horizontal bars from 1× outward, blue when faster, red when slower, sorted by ratio."""
    items = sorted(items, key=lambda kv: kv[1], reverse=True)
    ratios = [r for _, r in items]
    row_labels(ax, [n for n, _ in items])
    ax.barh(range(len(items)), [r - 1 for r in ratios], left=1, height=0.6,
            color=[FASTER if r >= 1 else SLOWER for r in ratios], zorder=2)
    for y, r in enumerate(ratios):
        ax.text(r * (1.04 if r >= 1 else 0.96), y, f"{r:.2f}×", va="center", ha="left" if r >= 1 else "right",
                fontsize=8, color="#0b0b0b")
    ratio_axis(ax, min(ratios), max(ratios), label)


def pangui_speedup_plot(rows, engines, out):
    """cephal over taffy for every workload measured with both, on this machine."""
    pairs = [(c, "taffy" + c.removeprefix("cephal")) for c in engines if c.startswith("cephal")]
    pairs = [(c, t) for c, t in pairs if t in engines]
    if not pairs:
        return None
    count = max(sum(1 for _, e in rows if e == c) for c, _ in pairs)
    fig, axes = plt.subplots(1, len(pairs), figsize=(7.5 * len(pairs), 0.3 * count + 1.6), squeeze=False)
    for ax, (c, t) in zip(axes[0], pairs):
        items = [(w, rows[(w, t)][1] / rows[(w, c)][1]) for w, e in rows if e == c and (w, t) in rows]
        ratio_bars(ax, items, "taffy time / cephal time (log scale; right of 1× is cephal faster)")
        suffix = " (no rounding)" if c.endswith("_no_rounding") else ""
        ax.set_title(f"cephal speed-up over taffy{suffix}, same tree, same machine", fontsize=10)
    fig.tight_layout()
    path = out / "pangui_speedup_vs_taffy.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def pangui_relative_plot(rows, engines, out):
    """Speed-up over taffy per engine, each against taffy on its own machine, so machines cancel."""
    c, t = engines[0], "taffy" + engines[0].removeprefix("cephal")
    if not c.startswith("cephal") or t not in engines:
        return None
    workloads = [w for w in PUBLISHED if (w, c) in rows and (w, t) in rows and PUBLISHED[w][3]]
    if not workloads:
        return None
    here = {w: rows[(w, t)][1] / rows[(w, c)][1] for w in workloads}
    workloads.sort(key=lambda w: here[w], reverse=True)
    series = [(series_style(c)[2], *STYLE["cephal"], True, here)]
    min0 = {w: rows[(w + "_min0", t)][1] / rows[(w + "_min0", c)][1]
            for w in workloads if (w + "_min0", c) in rows and (w + "_min0", t) in rows}
    if min0:
        series.append((series_style(c, min0=True)[2], *STYLE["cephal_min0"], True, min0))
    for i, name in enumerate(PUBLISHED_ENGINES):
        vals = {w: PUBLISHED[w][3] / PUBLISHED[w][1 + i] for w in workloads if PUBLISHED[w][1 + i]}
        if name != "Taffy":
            series.append((name, *STYLE[name], False, vals))

    fig, ax = plt.subplots(figsize=(11.5, 0.55 * len(workloads) + 1.8))
    row_labels(ax, workloads)
    dots(ax, workloads, series, 0.1)
    ratio_axis(ax, min(v for *_, vals in series for v in vals.values()),
               max(v for *_, vals in series for v in vals.values()),
               "speed-up over taffy (log scale; right of 1× is faster than taffy)")
    ax.legend(handles=handles(series, lambda lab, f: f"{lab} ({HERE if f else THERE})"), loc="upper left",
              bbox_to_anchor=(1.01, 1.0), fontsize=8)
    ax.set_title("speed-up over taffy on the same machine: cephal against taffy main here (filled),\n"
                 f"PanGui, Clay and Yoga against the taffy PanGui benchmarked ({PUBLISHED_MACHINE}, hollow)",
                 fontsize=10)
    fig.tight_layout()
    path = out / "pangui_relative_to_taffy.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def pangui_baseline_plot(rows, base, engines, out):
    """Per-workload speed-up of this run over a baseline run, for each cephal engine in both."""
    both = [e for e in engines if e.startswith("cephal") and any(k[1] == e for k in base)]
    if not both:
        return None
    count = max(sum(1 for _, en in rows if en == e) for e in both)
    fig, axes = plt.subplots(1, len(both), figsize=(7.5 * len(both), 0.3 * count + 1.6), squeeze=False)
    for ax, e in zip(axes[0], both):
        items = [(w, base[(w, e)][1] / rows[(w, e)][1]) for w, en in rows if en == e and (w, e) in base]
        ratio_bars(ax, items, "baseline time / this run's time (log scale; right of 1× is faster)")
        ax.set_title(f"{e}: speed-up over the baseline run", fontsize=10)
    fig.tight_layout()
    path = out / "pangui_vs_baseline.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def pangui_table(rows, engines, base):
    """Markdown table of the measured ns/node, with the published numbers alongside."""
    head = ["workload", "nodes"] + [f"{e} ns/node" for e in engines]
    ratio = "cephal" in engines and "taffy" in engines
    if ratio:
        head.append("taffy / cephal")
    if base:
        head.append("baseline cephal")
    head += [f"{n} (published)" for n in PUBLISHED_ENGINES]
    print("| " + " | ".join(head) + " |")
    print("| --- |" + " ---: |" * (len(head) - 1))
    for w in dict.fromkeys(w for w, _ in rows):
        nodes = next(rows[(w, e)][0] for e in engines if (w, e) in rows)
        cells = [w, f"{nodes:,}"] + [f"{rows[(w, e)][1]:.1f}" if (w, e) in rows else "" for e in engines]
        if ratio:
            both = (w, "cephal") in rows and (w, "taffy") in rows
            cells.append(f"{rows[(w, 'taffy')][1] / rows[(w, 'cephal')][1]:.2f}×" if both else "")
        if base:
            cells.append(f"{base[(w, 'cephal')][1]:.1f}" if (w, "cephal") in base else "")
        pub = PUBLISHED.get(w)
        cells += [(f"{v:.1f}" if v else "—") if pub else "" for v in (pub[1:] if pub else PUBLISHED_ENGINES)]
        print("| " + " | ".join(cells) + " |")


def pangui_main(argv):
    p = argparse.ArgumentParser(prog="plot_bench.py pangui")
    p.add_argument("csv", nargs="*", default=["target/bench/suite.csv"],
                   help="CSVs written by CEPHAL_BENCH_CSV; later files override rows of earlier ones")
    p.add_argument("--baseline", help="a CSV of an earlier run to compare cephal against")
    p.add_argument("--out", default="target/bench/plots")
    args = p.parse_args(argv)
    rows = load_pangui(args.csv)
    base = load_pangui([args.baseline]) if args.baseline else {}
    engines = engines_of(rows)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    pangui_table(rows, engines, base)
    paths = [
        pangui_ns_plot(rows, engines, out),
        pangui_speedup_plot(rows, engines, out),
        pangui_relative_plot(rows, engines, out),
        pangui_baseline_plot(rows, base, engines, out) if base else None,
    ]
    for path in filter(None, paths):
        print(path)


def main():
    if sys.argv[1:2] == ["pangui"]:
        return pangui_main(sys.argv[2:])
    if sys.argv[1:2] == ["suite"]:
        import plot_suite  # noqa: PLC0415

        return plot_suite.main(sys.argv[2:])
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
