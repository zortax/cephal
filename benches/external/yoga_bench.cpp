// Yoga on the suite's workloads: cold layout and incremental scenarios, checked against cephal.
//
//   yoga_bench [--secs S] [--csv out.csv] [--no-rounding] trees/*.tree
//
// Every property is set explicitly to its CSS meaning, so Yoga's own defaults (column direction,
// no shrinking, flex-start content alignment) never leak in. A tree with anything Yoga cannot express
// exactly (grid, block flow, floats, sizing keywords) is skipped with the reason. Yoga has no
// automatic minimum size; the agreement column says where that, or anything else, makes it differ.
#include <yoga/Yoga.h>

#include <string>
#include <vector>

extern "C" {
#include "tree.h"
}

static long g_measures = 0;

static YGSize measure(YGNodeConstRef node, float width, YGMeasureMode wmode, float height, YGMeasureMode hmode) {
    g_measures++;
    uint32_t glyphs = (uint32_t)(uintptr_t)YGNodeGetContext(node);
    float known_w = wmode == YGMeasureModeExactly ? width : NAN;
    float avail_w = wmode == YGMeasureModeAtMost ? width : NAN;
    float known_h = hmode == YGMeasureModeExactly ? height : NAN;
    float w, h;
    measure_text(glyphs, known_w, known_h, avail_w, &w, &h);
    return YGSize{w, h};
}

// Why Yoga cannot lay out the tree as CSS would, or empty.
static std::string unsupported(const Tree &t) {
    for (int i = 0; i < t.count; i++) {
        const Node &n = t.nodes[i];
        if (n.grid) return "grid";
        if (n.child_count > 0 && strcmp(n.display, "flex") != 0) return std::string("display ") + n.display;
        if (strcmp(n.display, "grid") == 0 || strcmp(n.display, "flowroot") == 0) return std::string("display ") + n.display;
        if (strcmp(n.float_, "none") != 0) return "float";
        const Len *lens[] = {&n.width, &n.height, &n.min_width, &n.min_height, &n.max_width, &n.max_height, &n.flex_basis,
                             &n.gap[0], &n.gap[1]};
        for (const Len *l : lens)
            if (l->kind == LEN_UNSUPPORTED) return "sizing keyword";
        for (int k = 0; k < 4; k++)
            if (n.margin[k].kind == LEN_UNSUPPORTED || n.padding[k].kind == LEN_UNSUPPORTED || n.inset[k].kind == LEN_UNSUPPORTED)
                return "sizing keyword";
    }
    return "";
}

static YGAlign align(const char *v, YGAlign normal) {
    if (strcmp(v, "normal") == 0) return normal;
    if (strcmp(v, "auto") == 0) return YGAlignAuto;
    if (strcmp(v, "flexstart") == 0 || strcmp(v, "start") == 0 || strcmp(v, "selfstart") == 0) return YGAlignFlexStart;
    if (strcmp(v, "flexend") == 0 || strcmp(v, "end") == 0 || strcmp(v, "selfend") == 0) return YGAlignFlexEnd;
    if (strcmp(v, "center") == 0) return YGAlignCenter;
    if (strcmp(v, "baseline") == 0) return YGAlignBaseline;
    if (strcmp(v, "stretch") == 0) return YGAlignStretch;
    if (strcmp(v, "spacebetween") == 0) return YGAlignSpaceBetween;
    if (strcmp(v, "spacearound") == 0) return YGAlignSpaceAround;
    if (strcmp(v, "spaceevenly") == 0) return YGAlignSpaceEvenly;
    fprintf(stderr, "unknown alignment %s\n", v);
    exit(2);
}

static YGJustify justify(const char *v) {
    if (strcmp(v, "flexend") == 0 || strcmp(v, "end") == 0) return YGJustifyFlexEnd;
    if (strcmp(v, "center") == 0) return YGJustifyCenter;
    if (strcmp(v, "spacebetween") == 0) return YGJustifySpaceBetween;
    if (strcmp(v, "spacearound") == 0) return YGJustifySpaceAround;
    if (strcmp(v, "spaceevenly") == 0) return YGJustifySpaceEvenly;
    return YGJustifyFlexStart;  // normal, flex-start, start, stretch
}

#define SET_LEN(node, len, px, pct, autof)                                    \
    do {                                                                      \
        if ((len).kind == LEN_PX) px(node, (len).value);                      \
        else if ((len).kind == LEN_PCT) pct(node, (len).value * 100.0f);      \
        else autof(node);                                                     \
    } while (0)

// Sets what differs from Yoga's defaults to its CSS value. Lengths that are zero or `auto` are left
// unset, as a Yoga user would leave them: Yoga stores a set length in a value pool it then reads on
// every access, so setting zeros explicitly would make it slower without changing any result.
static void style(YGNodeRef y, const Node &n) {
    if (strcmp(n.display, "none") == 0) YGNodeStyleSetDisplay(y, YGDisplayNone);
    if (strcmp(n.position, "absolute") == 0) YGNodeStyleSetPositionType(y, YGPositionTypeAbsolute);
    if (strcmp(n.direction, "rtl") == 0) YGNodeStyleSetDirection(y, YGDirectionRTL);
    if (strcmp(n.box_sizing, "contentbox") == 0) YGNodeStyleSetBoxSizing(y, YGBoxSizingContentBox);
    if (strcmp(n.overflow, "hidden") == 0) YGNodeStyleSetOverflow(y, YGOverflowHidden);
    if (strcmp(n.overflow, "scroll") == 0) YGNodeStyleSetOverflow(y, YGOverflowScroll);
    // Yoga's defaults differ from CSS here: column direction, flex-start content, no shrinking.
    const char *d = n.flex_direction;
    YGNodeStyleSetFlexDirection(y, strcmp(d, "column") == 0 ? YGFlexDirectionColumn : strcmp(d, "rowreverse") == 0 ? YGFlexDirectionRowReverse : strcmp(d, "columnreverse") == 0 ? YGFlexDirectionColumnReverse : YGFlexDirectionRow);
    if (strcmp(n.flex_wrap, "nowrap") != 0) YGNodeStyleSetFlexWrap(y, strcmp(n.flex_wrap, "wrap") == 0 ? YGWrapWrap : YGWrapWrapReverse);
    YGJustify j = justify(n.justify_content);
    if (j != YGJustifyFlexStart) YGNodeStyleSetJustifyContent(y, j);
    YGAlign ai = align(n.align_items, YGAlignStretch);
    if (ai != YGAlignStretch) YGNodeStyleSetAlignItems(y, ai);
    YGAlign as = align(n.align_self, YGAlignAuto);
    if (as != YGAlignAuto) YGNodeStyleSetAlignSelf(y, as);
    YGNodeStyleSetAlignContent(y, align(n.align_content, YGAlignStretch));
    if (n.flex_grow != 0) YGNodeStyleSetFlexGrow(y, n.flex_grow);
    YGNodeStyleSetFlexShrink(y, n.flex_shrink);
    if (n.flex_basis.kind != LEN_AUTO) SET_LEN(y, n.flex_basis, YGNodeStyleSetFlexBasis, YGNodeStyleSetFlexBasisPercent, YGNodeStyleSetFlexBasisAuto);
    if (n.width.kind != LEN_AUTO) SET_LEN(y, n.width, YGNodeStyleSetWidth, YGNodeStyleSetWidthPercent, YGNodeStyleSetWidthAuto);
    if (n.height.kind != LEN_AUTO) SET_LEN(y, n.height, YGNodeStyleSetHeight, YGNodeStyleSetHeightPercent, YGNodeStyleSetHeightAuto);
    // `auto` min and max sizes are no constraint (Yoga has no automatic minimum size).
    auto minmax = [](YGNodeRef y, const Len &l, void (*px)(YGNodeRef, float), void (*pct)(YGNodeRef, float)) {
        if (l.kind == LEN_PX) px(y, l.value);
        else if (l.kind == LEN_PCT) pct(y, l.value * 100.0f);
    };
    minmax(y, n.min_width, YGNodeStyleSetMinWidth, YGNodeStyleSetMinWidthPercent);
    minmax(y, n.min_height, YGNodeStyleSetMinHeight, YGNodeStyleSetMinHeightPercent);
    minmax(y, n.max_width, YGNodeStyleSetMaxWidth, YGNodeStyleSetMaxWidthPercent);
    minmax(y, n.max_height, YGNodeStyleSetMaxHeight, YGNodeStyleSetMaxHeightPercent);
    if (n.aspect_ratio > 0) YGNodeStyleSetAspectRatio(y, n.aspect_ratio);
    static const YGEdge edges[4] = {YGEdgeTop, YGEdgeRight, YGEdgeBottom, YGEdgeLeft};
    for (int k = 0; k < 4; k++) {
        const Len &m = n.margin[k];
        if (m.kind == LEN_PX && m.value != 0) YGNodeStyleSetMargin(y, edges[k], m.value);
        else if (m.kind == LEN_PCT && m.value != 0) YGNodeStyleSetMarginPercent(y, edges[k], m.value * 100.0f);
        else if (m.kind == LEN_AUTO) YGNodeStyleSetMarginAuto(y, edges[k]);
        const Len &p = n.padding[k];
        if (p.kind == LEN_PCT && p.value != 0) YGNodeStyleSetPaddingPercent(y, edges[k], p.value * 100.0f);
        else if (p.value != 0) YGNodeStyleSetPadding(y, edges[k], p.value);
        if (n.border[k].value != 0) YGNodeStyleSetBorder(y, edges[k], n.border[k].value);
        const Len &i = n.inset[k];
        if (i.kind == LEN_PX) YGNodeStyleSetPosition(y, edges[k], i.value);
        else if (i.kind == LEN_PCT) YGNodeStyleSetPositionPercent(y, edges[k], i.value * 100.0f);
    }
    for (int k = 0; k < 2; k++) {
        YGGutter g = k == 0 ? YGGutterColumn : YGGutterRow;
        if (n.gap[k].kind == LEN_PCT && n.gap[k].value != 0) YGNodeStyleSetGapPercent(y, g, n.gap[k].value * 100.0f);
        else if (n.gap[k].value != 0) YGNodeStyleSetGap(y, g, n.gap[k].value);
    }
}

struct Built {
    YGNodeRef root;
    std::vector<YGNodeRef> nodes;
};

static Built build(const Tree &t, YGConfigRef config) {
    Built b;
    b.nodes.resize((size_t)t.count);
    for (int i = 0; i < t.count; i++) {
        YGNodeRef y = YGNodeNewWithConfig(config);
        style(y, t.nodes[i]);
        if (t.nodes[i].text > 0 && t.nodes[i].child_count == 0) {
            YGNodeSetContext(y, (void *)(uintptr_t)t.nodes[i].text);
            YGNodeSetMeasureFunc(y, measure);
        }
        b.nodes[(size_t)i] = y;
        if (t.nodes[i].parent >= 0) {
            YGNodeRef p = b.nodes[(size_t)t.nodes[i].parent];
            YGNodeInsertChild(p, y, YGNodeGetChildCount(p));
        }
    }
    b.root = b.nodes[0];
    return b;
}

// The root's height is its content's, as for a CSS root with `height: auto`: Yoga would make a
// definite owner height the root's exact height.
static void layout(const Tree &, YGNodeRef root, float width) {
    YGNodeCalculateLayout(root, width, YGUndefined, YGDirectionLTR);
}

static float viewport_width(const Tree &t) { return isnan(t.viewport[0]) ? YGUndefined : t.viewport[0]; }

// Reads every node's position, as the Rust suite does under CEPHAL_BENCH_READ, so the cost of
// getting layouts out of the engine is timed alike.
static bool g_read = getenv("CEPHAL_BENCH_READ") != nullptr;

static void read_all(const std::vector<YGNodeRef> &nodes) {
    if (!g_read) return;
    float sum = 0;
    for (YGNodeRef y : nodes) sum += YGNodeLayoutGetLeft(y);
    volatile float sink = sum;
    (void)sink;
}

static void run(const Tree &t, double secs, bool rounding, FILE *csv) {
    std::string why = unsupported(t);
    if (!why.empty()) {
        fprintf(stderr, "%-34s %7d  yoga: skipped (%s)\n", t.workload, t.size, why.c_str());
        return;
    }
    YGConfigRef exact = YGConfigNew();
    YGConfigSetErrata(exact, YGErrataNone);
    YGConfigSetPointScaleFactor(exact, 0.0f);
    YGConfigRef config = YGConfigNew();
    YGConfigSetErrata(config, YGErrataNone);
    YGConfigSetPointScaleFactor(config, rounding ? 1.0f : 0.0f);

    // Agreement and measure count, unrounded.
    Built b = build(t, exact);
    g_measures = 0;
    layout(t, b.root, viewport_width(t));
    long measures = g_measures;
    bool same = true, close = true;
    float max_err = 0;
    for (int i = 0; i < t.count; i++) {
        YGNodeRef y = b.nodes[(size_t)i];
        float got[4] = {YGNodeLayoutGetLeft(y), YGNodeLayoutGetTop(y), YGNodeLayoutGetWidth(y), YGNodeLayoutGetHeight(y)};
        if (strcmp(t.nodes[i].display, "none") == 0) continue;
        compare_box(t.nodes[i].box, got, &same, &close, &max_err);
    }
    YGNodeFreeRecursive(b.root);
    const char *agree = same ? "exact" : close ? "close" : "differs";

    // Cold: every node dirtied (untimed) by toggling its direction between `inherit` and `ltr`, which
    // changes no result, as cephal's `clear_layout` and taffy's `mark_dirty` empty their caches on
    // a tree that keeps its memory. Build: a fresh tree per sample, construction included.
    bool rtl = false;
    for (int i = 0; i < t.count; i++) rtl |= strcmp(t.nodes[i].direction, "rtl") == 0;
    Built warm = build(t, config);
    std::vector<double> samples;
    double begin = now_ns();
    for (int warmup = 0; (now_ns() - begin) < secs * 1e9 || samples.size() < 10; warmup++) {
        if (rtl) {
            YGNodeFreeRecursive(warm.root);
            warm = build(t, config);
        } else {
            YGDirection d = warmup % 2 ? YGDirectionLTR : YGDirectionInherit;
            for (YGNodeRef y : warm.nodes) YGNodeStyleSetDirection(y, d);
        }
        double start = now_ns();
        layout(t, warm.root, viewport_width(t));
        read_all(warm.nodes);
        double ns = now_ns() - start;
        if (warmup >= 3) samples.push_back(ns);
    }
    YGNodeFreeRecursive(warm.root);
    std::vector<double> builds;
    begin = now_ns();
    for (int warmup = 0; (now_ns() - begin) < secs * 1e9 || builds.size() < 10; warmup++) {
        double start = now_ns();
        Built c = build(t, config);
        layout(t, c.root, viewport_width(t));
        double ns = now_ns() - start;
        YGNodeFreeRecursive(c.root);
        if (warmup >= 3) builds.push_back(ns);
    }
    Stats bs = summarise(builds.data(), (int)builds.size());
    Stats s = summarise(samples.data(), (int)samples.size());
    printf("%-34s %7d %7d %-16s %-15s %10.4f ms %9.1f ns/node  [%.4f..%.4f] %ld measures, %s (max %.3f)\n", t.workload, t.size,
           t.count, "yoga", "cold", s.median / 1e6, s.median / t.count, s.p10 / 1e6, s.p90 / 1e6, measures, agree, max_err);
    if (csv) csv_row(csv, &t, "yoga", "cold", "-", s, agree, max_err, measures);
    printf("%-34s %7d %7d %-16s %-15s %10.4f ms %9.1f ns/node  [%.4f..%.4f]\n", t.workload, t.size, t.count, "yoga", "build",
           bs.median / 1e6, bs.median / t.count, bs.p10 / 1e6, bs.p90 / 1e6);
    if (csv) csv_row(csv, &t, "yoga", "build", "-", bs, "-", 0, -1);

    // Incremental scenarios: one site toggled on and back off before the next.
    for (int k = 0; k < t.scenario_count; k++) {
        const Scenario &sc = t.scenarios[k];
        bool text = strcmp(sc.name, "edit_text") == 0, width = strcmp(sc.name, "hover_width") == 0,
             display = strcmp(sc.name, "toggle_display") == 0, self = strcmp(sc.name, "align_self") == 0,
             inset = strcmp(sc.name, "relative_inset") == 0, resize = strcmp(sc.name, "resize_root") == 0,
             idle = strcmp(sc.name, "idle") == 0;
        if (!(text || width || display || self || inset || resize || idle) || sc.site_count == 0) continue;
        Built c = build(t, config);
        layout(t, c.root, viewport_width(t));
        std::vector<double> frames;
        unsigned frame = 0;
        double begin = now_ns();
        while ((now_ns() - begin) < secs * 1e9 || frames.size() < 10) {
            frame++;
            bool on = frame % 2 == 1;
            int site = sc.sites[(frame / 2) % (unsigned)sc.site_count];
            const Node &n = t.nodes[site];
            YGNodeRef y = c.nodes[(size_t)site];
            float w = viewport_width(t);
            double start = now_ns();
            if (text) {
                YGNodeSetContext(y, (void *)(uintptr_t)(n.text + (on ? 4 : 0)));
                YGNodeMarkDirty(y);
            } else if (width) {
                if (on) YGNodeStyleSetWidth(y, 41.0f);
                else SET_LEN(y, n.width, YGNodeStyleSetWidth, YGNodeStyleSetWidthPercent, YGNodeStyleSetWidthAuto);
            } else if (display) {
                YGNodeStyleSetDisplay(y, on ? YGDisplayNone : YGDisplayFlex);
            } else if (self) {
                YGNodeStyleSetAlignSelf(y, on ? YGAlignCenter : align(n.align_self, YGAlignAuto));
            } else if (inset) {
                if (on) YGNodeStyleSetPosition(y, YGEdgeLeft, 5.0f);
                else YGNodeStyleSetPositionAuto(y, YGEdgeLeft);
            } else if (resize) {
                w = (isnan(t.viewport[0]) ? 1000.0f : t.viewport[0]) + (on ? 100.0f : 0.0f);
            }
            layout(t, c.root, w);
            read_all(c.nodes);
            double ns = now_ns() - start;
            if (frame > 3) frames.push_back(ns);
        }
        YGNodeFreeRecursive(c.root);
        Stats f = summarise(frames.data(), (int)frames.size());
        printf("%-34s %7d %7d %-16s %-15s %10.4f ms %9.1f ns/node  [%.4f..%.4f]\n", t.workload, t.size, t.count, "yoga", sc.name,
               f.median / 1e6, f.median / t.count, f.p10 / 1e6, f.p90 / 1e6);
        if (csv) csv_row(csv, &t, "yoga", "incremental", sc.name, f, "-", 0, -1);
    }
    YGConfigFree(exact);
    YGConfigFree(config);
}

int main(int argc, char **argv) {
    double secs = 1.0;
    bool rounding = true;
    FILE *csv = NULL;
    std::vector<const char *> files;
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--secs") == 0 && i + 1 < argc) secs = atof(argv[++i]);
        else if (strcmp(argv[i], "--csv") == 0 && i + 1 < argc) csv = fopen(argv[++i], "a");
        else if (strcmp(argv[i], "--no-rounding") == 0) rounding = false;
        else files.push_back(argv[i]);
    }
    for (const char *path : files) {
        Tree t;
        if (!read_tree(path, &t)) return 1;
        run(t, secs, rounding, csv);
        free(t.nodes);
    }
    if (csv) fclose(csv);
    return 0;
}
