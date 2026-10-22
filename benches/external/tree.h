/* Reads the trees `CEPHAL_BENCH_DUMP` writes (see benches/suite.rs `dump`), for the external engines.
 * Plain C, shared by the Yoga (C++) and Clay (C) harnesses. */
#ifndef CEPHAL_TREE_H
#define CEPHAL_TREE_H

#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef enum { LEN_AUTO, LEN_PX, LEN_PCT, LEN_UNSUPPORTED } LenKind;
typedef struct {
    LenKind kind;
    float value; /* px, or percent / 100 */
} Len;

typedef struct {
    int parent; /* -1 for the root */
    int first_child, next_sibling, child_count;
    uint32_t text; /* glyphs of 10px; 0 for none */
    char display[16], position[16], box_sizing[16], direction[8], overflow[16];
    char flex_direction[16], flex_wrap[16], align_items[16], align_self[16];
    char justify_content[16], align_content[16], float_[8];
    Len width, height, min_width, min_height, max_width, max_height, flex_basis;
    Len margin[4], inset[4], padding[4], border[4]; /* top, right, bottom, left */
    Len gap[2];                                     /* column, row */
    float flex_grow, flex_shrink, aspect_ratio;     /* aspect_ratio 0: auto */
    bool grid;
    float box[4]; /* cephal's unrounded x, y, width, height, parent-relative */
} Node;

typedef struct {
    char name[128];
    char kind[64];
    int sites[64];
    int site_count;
} Scenario;

typedef struct {
    char workload[128];
    char family[128];
    int size;
    char tags[256];
    float viewport[2]; /* NAN: max-content */
    Node *nodes;
    int count;
    Scenario scenarios[16];
    int scenario_count;
} Tree;

static Len parse_len(const char *s) {
    Len l = {LEN_AUTO, 0};
    if (strcmp(s, "auto") == 0) return l;
    size_t n = strlen(s);
    if (n > 2 && strcmp(s + n - 2, "px") == 0) {
        l.kind = LEN_PX;
        l.value = strtof(s, NULL);
    } else if (n > 1 && s[n - 1] == '%') {
        l.kind = LEN_PCT;
        l.value = strtof(s, NULL) / 100.0f;
    } else {
        l.kind = LEN_UNSUPPORTED;
    }
    return l;
}

/* Splits `v` on commas into up to `max` lengths. */
static void parse_lens(const char *v, Len *out, int max) {
    char buf[256];
    snprintf(buf, sizeof buf, "%s", v);
    char *save = NULL;
    char *tok = strtok_r(buf, ",", &save);
    for (int i = 0; i < max && tok; i++, tok = strtok_r(NULL, ",", &save)) out[i] = parse_len(tok);
}

static void set_field(Node *n, const char *key, const char *v) {
#define STR(k, f) if (strcmp(key, k) == 0) { snprintf(n->f, sizeof n->f, "%s", v); return; }
#define LEN(k, f) if (strcmp(key, k) == 0) { n->f = parse_len(v); return; }
    STR("display", display) STR("position", position) STR("box-sizing", box_sizing) STR("direction", direction)
    STR("overflow", overflow) STR("flex-direction", flex_direction) STR("flex-wrap", flex_wrap)
    STR("align-items", align_items) STR("align-self", align_self) STR("justify-content", justify_content)
    STR("align-content", align_content) STR("float", float_)
    LEN("width", width) LEN("height", height) LEN("min-width", min_width) LEN("min-height", min_height)
    LEN("max-width", max_width) LEN("max-height", max_height) LEN("flex-basis", flex_basis)
#undef STR
#undef LEN
    if (strcmp(key, "text") == 0) n->text = (uint32_t)strtoul(v, NULL, 10);
    else if (strcmp(key, "flex-grow") == 0) n->flex_grow = strtof(v, NULL);
    else if (strcmp(key, "flex-shrink") == 0) n->flex_shrink = strtof(v, NULL);
    else if (strcmp(key, "aspect-ratio") == 0) n->aspect_ratio = strcmp(v, "auto") == 0 ? 0.0f : strtof(v, NULL);
    else if (strcmp(key, "margin") == 0) parse_lens(v, n->margin, 4);
    else if (strcmp(key, "inset") == 0) parse_lens(v, n->inset, 4);
    else if (strcmp(key, "padding") == 0) parse_lens(v, n->padding, 4);
    else if (strcmp(key, "border") == 0) parse_lens(v, n->border, 4);
    else if (strcmp(key, "gap") == 0) parse_lens(v, n->gap, 2);
    else if (strcmp(key, "grid") == 0) n->grid = true;
    else if (strcmp(key, "box") == 0) sscanf(v, "%f,%f,%f,%f", &n->box[0], &n->box[1], &n->box[2], &n->box[3]);
}

static float parse_space(const char *s) { return strcmp(s, "max-content") == 0 ? NAN : strtof(s, NULL); }

/* Reads a tree; returns false (with a message on stderr) if the file is malformed. */
static bool read_tree(const char *path, Tree *t) {
    FILE *f = fopen(path, "r");
    if (!f) {
        fprintf(stderr, "cannot open %s\n", path);
        return false;
    }
    memset(t, 0, sizeof *t);
    int cap = 1024;
    t->nodes = (Node *)calloc((size_t)cap, sizeof(Node));
    char *line = NULL;
    size_t len = 0;
    while (getline(&line, &len, f) > 0) {
        line[strcspn(line, "\n")] = 0;
        if (strncmp(line, "workload ", 9) == 0) {
            sscanf(line + 9, "%127s %d %127s", t->workload, &t->size, t->family);
        } else if (strncmp(line, "tags ", 5) == 0) {
            snprintf(t->tags, sizeof t->tags, "%s", line + 5);
        } else if (strncmp(line, "viewport ", 9) == 0) {
            char a[32], b[32];
            sscanf(line + 9, "%31s %31s", a, b);
            t->viewport[0] = parse_space(a);
            t->viewport[1] = parse_space(b);
        } else if (strncmp(line, "node ", 5) == 0) {
            if (t->count == cap) {
                cap *= 2;
                t->nodes = (Node *)realloc(t->nodes, (size_t)cap * sizeof(Node));
                memset(t->nodes + t->count, 0, (size_t)(cap - t->count) * sizeof(Node));
            }
            Node *n = &t->nodes[t->count];
            char *save = NULL;
            char *tok = strtok_r(line + 5, " ", &save);
            int index = atoi(tok);
            tok = strtok_r(NULL, " ", &save);
            n->parent = atoi(tok);
            n->first_child = n->next_sibling = -1;
            if (index != t->count) {
                fprintf(stderr, "%s: nodes out of order\n", path);
                return false;
            }
            while ((tok = strtok_r(NULL, " ", &save))) {
                char *eq = strchr(tok, '=');
                if (!eq) continue;
                *eq = 0;
                set_field(n, tok, eq + 1);
            }
            t->count++;
        } else if (strncmp(line, "scenario ", 9) == 0 && t->scenario_count < 16) {
            Scenario *s = &t->scenarios[t->scenario_count++];
            char ids[4096];
            ids[0] = 0;
            sscanf(line + 9, "%127s %4095s", s->name, ids);
            char *save = NULL;
            for (char *tok = strtok_r(ids, ",", &save); tok && s->site_count < 64; tok = strtok_r(NULL, ",", &save))
                s->sites[s->site_count++] = atoi(tok);
        }
    }
    free(line);
    fclose(f);
    /* Children in order: walk backwards so prepending keeps document order. */
    for (int i = t->count - 1; i > 0; i--) {
        Node *p = &t->nodes[t->nodes[i].parent];
        t->nodes[i].next_sibling = p->first_child;
        p->first_child = i;
        p->child_count++;
    }
    return t->count > 0;
}

/* The text measurement of tests/common `measure`: `n` glyphs of 10px that wrap at the available
 * width. `known` and `available` use NAN for none / max-content and -INFINITY for min-content. */
static void measure_text(uint32_t glyphs, float known_w, float known_h, float avail_w, float *out_w, float *out_h) {
    const float glyph = 10.0f;
    float total = (float)glyphs * glyph;
    float width;
    if (!isnan(known_w)) width = known_w;
    else if (avail_w == -INFINITY) width = glyph;
    else if (isnan(avail_w)) width = total;
    else width = fmaxf(fminf(avail_w, total), glyph);
    float per_line = fmaxf(floorf(width / glyph), 1.0f);
    float lines = fmaxf(ceilf((float)glyphs / per_line), 1.0f);
    *out_w = width;
    *out_h = isnan(known_h) ? lines * glyph : known_h;
}

static double now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

static int cmp_double(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

typedef struct {
    double median, p10, p90;
    int samples;
} Stats;

/* Sorts `v` and summarises it. */
static Stats summarise(double *v, int n) {
    qsort(v, (size_t)n, sizeof(double), cmp_double);
    Stats s;
    s.median = v[(int)lround((n - 1) * 0.5)];
    s.p10 = v[(int)lround((n - 1) * 0.1)];
    s.p90 = v[(int)lround((n - 1) * 0.9)];
    s.samples = n;
    return s;
}

/* Agreement of one box with cephal's, as in the Rust suite: `close` within 0.01 relative. */
static void compare_box(const float *expected, const float *got, bool *exact, bool *close, float *max_err) {
    for (int k = 0; k < 4; k++) {
        float p = expected[k], q = got[k];
        float d = fabsf(p - q);
        if (d > *max_err) *max_err = d;
        if (p != q) *exact = false;
        if (d > 0.01f * (1.0f + fmaxf(fabsf(p), fabsf(q)))) *close = false;
    }
}

/* One CSV row in the suite's format. */
static void csv_row(FILE *out, const Tree *t, const char *engine, const char *mode, const char *scenario, Stats s,
                    const char *agree, float max_err, long measures) {
    char measures_s[32] = "";
    if (measures >= 0) snprintf(measures_s, sizeof measures_s, "%ld", measures);
    fprintf(out, "%s,%s,%s,%d,%d,%s,%s,%s,%.3f,%.6f,%.6f,%.6f,%d,%s,%e,%s\n", t->workload, t->family, t->tags, t->size,
            t->count, engine, mode, scenario, s.median / t->count, s.median / 1e6, s.p10 / 1e6, s.p90 / 1e6, s.samples,
            agree, (double)max_err, measures_s);
    fflush(out);
}

#endif
