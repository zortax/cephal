/* Clay on the suite's workloads that Clay can express: cold frames, checked against cephal.
 *
 *   clay_bench [--secs S] [--csv out.csv] trees/*.tree
 *
 * Clay is immediate-mode: a frame declares every element and lays them out, so a sample times
 * Clay_BeginLayout, the declarations and Clay_EndLayout together. Its model is a subset of flexbox:
 * row or column, FIXED/FIT/GROW/PERCENT sizing with min/max, padding and a main-axis gap, no wrap,
 * no margins, no shrinking of fixed sizes and equal-weight growth. A tree that needs anything else is
 * skipped with the reason; the agreement column shows where Clay's semantics still differ from CSS.
 */
#define CLAY_IMPLEMENTATION
#include "clay.h"
#include "tree.h"

/* CEPHAL_BENCH_READ: read every box inside the timed frame, as the Rust suite does. */
static bool g_read;

static Clay_Dimensions clay_measure_text(Clay_StringSlice text, Clay_TextElementConfig *config, void *user) {
    (void)text;
    (void)config;
    (void)user;
    return (Clay_Dimensions){0, 0};
}

static void on_error(Clay_ErrorData error) {
    fprintf(stderr, "clay: %.*s\n", (int)error.errorText.length, error.errorText.chars);
}

static bool is_row(const Node *n) { return strcmp(n->flex_direction, "row") == 0; }

static bool whole(float v) { return v >= 0 && v <= 65535 && floorf(v) == v; }

/* Clay's sizing of one axis of `n`; `why` is set when Clay cannot express it. */
static Clay_SizingAxis axis(const Tree *t, const Node *n, bool x, const char **why) {
    Clay_SizingAxis a;
    memset(&a, 0, sizeof a);
    const Len *size = x ? &n->width : &n->height;
    const Len *min = x ? &n->min_width : &n->min_height;
    const Len *max = x ? &n->max_width : &n->max_height;
    if (min->kind == LEN_PCT || max->kind == LEN_PCT) *why = "percent min/max";
    float lo = min->kind == LEN_PX ? min->value : 0, hi = max->kind == LEN_PX ? max->value : 0;
    const Node *parent = n->parent >= 0 ? &t->nodes[n->parent] : NULL;
    bool main_axis = parent && is_row(parent) == x;
    if (parent && main_axis && n->flex_grow > 0) {
        if (n->flex_basis.kind != LEN_PX || n->flex_basis.value != 0) *why = "grow from a non-zero basis";
        a.type = CLAY__SIZING_TYPE_GROW;
        a.size.minMax = (Clay_SizingMinMax){lo, hi};
        return a;
    }
    if (size->kind == LEN_PX) {
        if (min->kind != LEN_AUTO || max->kind != LEN_AUTO) *why = "min/max on a fixed size";
        a.type = CLAY__SIZING_TYPE_FIXED;
        a.size.minMax = (Clay_SizingMinMax){size->value, size->value};
    } else if (size->kind == LEN_PCT) {
        a.type = CLAY__SIZING_TYPE_PERCENT;
        a.size.percent = size->value;
    } else if (!parent && !isnan(t->viewport[x ? 0 : 1]) && x) {
        /* A block root fills a definite viewport's width. */
        a.type = CLAY__SIZING_TYPE_FIXED;
        a.size.minMax = (Clay_SizingMinMax){t->viewport[0], t->viewport[0]};
    } else if (parent && !main_axis && strcmp(parent->align_items, "normal") == 0 && strcmp(n->align_self, "auto") == 0) {
        /* Stretched to the line, which a single line makes the container's cross size. */
        a.type = CLAY__SIZING_TYPE_GROW;
        a.size.minMax = (Clay_SizingMinMax){lo, hi};
    } else {
        a.type = CLAY__SIZING_TYPE_FIT;
        a.size.minMax = (Clay_SizingMinMax){lo, hi};
    }
    return a;
}

/* Why Clay cannot lay out the tree, or NULL; fills `decls`. */
static const char *declare(const Tree *t, Clay_ElementDeclaration *decls) {
    const char *why = NULL;
    for (int i = 0; i < t->count && !why; i++) {
        const Node *n = &t->nodes[i];
        Clay_ElementDeclaration *d = &decls[i];
        memset(d, 0, sizeof *d);
        d->id = Clay__HashString(CLAY_STRING("n"), (uint32_t)i, 0);
        /* A box Clay reports through a render command, so timed reads read every box. */
        if (g_read) d->backgroundColor = (Clay_Color){255, 255, 255, 255};
        if (n->grid) return "grid";
        if (n->text) return "text";
        if (n->child_count > 0 && strcmp(n->display, "flex") != 0) return "non-flex container";
        if (strcmp(n->display, "none") == 0) return "display none";
        if (strcmp(n->position, "relative") != 0) return "absolute";
        if (strcmp(n->float_, "none") != 0) return "float";
        if (strcmp(n->direction, "ltr") != 0) return "rtl";
        if (strcmp(n->box_sizing, "borderbox") != 0) return "content-box";
        if (strcmp(n->flex_wrap, "nowrap") != 0) return "wrap";
        if (strcmp(n->flex_direction, "row") != 0 && strcmp(n->flex_direction, "column") != 0) return "reversed direction";
        if (n->aspect_ratio > 0) return "aspect-ratio";
        if (strcmp(n->align_self, "auto") != 0) return "align-self";
        for (int k = 0; k < 4; k++) {
            if (n->margin[k].kind != LEN_PX || n->margin[k].value != 0) return "margin";
            if (n->border[k].value != 0) return "border";
            if (n->padding[k].kind != LEN_PX || !whole(n->padding[k].value)) return "padding";
        }
        d->layout.layoutDirection = is_row(n) ? CLAY_LEFT_TO_RIGHT : CLAY_TOP_TO_BOTTOM;
        d->layout.padding = (Clay_Padding){(uint16_t)n->padding[3].value, (uint16_t)n->padding[1].value, (uint16_t)n->padding[0].value,
                                           (uint16_t)n->padding[2].value};
        const Len *gap = is_row(n) ? &n->gap[0] : &n->gap[1];
        if (gap->kind != LEN_PX || !whole(gap->value)) return "gap";
        d->layout.childGap = (uint16_t)gap->value;
        const char *jc = n->justify_content, *ai = n->align_items;
        int main_align = strcmp(jc, "normal") == 0 || strcmp(jc, "flexstart") == 0 || strcmp(jc, "start") == 0 ? 0
                         : strcmp(jc, "center") == 0 ? 1
                         : strcmp(jc, "flexend") == 0 || strcmp(jc, "end") == 0 ? 2 : -1;
        int cross_align = strcmp(ai, "normal") == 0 || strcmp(ai, "stretch") == 0 || strcmp(ai, "flexstart") == 0 || strcmp(ai, "start") == 0 ? 0
                          : strcmp(ai, "center") == 0 ? 1
                          : strcmp(ai, "flexend") == 0 || strcmp(ai, "end") == 0 ? 2 : -1;
        if (main_align < 0) return "justify-content";
        if (cross_align < 0) return "align-items";
        int ax = is_row(n) ? main_align : cross_align, ay = is_row(n) ? cross_align : main_align;
        d->layout.childAlignment.x = ax == 0 ? CLAY_ALIGN_X_LEFT : ax == 1 ? CLAY_ALIGN_X_CENTER : CLAY_ALIGN_X_RIGHT;
        d->layout.childAlignment.y = ay == 0 ? CLAY_ALIGN_Y_TOP : ay == 1 ? CLAY_ALIGN_Y_CENTER : CLAY_ALIGN_Y_BOTTOM;
        d->layout.sizing.width = axis(t, n, true, &why);
        d->layout.sizing.height = axis(t, n, false, &why);
    }
    /* Growing siblings share space equally in Clay; CSS weights them. */
    for (int i = 0; i < t->count && !why; i++) {
        float weight = 0;
        for (int c = t->nodes[i].first_child; c >= 0; c = t->nodes[c].next_sibling) {
            float g = t->nodes[c].flex_grow;
            if (g > 0 && weight > 0 && g != weight) return "unequal grow weights";
            if (g > 0) weight = g;
        }
    }
    return why;
}

static void frame(const Tree *t, const Clay_ElementDeclaration *decls, int *stack) {
    Clay_BeginLayout();
    /* Depth-first: open a node, its children in order, then close it. */
    int top = 0;
    stack[top++] = 0;
    Clay__OpenElement();
    Clay__ConfigureOpenElementPtr(&decls[0]);
    int at = t->nodes[0].first_child;
    while (top > 0) {
        if (at >= 0) {
            Clay__OpenElement();
            Clay__ConfigureOpenElementPtr(&decls[at]);
            stack[top++] = at;
            at = t->nodes[at].first_child;
        } else {
            Clay__CloseElement();
            int done = stack[--top];
            at = top > 0 ? t->nodes[done].next_sibling : -1;
        }
    }
    Clay_RenderCommandArray commands = Clay_EndLayout();
    /* Under CEPHAL_BENCH_READ, read every box as the Rust suite reads every layout. */
    if (g_read) {
        float sum = 0;
        for (int32_t i = 0; i < commands.length; i++) sum += commands.internalArray[i].boundingBox.x;
        volatile float sink = sum;
        (void)sink;
    }
}

static void run(const Tree *t, double secs, FILE *csv) {
    Clay_ElementDeclaration *decls = calloc((size_t)t->count, sizeof *decls);
    const char *why = declare(t, decls);
    if (why) {
        fprintf(stderr, "%-34s %7d  clay: skipped (%s)\n", t->workload, t->size, why);
        free(decls);
        return;
    }
    Clay_SetMaxElementCount(t->count + 16);
    uint32_t bytes = Clay_MinMemorySize();
    void *memory = malloc(bytes);
    Clay_Arena arena = Clay_CreateArenaWithCapacityAndMemory(bytes, memory);
    float vw = isnan(t->viewport[0]) ? 1e7f : t->viewport[0], vh = isnan(t->viewport[1]) ? 1e7f : t->viewport[1];
    Clay_Initialize(arena, (Clay_Dimensions){vw, vh}, (Clay_ErrorHandler){on_error, 0});
    Clay_SetMeasureTextFunction(clay_measure_text, NULL);
    Clay_SetCullingEnabled(false);
    int *stack = malloc((size_t)t->count * sizeof(int));

    /* Agreement: Clay reports absolute boxes; cephal's are parent-relative. */
    frame(t, decls, stack);
    float *abs_xy = malloc((size_t)t->count * 2 * sizeof(float));
    bool same = true, close = true;
    float max_err = 0;
    for (int i = 0; i < t->count; i++) {
        const Node *n = &t->nodes[i];
        float px = n->parent >= 0 ? abs_xy[2 * n->parent] : 0, py = n->parent >= 0 ? abs_xy[2 * n->parent + 1] : 0;
        abs_xy[2 * i] = px + n->box[0];
        abs_xy[2 * i + 1] = py + n->box[1];
        float expected[4] = {abs_xy[2 * i], abs_xy[2 * i + 1], n->box[2], n->box[3]};
        Clay_ElementData e = Clay_GetElementData(decls[i].id);
        float got[4] = {e.boundingBox.x, e.boundingBox.y, e.boundingBox.width, e.boundingBox.height};
        if (!e.found) got[0] = got[1] = got[2] = got[3] = NAN;
        compare_box(expected, got, &same, &close, &max_err);
        if (!e.found) close = same = false;
    }
    free(abs_xy);
    const char *agree = same ? "exact" : close ? "close" : "differs";

    double *samples = malloc(sizeof(double) * 1000000);
    int n = 0;
    double begin = now_ns();
    for (int warm = 0; ((now_ns() - begin) < secs * 1e9 || n < 10) && n < 1000000; warm++) {
        double start = now_ns();
        frame(t, decls, stack);
        double ns = now_ns() - start;
        if (warm >= 3) samples[n++] = ns;
    }
    Stats s = summarise(samples, n);
    printf("%-34s %7d %7d %-16s %-15s %10.4f ms %9.1f ns/node  [%.4f..%.4f] %s (max %.3f)\n", t->workload, t->size, t->count, "clay",
           "cold", s.median / 1e6, s.median / t->count, s.p10 / 1e6, s.p90 / 1e6, agree, max_err);
    if (csv) csv_row(csv, t, "clay", "cold", "-", s, agree, max_err, -1);
    free(samples);
    free(stack);
    /* The context lives in the arena: forget it before the memory goes. */
    Clay_SetCurrentContext(NULL);
    free(memory);
    free(decls);
}

int main(int argc, char **argv) {
    g_read = getenv("CEPHAL_BENCH_READ") != NULL;
    double secs = 1.0;
    FILE *csv = NULL;
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--secs") == 0 && i + 1 < argc) {
            secs = atof(argv[++i]);
            continue;
        }
        if (strcmp(argv[i], "--csv") == 0 && i + 1 < argc) {
            csv = fopen(argv[++i], "a");
            continue;
        }
        if (strcmp(argv[i], "--no-rounding") == 0) continue;
        Tree t;
        if (!read_tree(argv[i], &t)) return 1;
        run(&t, secs, csv);
        free(t.nodes);
    }
    if (csv) fclose(csv);
    return 0;
}
