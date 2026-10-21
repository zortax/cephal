"""Plots of the comparative suite (`cargo bench --bench suite`, `scripts/external.sh`).

Run through `scripts/plot_bench.py suite ...`; see its docstring. Every plot reads the CSV's rows, the
`<csv>.meta.json` beside the first CSV, and marks workloads where an engine's layout differs from
cephal's as not comparable instead of reporting a speed-up.
"""

import argparse
import csv
import json
import math
from collections import defaultdict
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402
from matplotlib.colors import TwoSlopeNorm  # noqa: E402
from matplotlib.lines import Line2D  # noqa: E402
from matplotlib.patches import Patch  # noqa: E402
from matplotlib.ticker import FixedLocator, FuncFormatter, NullFormatter  # noqa: E402

ENGINES = ["cephal", "cephal_parallel", "taffy", "yoga", "clay"]
LABEL = {"cephal": "cephal", "cephal_parallel": "cephal (parallel)", "taffy": "taffy 0.15", "yoga": "Yoga", "clay": "Clay"}
COLOR = {"cephal": "#2a78d6", "cephal_parallel": "#1baf7a", "taffy": "#e34948", "yoga": "#4a3aa7", "clay": "#eda100"}
MARKER = {"cephal": "o", "cephal_parallel": "s", "taffy": "^", "yoga": "X", "clay": "P"}
# The first tag of these decides a workload's group, in this order.
GROUPS = [
    ("pangui", "PanGui's workloads"),
    ("cephal", "cephal's home ground"),
    ("taffy", "per-call overhead"),
    ("grid", "grid"),
    ("flex", "flex"),
    ("block", "block flow"),
    ("mixed", "mixed documents"),
]
# Fast-path pairs: the workload taking a path, and the ones built to miss it.
PAIRS = [
    ("fused flex", "row_fixed", ["row_one_baseline", "row_one_auto_margin", "row_one_percent", "row_one_aspect", "row_one_content_basis", "row_wrap"]),
    ("cheap leaves", "row_empty_leaves", ["row_measured_leaves"]),
    ("fixed grid tracks", "grid_fixed", ["grid_one_auto_column"]),
]
FASTER, SLOWER, GRAY = "#2a78d6", "#e34948", "#9a9893"
BUDGETS = [(1 / 60, "60 Hz"), (1 / 120, "120 Hz")]


def load(paths):
    rows = []
    for path in paths:
        with open(path) as f:
            for r in csv.DictReader(f):
                for k in ("ns_per_node", "median_ms", "p10_ms", "p90_ms", "max_err"):
                    r[k] = float(r[k]) if r[k] not in ("", None) else math.nan
                for k in ("size", "nodes", "samples"):
                    r[k] = int(r[k])
                r["measure_calls"] = int(r["measure_calls"]) if r.get("measure_calls") else None
                r["tags"] = r["tags"].split("|") if r["tags"] else []
                rows.append(r)
    # Later files override earlier rows of the same measurement.
    keyed = {}
    for r in rows:
        keyed[(r["workload"], r["size"], r["engine"], r["mode"], r["scenario"])] = r
    return list(keyed.values())


def meta_of(path):
    p = Path(path)
    m = p.with_name(p.stem + ".meta.json")
    try:
        return json.loads(m.read_text())
    except (OSError, ValueError):
        return {}


def footer(fig, meta):
    if not meta:
        return
    text = (f"{meta.get('cpu', '?')} · core {meta.get('affinity', '?').split(':')[-1].strip()} · "
            f"governor {meta.get('governor', '?')} · {meta.get('rustc', '?')} · cephal {meta.get('cephal', '?')[:10]}"
            f"{' (dirty)' if meta.get('cephal_dirty') else ''} · taffy {meta.get('taffy', '?')} · "
            f"{meta.get('secs', '?')} s/sample set · reads {'timed' if meta.get('read') else 'not timed'} · "
            f"rounding {'on' if meta.get('rounding', True) else 'off'} · {meta.get('date', '')}")
    import textwrap  # noqa: PLC0415

    width = max(60, int(fig.get_size_inches()[0] * 21))
    fig.text(0.005, 0.003, "\n".join(textwrap.wrap(text, width)), fontsize=6.5, color="#6b6a66", ha="left", va="bottom")


def group_of(tags):
    for tag, _ in GROUPS:
        if tag in tags:
            return tag
    return "mixed"


def group_label(tag):
    return dict(GROUPS)[tag]


def cold(rows):
    """{(workload, size): {engine: row}} of cold layouts."""
    out = defaultdict(dict)
    for r in rows:
        if r["mode"] == "cold":
            out[(r["workload"], r["size"])][r["engine"]] = r
    return out


def representative(cold_rows):
    """One size per workload: the one closest to 10,000 nodes."""
    best = {}
    for (w, size), engines in cold_rows.items():
        nodes = next(iter(engines.values()))["nodes"]
        score = abs(math.log10(max(nodes, 1)) - 4)
        if w not in best or score < best[w][0]:
            best[w] = (score, size)
    return {w: s for w, (_, s) in best.items()}


def ordered_workloads(cold_rows, rep):
    """Workloads grouped, then sorted by cephal's ns/node within the group."""
    def key(w):
        r = cold_rows[(w, rep[w])]
        tags = next(iter(r.values()))["tags"]
        g = [t for t, _ in GROUPS].index(group_of(tags))
        c = r.get("cephal", {}).get("ns_per_node", math.inf)
        return (g, c)
    return sorted(rep, key=key)


def engines_present(rows):
    present = {r["engine"] for r in rows}
    return [e for e in ENGINES if e in present]


def comparable(r):
    return r is not None and r.get("agree") in ("exact", "close", "-", "")


def fmt_ns(v):
    return f"{v:.0f}" if v >= 10 else f"{v:.1f}"


def log_x(ax, values, label):
    vals = [v for v in values if v > 0 and math.isfinite(v)]
    lo, hi = min(vals) / 1.5, max(vals) * 1.5
    ax.set_xscale("log")
    ax.set_xlim(lo, hi)
    ticks = [t for t in (1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000, 50000) if lo <= t <= hi]
    ax.xaxis.set_major_locator(FixedLocator(ticks))
    ax.xaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v:g}"))
    ax.xaxis.set_minor_formatter(NullFormatter())
    ax.set_xlabel(label)
    ax.grid(True, axis="x", alpha=0.3)


def shade_rows(ax, n):
    for y in range(1, n, 2):
        ax.axhspan(y - 0.5, y + 0.5, color="#f0efec", zorder=0, linewidth=0)


# ---- Plots ----------------------------------------------------------------------------------------


def overview_heatmap(rows, meta, out):
    """Workloads × engines; colour is the log ratio of the engine's time to cephal's."""
    c = cold(rows)
    rep = representative(c)
    order = ordered_workloads(c, rep)
    engines = engines_present([r for r in rows if r["mode"] == "cold"])
    data = np.full((len(order), len(engines)), np.nan)
    fig, ax = plt.subplots(figsize=(2.0 + 1.35 * len(engines), 0.27 * len(order) + 1.6))
    for i, w in enumerate(order):
        er = c[(w, rep[w])]
        base = er.get("cephal")
        for j, e in enumerate(engines):
            r = er.get(e)
            if r is None or base is None:
                continue
            if comparable(r):
                data[i, j] = math.log2(r["ns_per_node"] / base["ns_per_node"])
    lim = max(1.0, np.nanmax(np.abs(data))) if np.isfinite(data).any() else 1.0
    cmap = plt.get_cmap("RdBu_r")
    ax.imshow(data, aspect="auto", cmap=cmap, norm=TwoSlopeNorm(vcenter=0, vmin=-lim, vmax=lim))
    for i, w in enumerate(order):
        er = c[(w, rep[w])]
        for j, e in enumerate(engines):
            r = er.get(e)
            if r is None:
                ax.text(j, i, "–", ha="center", va="center", fontsize=7, color=GRAY)
                continue
            text = fmt_ns(r["ns_per_node"])
            if not comparable(r):
                ax.add_patch(plt.Rectangle((j - 0.5, i - 0.5), 1, 1, fill=False, hatch="////", edgecolor=GRAY, linewidth=0))
                text += " ≠"
            dark = math.isfinite(data[i, j]) and abs(data[i, j]) > lim * 0.6
            ax.text(j, i, text, ha="center", va="center", fontsize=7, color="white" if dark else "#0b0b0b")
    labels = []
    last = None
    for w in order:
        g = group_of(next(iter(c[(w, rep[w])].values()))["tags"])
        nodes = next(iter(c[(w, rep[w])].values()))["nodes"]
        labels.append(f"{w} ({nodes:,})")
        if g != last and last is not None:
            ax.axhline(order.index(w) - 0.5, color="#0b0b0b", linewidth=0.8)
        last = g
    ax.set_yticks(range(len(order)))
    ax.set_yticklabels(labels, fontsize=7)
    ax.set_xticks(range(len(engines)))
    ax.set_xticklabels([LABEL[e] for e in engines], fontsize=8)
    ax.xaxis.tick_top()
    # Group names at the right edge.
    starts = {}
    for i, w in enumerate(order):
        starts.setdefault(group_of(next(iter(c[(w, rep[w])].values()))["tags"]), []).append(i)
    for g, idx in starts.items():
        # A group of a few rows has no room for its name along the edge; write it across.
        tall = len(idx) >= 4
        ax.annotate(group_label(g), xy=(1.01, (idx[0] + idx[-1]) / 2), xycoords=("axes fraction", "data"), rotation=-90 if tall else 0,
                    va="center", ha="left", fontsize=8 if tall else 6.5, color="#52514e", annotation_clip=False)
    sm = plt.cm.ScalarMappable(cmap=cmap, norm=TwoSlopeNorm(vcenter=0, vmin=-lim, vmax=lim))
    cb = fig.colorbar(sm, ax=ax, fraction=0.04, pad=0.09)
    cb.set_label("time relative to cephal (log scale)")
    ticks = [t for t in range(-int(lim), int(lim) + 1)]
    cb.set_ticks(ticks)
    cb.set_ticklabels([f"{2 ** t:g}×" for t in ticks])
    ax.set_title("cold layout, ns per node (blue: faster than cephal, red: slower; ≠ hatched: layout differs, not comparable)", fontsize=9, pad=24)
    fig.tight_layout(rect=(0, 0.015, 1, 1))
    footer(fig, meta)
    path = out / "overview_heatmap.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def category_dumbbells(rows, meta, out):
    """Per group: each workload's engines on one log axis, cephal to taffy joined by a line."""
    c = cold(rows)
    rep = representative(c)
    order = ordered_workloads(c, rep)
    engines = engines_present([r for r in rows if r["mode"] == "cold"])
    groups = [g for g, _ in GROUPS if any(group_of(next(iter(c[(w, rep[w])].values()))["tags"]) == g for w in order)]
    counts = [sum(1 for w in order if group_of(next(iter(c[(w, rep[w])].values()))["tags"]) == g) for g in groups]
    fig, axes = plt.subplots(len(groups), 1, figsize=(11, 0.33 * sum(counts) + 1.0 * len(groups)), gridspec_kw={"height_ratios": counts}, squeeze=False)
    allv = [r["ns_per_node"] for k, er in c.items() for r in er.values()]
    for ax, g in zip(axes[:, 0], groups):
        ws = [w for w in order if group_of(next(iter(c[(w, rep[w])].values()))["tags"]) == g]
        shade_rows(ax, len(ws))
        for y, w in enumerate(ws):
            er = c[(w, rep[w])]
            a, b = er.get("cephal"), er.get("taffy")
            if a and b:
                ax.plot([a["ns_per_node"], b["ns_per_node"]], [y, y], color=FASTER if a["ns_per_node"] <= b["ns_per_node"] else SLOWER, linewidth=2.2, alpha=0.5, zorder=1)
            for e in engines:
                r = er.get(e)
                if r is None:
                    continue
                ok = comparable(r)
                ax.scatter([r["ns_per_node"]], [y], marker=MARKER[e], s=40, zorder=3, facecolors=COLOR[e] if ok else "white", edgecolors=COLOR[e], linewidths=1.4)
            if a and b and comparable(b):
                ratio = b["ns_per_node"] / a["ns_per_node"]
                ax.text(log_hi(allv), y, f"{ratio:.2f}×", va="center", ha="right", fontsize=7, color=FASTER if ratio >= 1 else SLOWER)
        ax.set_yticks(range(len(ws)))
        ax.set_yticklabels([f"{w} ({next(iter(c[(w, rep[w])].values()))['nodes']:,})" for w in ws], fontsize=7.5)
        ax.set_ylim(len(ws) - 0.5, -0.5)
        log_x(ax, allv, "")
        ax.set_title(group_label(g), fontsize=9, loc="left")
    axes[-1, 0].set_xlabel("ns per node, cold layout (log scale; right column: taffy time / cephal time)")
    handles = [Line2D([], [], linestyle="none", marker=MARKER[e], color=COLOR[e], markerfacecolor=COLOR[e], label=LABEL[e]) for e in engines]
    handles.append(Line2D([], [], linestyle="none", marker="o", color=GRAY, markerfacecolor="white", label="hollow: layout differs from cephal's"))
    fig.legend(handles=handles, loc="upper right", fontsize=8, ncol=len(handles))
    fig.tight_layout(rect=(0, 0.015, 1, 0.97))
    footer(fig, meta)
    path = out / "category_dumbbells.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def log_hi(values):
    vals = [v for v in values if v > 0 and math.isfinite(v)]
    return max(vals) * 1.45


def scaling(rows, meta, out):
    """ns/node against tree size per family, with the p10–p90 band."""
    by = defaultdict(lambda: defaultdict(list))
    for r in rows:
        if r["mode"] == "cold":
            by[r["workload"]][r["engine"]].append(r)
    fams = [w for w, es in by.items() if len({r["size"] for rs in es.values() for r in rs}) >= 3]
    if not fams:
        return None
    cols = 4
    nrows = math.ceil(len(fams) / cols)
    fig, axes = plt.subplots(nrows, cols, figsize=(4.2 * cols, 3.2 * nrows), squeeze=False)
    for ax, w in zip(axes.flat, fams):
        for e in ENGINES:
            rs = sorted(by[w].get(e, []), key=lambda r: r["nodes"])
            if not rs:
                continue
            x = [r["nodes"] for r in rs]
            y = [r["ns_per_node"] for r in rs]
            lo = [r["p10_ms"] * 1e6 / r["nodes"] for r in rs]
            hi = [r["p90_ms"] * 1e6 / r["nodes"] for r in rs]
            ax.fill_between(x, lo, hi, color=COLOR[e], alpha=0.15, linewidth=0)
            ax.plot(x, y, marker=MARKER[e], color=COLOR[e], label=LABEL[e], markersize=4)
            for r in rs:
                if not comparable(r):
                    ax.scatter([r["nodes"]], [r["ns_per_node"]], marker=MARKER[e], s=60, facecolors="white", edgecolors=COLOR[e], zorder=4)
        ax.set_xscale("log")
        ax.set_yscale("log")
        ax.set_title(w, fontsize=9)
        ax.grid(True, which="major", alpha=0.3)
        ax.xaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v / 1000:g}k" if v >= 1000 else f"{v:g}"))
        ax.yaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v:g}"))
        ax.set_xlabel("nodes", fontsize=8)
        ax.set_ylabel("ns/node", fontsize=8)
    for ax in list(axes.flat)[len(fams):]:
        ax.axis("off")
    axes[0, 0].legend(fontsize=7)
    fig.suptitle("cold layout cost per node as trees grow (band: p10–p90; flat is linear scaling, rising is worse)", fontsize=10)
    fig.tight_layout(rect=(0, 0.015, 1, 0.97))
    footer(fig, meta)
    path = out / "scaling.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def fastpath_pairs(rows, meta, out):
    """Slope chart from each fast path's workload to the ones that miss it, per engine."""
    c = cold(rows)
    size_of = representative(c)
    panels = [(name, hit, misses) for name, hit, misses in PAIRS if hit in size_of]
    if not panels:
        return None
    fig, axes = plt.subplots(1, len(panels), figsize=(5.0 * len(panels), 4.6), squeeze=False, gridspec_kw={"width_ratios": [1 + len(m) for _, _, m in panels]})
    for ax, (name, hit, misses) in zip(axes[0], panels):
        names = [hit] + [m for m in misses if m in size_of]
        for e in ("cephal", "taffy", "yoga"):
            ys = []
            for n in names:
                r = c.get((n, size_of[n]), {}).get(e)
                ys.append(r["ns_per_node"] if r and comparable(r) else math.nan)
            if all(math.isnan(v) for v in ys):
                continue
            ax.plot(range(len(names)), ys, marker=MARKER[e], color=COLOR[e], label=LABEL[e])
            base = ys[0]
            for i, v in enumerate(ys[1:], 1):
                if math.isfinite(v) and math.isfinite(base):
                    ax.annotate(f"{v / base:.2f}×", (i, v), xytext=(4, 4), textcoords="offset points", fontsize=7, color=COLOR[e])
        ax.set_xticks(range(len(names)))
        ax.set_xticklabels([n.replace("row_one_", "one ").replace("row_", "").replace("grid_", "") for n in names], rotation=30, ha="right", fontsize=8)
        ax.get_xticklabels()[0].set_fontweight("bold")
        ax.set_yscale("log")
        ax.yaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v:g}"))
        ax.grid(True, alpha=0.3)
        ax.set_ylabel("ns/node, cold")
        ax.set_title(name, fontsize=9)
        ax.legend(fontsize=7)
    fig.suptitle("what each fast path is worth: the workload in bold takes the path, each other one misses it by one property;\n"
                 "labels give the cost relative to the path", fontsize=10)
    fig.tight_layout(rect=(0, 0.015, 1, 0.95))
    footer(fig, meta)
    path = out / "fastpath_pairs.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def incremental_frames(rows, meta, out):
    """Frame time against tree size per scenario, with frame budgets; and incremental over cold."""
    inc = [r for r in rows if r["mode"] == "incremental"]
    if not inc:
        return None
    scenarios = sorted({r["scenario"] for r in inc}, key=lambda s: ["edit_text", "hover_width", "align_self", "relative_inset", "move_absolute", "toggle_display", "insert_remove", "resize_root", "idle"].index(s) if s in ["edit_text", "hover_width", "align_self", "relative_inset", "move_absolute", "toggle_display", "insert_remove", "resize_root", "idle"] else 99)
    workloads = sorted({r["workload"] for r in inc})
    styles = ["-", "--", ":", "-.", (0, (5, 1, 1, 1, 1, 1)), (0, (1, 3))]
    cols = 3
    nrows = math.ceil((len(scenarios) + 1) / cols)
    fig, axes = plt.subplots(nrows, cols, figsize=(5.2 * cols, 3.6 * nrows), squeeze=False)
    for ax, sc in zip(axes.flat, scenarios):
        for wi, w in enumerate(workloads):
            for e in ENGINES:
                rs = sorted([r for r in inc if r["scenario"] == sc and r["workload"] == w and r["engine"] == e], key=lambda r: r["nodes"])
                if not rs:
                    continue
                ax.plot([r["nodes"] for r in rs], [r["median_ms"] * 1e-3 for r in rs], linestyle=styles[wi % len(styles)], marker=MARKER[e], color=COLOR[e], markersize=3.5, label=f"{LABEL[e]} · {w}")
        for b, name in BUDGETS:
            ax.axhline(b, color=GRAY, linestyle="--", linewidth=0.8)
            ax.text(ax.get_xlim()[0] if ax.lines else 1, b, f" {name}", fontsize=6.5, color=GRAY, va="bottom")
        ax.set_xscale("log")
        ax.set_yscale("log")
        ax.set_title(sc.replace("_", " "), fontsize=9)
        ax.grid(True, alpha=0.3)
        ax.yaxis.set_major_formatter(FuncFormatter(seconds))
        ax.xaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v / 1000:g}k" if v >= 1000 else f"{v:g}"))
        ax.set_xlabel("nodes", fontsize=8)
    # Incremental over cold: how many frames of this edit fit in one cold layout, at the largest size.
    ax = list(axes.flat)[len(scenarios)]
    c = cold(rows)
    bars = []
    for w in workloads:
        sizes = sorted({r["size"] for r in inc if r["workload"] == w})
        if not sizes:
            continue
        s = sizes[-1]
        for sc in scenarios:
            if sc == "idle":
                continue
            for e in ("cephal", "taffy"):
                r = next((r for r in inc if r["workload"] == w and r["size"] == s and r["scenario"] == sc and r["engine"] == e), None)
                base = c.get((w, s), {}).get(e)
                if r and base and r["median_ms"] > 0:
                    bars.append((f"{w}·{sc}", e, base["median_ms"] / r["median_ms"]))
    labels = sorted({b[0] for b in bars}, key=lambda l: -max(v for k, _, v in bars if k == l))
    for i, lab in enumerate(labels):
        for j, e in enumerate(("cephal", "taffy")):
            v = next((v for k, en, v in bars if k == lab and en == e), None)
            if v:
                ax.barh(i + (j - 0.5) * 0.38, v, height=0.36, color=COLOR[e])
    ax.set_yticks(range(len(labels)))
    ax.set_yticklabels(labels, fontsize=6)
    ax.set_ylim(len(labels) - 0.5, -0.5)
    ax.set_xscale("log")
    ax.set_xlabel("cold layout time / incremental frame time", fontsize=8)
    ax.set_title("incremental speed-up over a cold layout (largest size)", fontsize=9)
    ax.grid(True, axis="x", alpha=0.3)
    for extra in list(axes.flat)[len(scenarios) + 1:]:
        extra.axis("off")
    handles, labs = axes[0, 0].get_legend_handles_labels()
    fig.legend(handles, labs, loc="lower right", fontsize=7, ncol=2)
    fig.suptitle("incremental frames: one edit then layout, per scenario (dashed lines: 60 and 120 Hz frame budgets)", fontsize=10)
    fig.tight_layout(rect=(0, 0.015, 1, 0.97))
    footer(fig, meta)
    path = out / "incremental_frames.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def seconds(v, _=None):
    for scale, unit in ((1, "s"), (1e-3, "ms"), (1e-6, "µs"), (1e-9, "ns")):
        if v >= scale:
            return f"{v / scale:g} {unit}"
    return f"{v:g}"


def measure_calls(rows, meta, out):
    """Measure calls per node on the workloads with measured text, per engine."""
    c = cold(rows)
    rep = representative(c)
    items = []
    for w, s in rep.items():
        er = c[(w, s)]
        if "text" not in next(iter(er.values()))["tags"]:
            continue
        vals = {e: er[e]["measure_calls"] / er[e]["nodes"] for e in ("cephal", "taffy", "yoga") if e in er and er[e].get("measure_calls")}
        if vals:
            items.append((w, vals))
    if not items:
        return None
    items.sort(key=lambda kv: kv[1].get("cephal", 0))
    fig, ax = plt.subplots(figsize=(10, 0.32 * len(items) + 1.4))
    shade_rows(ax, len(items))
    present = [e for e in ("cephal", "taffy", "yoga") if any(e in v for _, v in items)]
    h = 0.8 / len(present)
    for j, e in enumerate(present):
        ys = [i + (j - (len(present) - 1) / 2) * h for i in range(len(items))]
        vs = [v.get(e, 0) for _, v in items]
        ax.barh(ys, vs, height=h * 0.95, color=COLOR[e], label=LABEL[e])
        for y, v in zip(ys, vs):
            if v:
                ax.text(v, y, f" {v:.2f}", va="center", fontsize=6.5)
    ax.set_yticks(range(len(items)))
    ax.set_yticklabels([w for w, _ in items], fontsize=8)
    ax.set_ylim(len(items) - 0.5, -0.5)
    ax.set_xlabel("text measurements per node in one cold layout")
    ax.set_title("how often each engine asks the content for its size (fewer is better: text shaping is the expensive part in real UIs)", fontsize=9)
    ax.legend(fontsize=8)
    ax.grid(True, axis="x", alpha=0.3)
    fig.tight_layout(rect=(0, 0.015, 1, 1))
    footer(fig, meta)
    path = out / "measure_calls.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def win_loss(rows, meta, out):
    """Every workload's speed-up of cephal over each other engine, sorted, coloured by group."""
    c = cold(rows)
    rep = representative(c)
    others = [e for e in ("taffy", "yoga", "clay") if any(e in er for er in c.values())]
    if not others:
        return None
    palette = dict(zip([g for g, _ in GROUPS], ["#eb6834", "#2a78d6", "#e34948", "#1baf7a", "#4a3aa7", "#9a9893", "#eda100"]))
    widths = [sum(1 for w, s in rep.items() if e in c[(w, s)]) for e in others]
    fig, axes = plt.subplots(1, len(others), figsize=(5.5 * len(others), 0.22 * max(widths) + 1.8), squeeze=False)
    for ax, e in zip(axes[0], others):
        items = []
        for w, s in rep.items():
            er = c[(w, s)]
            if e in er and "cephal" in er:
                ratio = er[e]["ns_per_node"] / er["cephal"]["ns_per_node"]
                items.append((w, ratio, comparable(er[e]), group_of(er["cephal"]["tags"])))
        items.sort(key=lambda t: t[1], reverse=True)
        shade_rows(ax, len(items))
        for y, (w, r, ok, g) in enumerate(items):
            ax.barh(y, r - 1, left=1, height=0.7, color=palette[g] if ok else "white", edgecolor=palette[g], hatch=None if ok else "////")
            ax.text(r * (1.05 if r >= 1 else 0.95), y, f"{r:.2f}×" + ("" if ok else " ≠"), va="center", ha="left" if r >= 1 else "right", fontsize=6.5)
        ax.set_yticks(range(len(items)))
        ax.set_yticklabels([w for w, *_ in items], fontsize=6.5)
        ax.set_ylim(len(items) - 0.5, -0.5)
        ax.set_xscale("log")
        ax.axvline(1, color="#0b0b0b", linewidth=0.8)
        ax.xaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v:g}×"))
        ax.set_xlabel(f"{LABEL[e]} time / cephal time (right of 1×: cephal faster)", fontsize=8)
        ax.set_title(f"cephal against {LABEL[e]}", fontsize=9)
        ax.grid(True, axis="x", alpha=0.3)
    fig.legend(handles=[Patch(color=palette[g], label=l) for g, l in GROUPS] + [Patch(facecolor="white", edgecolor=GRAY, hatch="////", label="layout differs (not comparable)")], loc="lower center", ncol=8, fontsize=7)
    fig.tight_layout(rect=(0, 0.05, 1, 1))
    footer(fig, meta)
    path = out / "win_loss.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def ipc_scatter(rows, instr_path, meta, out):
    """Instructions per node against time per node for cephal: memory-bound workloads sit high."""
    if not instr_path:
        return None
    instr = {}
    with open(instr_path) as f:
        for r in csv.DictReader(f):
            instr[(r["workload"], int(r["size"]))] = float(r["instructions_per_node"])
    c = cold(rows)
    pts = [(w, s, instr[(w, s)], er["cephal"]["ns_per_node"], group_of(er["cephal"]["tags"])) for (w, s), er in c.items() if (w, s) in instr and "cephal" in er]
    if not pts:
        return None
    fig, ax = plt.subplots(figsize=(9, 6.5))
    palette = dict(zip([g for g, _ in GROUPS], ["#eb6834", "#2a78d6", "#e34948", "#1baf7a", "#4a3aa7", "#9a9893", "#eda100"]))
    for w, s, i, t, g in pts:
        ax.scatter([i], [t], color=palette[g], s=28, zorder=3)
        ax.annotate(f"{w}@{s}", (i, t), xytext=(3, 2), textcoords="offset points", fontsize=5.5, color="#52514e")
    xs = np.array([p[2] for p in pts])
    lo, hi = xs.min() / 1.3, xs.max() * 1.3
    # Iso-lines of instructions per nanosecond (IPC × GHz): a 5.5 GHz core retiring 1, 2 or 4 per cycle.
    for ipn, name in ((5.5, "1 IPC"), (11, "2 IPC"), (22, "4 IPC")):
        xx = np.geomspace(lo, hi, 50)
        ax.plot(xx, xx / ipn, color=GRAY, linestyle=":", linewidth=0.8)
        ax.text(hi, hi / ipn, f" {name} @5.5 GHz", fontsize=7, color=GRAY, va="center")
    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.set_xlabel("instructions per node (user space, one cold layout)")
    ax.set_ylabel("ns per node")
    ax.set_title("cephal's cold layout: work against time (above the IPC lines: stalls, mostly memory)", fontsize=9)
    ax.grid(True, alpha=0.3)
    ax.legend(handles=[Patch(color=palette[g], label=l) for g, l in GROUPS], fontsize=7)
    fig.tight_layout(rect=(0, 0.015, 1, 1))
    footer(fig, meta)
    path = out / "ipc_scatter.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def query_breakdown(stats_path, meta, out):
    """cephal's questions per node in one cold layout, by kind (the `stats` feature)."""
    if not stats_path:
        return None
    with open(stats_path) as f:
        rs = list(csv.DictReader(f))
    if not rs:
        return None
    keys = ["cheap_leaf", "hit_size", "hit_final", "miss_size", "miss_final", "measure", "speculative", "collision"]
    rs.sort(key=lambda r: sum(float(r[k]) for k in ("miss_size", "miss_final")) / int(r["nodes"]))
    fig, ax = plt.subplots(figsize=(11, 0.3 * len(rs) + 1.6))
    left = np.zeros(len(rs))
    colors = ["#c7c5bf", "#7fb3e8", "#2a78d6", "#f2a3a2", "#e34948", "#eda100", "#4a3aa7", "#1baf7a"]
    for k, col in zip(keys, colors):
        v = np.array([float(r.get(k, 0)) / int(r["nodes"]) for r in rs])
        ax.barh(range(len(rs)), v, left=left, color=col, label=k.replace("_", " "))
        left += v
    ax.set_yticks(range(len(rs)))
    ax.set_yticklabels([f"{r['workload']}@{r['size']}" for r in rs], fontsize=6.5)
    ax.set_xlabel("per node, one cold layout")
    ax.set_title("cephal's questions by kind: cache hits, misses (container runs), cheap leaves, measures; collisions are misses another key had answered", fontsize=9)
    ax.legend(fontsize=7, ncol=4)
    ax.grid(True, axis="x", alpha=0.3)
    fig.tight_layout(rect=(0, 0.015, 1, 1))
    footer(fig, meta)
    path = out / "query_breakdown.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def noise(rows, meta, out):
    """Relative spread (p90 − p10) / median of every measurement, per engine: how much to trust a ratio."""
    fig, ax = plt.subplots(figsize=(8, 3.6))
    data, labels = [], []
    for e in ENGINES:
        v = [(r["p90_ms"] - r["p10_ms"]) / r["median_ms"] * 100 for r in rows if r["engine"] == e and r["median_ms"] > 0]
        if v:
            data.append(v)
            labels.append(LABEL[e])
    if not data:
        return None
    parts = ax.violinplot(data, showmedians=True, vert=False)
    for body, e in zip(parts["bodies"], [e for e in ENGINES if LABEL[e] in labels]):
        body.set_facecolor(COLOR[e])
    ax.set_yticks(range(1, len(labels) + 1))
    ax.set_yticklabels(labels)
    ax.set_xscale("log")
    ax.set_xlabel("p10–p90 spread, % of the median (log scale)")
    ax.set_title("measurement noise across all workloads and modes", fontsize=9)
    ax.grid(True, axis="x", alpha=0.3)
    fig.tight_layout(rect=(0, 0.03, 1, 1))
    footer(fig, meta)
    path = out / "noise.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def agreement(rows, meta, out):
    """Which engine's layout matches cephal's on which workload."""
    c = cold(rows)
    rep = representative(c)
    order = ordered_workloads(c, rep)
    engines = [e for e in engines_present([r for r in rows if r["mode"] == "cold"]) if e != "cephal"]
    code = {"exact": 0, "close": 1, "differs": 2}
    colors = ["#1baf7a", "#a6dcc6", "#e34948", "#f0efec"]
    data = np.full((len(order), len(engines)), 3)
    for i, w in enumerate(order):
        for j, e in enumerate(engines):
            r = c[(w, rep[w])].get(e)
            if r:
                data[i, j] = code.get(r["agree"], 3)
    fig, ax = plt.subplots(figsize=(2.5 + 1.3 * len(engines), 0.24 * len(order) + 1.4))
    ax.imshow(data, aspect="auto", cmap=matplotlib.colors.ListedColormap(colors), vmin=0, vmax=3)
    for i, w in enumerate(order):
        for j, e in enumerate(engines):
            r = c[(w, rep[w])].get(e)
            if r and r["agree"] == "differs":
                ax.text(j, i, f"{r['max_err']:.3g}px", ha="center", va="center", fontsize=6, color="white")
    ax.set_yticks(range(len(order)))
    ax.set_yticklabels(order, fontsize=6.5)
    ax.set_xticks(range(len(engines)))
    ax.set_xticklabels([LABEL[e] for e in engines], fontsize=8)
    ax.xaxis.tick_top()
    ax.legend(handles=[Patch(color=col, label=lab) for col, lab in zip(colors, ["same bits", "within 0.01 px", "differs (largest error shown)", "not run / skipped"])], loc="upper left", bbox_to_anchor=(1.02, 1), fontsize=7)
    ax.set_title("does the engine lay the workload out as cephal does?", fontsize=9, pad=20)
    fig.tight_layout(rect=(0, 0.015, 1, 1))
    footer(fig, meta)
    path = out / "agreement.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def baseline_plot(rows, base, meta, out):
    """Per-workload speed-up of this run's cephal over a baseline run's."""
    now, then = cold(rows), cold(base)
    items = [(f"{w}@{s}", then[(w, s)]["cephal"]["ns_per_node"] / er["cephal"]["ns_per_node"]) for (w, s), er in now.items() if "cephal" in er and "cephal" in then.get((w, s), {})]
    if not items:
        return None
    items.sort(key=lambda kv: kv[1], reverse=True)
    fig, ax = plt.subplots(figsize=(9, 0.22 * len(items) + 1.4))
    shade_rows(ax, len(items))
    for y, (w, r) in enumerate(items):
        ax.barh(y, r - 1, left=1, height=0.7, color=FASTER if r >= 1 else SLOWER)
        ax.text(r * (1.02 if r >= 1 else 0.98), y, f"{r:.2f}×", va="center", ha="left" if r >= 1 else "right", fontsize=6.5)
    ax.set_yticks(range(len(items)))
    ax.set_yticklabels([w for w, _ in items], fontsize=6.5)
    ax.set_ylim(len(items) - 0.5, -0.5)
    ax.set_xscale("log")
    ax.axvline(1, color="#0b0b0b", linewidth=0.8)
    ax.xaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v:g}×"))
    ax.set_xlabel("baseline time / this run's time (right of 1×: faster now)")
    ax.set_title("cephal against the baseline run, cold layout", fontsize=9)
    fig.tight_layout(rect=(0, 0.015, 1, 1))
    footer(fig, meta)
    path = out / "suite_vs_baseline.png"
    fig.savefig(path, dpi=150)
    plt.close(fig)
    return path


def report(rows, meta, out):
    """Markdown tables per group, the largest wins and losses, and the machine."""
    c = cold(rows)
    rep = representative(c)
    order = ordered_workloads(c, rep)
    engines = engines_present([r for r in rows if r["mode"] == "cold"])
    lines = ["# Benchmark report", ""]
    if meta:
        lines += [f"- machine: {meta.get('cpu')} ({meta.get('affinity')}), governor {meta.get('governor')}",
                  f"- {meta.get('rustc')}; cephal {meta.get('cephal')}{' (dirty)' if meta.get('cephal_dirty') else ''}; taffy {meta.get('taffy')}",
                  f"- reads timed: {meta.get('read')}; rounding: {meta.get('rounding')}; {meta.get('date')}", ""]
    lines += ["Cold layout, ns per node (median). `≠`: the engine's layout differs from cephal's, so the number is not a comparison.", ""]
    current = None
    for w in order:
        er = c[(w, rep[w])]
        g = group_of(next(iter(er.values()))["tags"])
        if g != current:
            current = g
            lines += ["", f"## {group_label(g)}", "", "| workload | nodes | " + " | ".join(LABEL[e] for e in engines) + " | taffy / cephal |",
                      "| --- | ---: |" + " ---: |" * (len(engines) + 1)]
        cells = []
        for e in engines:
            r = er.get(e)
            cells.append("" if r is None else fmt_ns(r["ns_per_node"]) + ("" if comparable(r) else " ≠"))
        ratio = ""
        if "cephal" in er and "taffy" in er and comparable(er["taffy"]):
            ratio = f"{er['taffy']['ns_per_node'] / er['cephal']['ns_per_node']:.2f}×"
        lines.append(f"| {w} | {next(iter(er.values()))['nodes']:,} | " + " | ".join(cells) + f" | {ratio} |")
    inc = [r for r in rows if r["mode"] == "incremental"]
    if inc:
        lines += ["", "## Incremental frames (median)", "", "| workload | nodes | scenario | " + " | ".join(LABEL[e] for e in ENGINES if any(r["engine"] == e for r in inc)) + " |",
                  "| --- | ---: | --- |" + " ---: |" * len([e for e in ENGINES if any(r["engine"] == e for r in inc)])]
        keys = sorted({(r["workload"], r["size"], r["nodes"], r["scenario"]) for r in inc})
        for w, s, n, sc in keys:
            cells = []
            for e in ENGINES:
                if not any(r["engine"] == e for r in inc):
                    continue
                r = next((r for r in inc if r["workload"] == w and r["size"] == s and r["scenario"] == sc and r["engine"] == e), None)
                cells.append(seconds(r["median_ms"] * 1e-3) if r else "")
            lines.append(f"| {w} | {n:,} | {sc} | " + " | ".join(cells) + " |")
    path = out / "report.md"
    path.write_text("\n".join(lines) + "\n")
    return path


def main(argv):
    p = argparse.ArgumentParser(prog="plot_bench.py suite")
    p.add_argument("csv", nargs="+")
    p.add_argument("--instructions", help="CSV of scripts/instructions.sh --csv")
    p.add_argument("--stats", help="CSV the suite writes under the `stats` feature")
    p.add_argument("--baseline", help="an earlier suite CSV to compare cephal against")
    p.add_argument("--out", default="target/bench/plots")
    args = p.parse_args(argv)
    rows = load(args.csv)
    meta = meta_of(args.csv[0])
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    paths = [
        overview_heatmap(rows, meta, out),
        category_dumbbells(rows, meta, out),
        scaling(rows, meta, out),
        fastpath_pairs(rows, meta, out),
        incremental_frames(rows, meta, out),
        measure_calls(rows, meta, out),
        win_loss(rows, meta, out),
        ipc_scatter(rows, args.instructions, meta, out),
        query_breakdown(args.stats, meta, out),
        noise(rows, meta, out),
        agreement(rows, meta, out),
        baseline_plot(rows, load([args.baseline]), meta, out) if args.baseline else None,
        report(rows, meta, out),
    ]
    for path in filter(None, paths):
        print(path)
