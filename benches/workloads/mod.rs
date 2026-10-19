//! The benchmark workloads, shared by the suite and the external-engine dump.
//!
//! A workload builds one tree at a given size. Styles are written as CSS declarations (see
//! [`css`]), so a workload reads like the page it stands for and maps one to one onto other engines.
//! Tags say what a workload is for:
//!
//! - `cephal`/`taffy`: shapes expected to favour that engine (`cephal`: repeated intrinsic questions,
//!   canonical keys, incremental edits, parallel subtrees; `taffy`: per-call overhead, one-shot builds,
//!   uncacheable measurement);
//! - `flex`, `grid`, `block`, `mixed`, `text` (measured leaves);
//! - `hit:<path>`/`miss:<reason>`: takes, or is built to miss, one of cephal's fast paths, in pairs that
//!   differ by one property;
//! - `pangui`: one of PanGui's published workloads.
#![allow(dead_code)]

use crate::common::{Rng, Text, build_subtree};
use cephal::Tree;
use cephal::geometry::{Line, Rect, Size};
use cephal::style::parse::*;
use cephal::style::Style;
use cephal::tree::NodeId;

pub struct Workload {
    pub name: &'static str,
    /// Workloads of one family share a builder and scale along `sizes`.
    pub family: &'static str,
    pub tags: &'static [&'static str],
    /// Sizes the suite runs by default, in the builder's own unit (usually leaves).
    pub sizes: &'static [usize],
    /// Definite viewport width and height; `None` is max-content.
    pub viewport: (Option<f32>, Option<f32>),
    pub build: fn(&mut Tree<Text>, usize) -> NodeId,
    /// Incremental scenarios on this workload.
    pub scenarios: &'static [Scenario],
}

/// One kind of edit, applied to a few sites spread through the tree; frames toggle one site at a
/// time between two states and rotate through the sites.
#[derive(Clone, Copy, Debug)]
pub enum Scenario {
    /// A measured leaf's text grows and shrinks.
    EditText,
    /// A measured leaf's width changes.
    HoverWidth,
    /// A container toggles `display: none`.
    ToggleDisplay,
    /// A child's `align-self` changes, which only its parent reads.
    AlignSelf,
    /// A relatively positioned box's `left` changes.
    RelativeInset,
    /// An absolutely positioned box moves.
    MoveAbsolute,
    /// A row is appended to a container and removed again.
    InsertRemove,
    /// The viewport width changes.
    ResizeRoot,
    /// Nothing changes.
    Idle,
}

impl Scenario {
    pub fn name(self) -> &'static str {
        match self {
            Self::EditText => "edit_text",
            Self::HoverWidth => "hover_width",
            Self::ToggleDisplay => "toggle_display",
            Self::AlignSelf => "align_self",
            Self::RelativeInset => "relative_inset",
            Self::MoveAbsolute => "move_absolute",
            Self::InsertRemove => "insert_remove",
            Self::ResizeRoot => "resize_root",
            Self::Idle => "idle",
        }
    }
}

const ALL_EDITS: &[Scenario] = &[
    Scenario::EditText,
    Scenario::HoverWidth,
    Scenario::ToggleDisplay,
    Scenario::AlignSelf,
    Scenario::RelativeInset,
    Scenario::MoveAbsolute,
    Scenario::InsertRemove,
    Scenario::ResizeRoot,
    Scenario::Idle,
];

/// One to four values, as the `margin`/`padding` shorthands take them: top, right, bottom, left.
fn sides(v: &str) -> [&str; 4] {
    let w: Vec<&str> = v.split_whitespace().collect();
    match w.as_slice() {
        [a] => [a, a, a, a],
        [y, x] => [y, x, y, x],
        [t, r, b, l] => [t, r, b, l],
        _ => bad("sides", v),
    }
}

fn bad<T>(prop: &str, v: &str) -> T {
    panic!("cannot parse {prop}: {v}")
}

/// A style from CSS declarations, `prop: value; ...`. Only what the workloads use is understood;
/// anything else panics, so a typo cannot silently change a benchmark.
pub fn css(t: &mut Tree<Text>, decls: &str) -> Style {
    let mut s = Style::DEFAULT;
    for decl in decls.split(';').map(str::trim).filter(|d| !d.is_empty()) {
        let (prop, value) = decl.split_once(':').unwrap_or_else(|| panic!("bad declaration {decl:?}"));
        let (prop, v) = (prop.trim(), value.trim());
        let lpa = |v: &str| parse_length_percentage_auto(v).unwrap_or_else(|| bad(prop, v));
        let lp = |v: &str| parse_length_percentage(v).unwrap_or_else(|| bad(prop, v));
        let dim = |v: &str| parse_dimension(v).unwrap_or_else(|| bad(prop, v));
        match prop {
            "display" => s.display = parse_display(v).unwrap_or_else(|| bad(prop, v)),
            "position" => s.position = parse_position(v).unwrap_or_else(|| bad(prop, v)),
            "box-sizing" => s.box_sizing = parse_box_sizing(v).unwrap_or_else(|| bad(prop, v)),
            "overflow" => {
                let o = parse_overflow(v).unwrap_or_else(|| bad(prop, v));
                s.overflow = cephal::geometry::Point { x: o, y: o };
            }
            "float" => s.float = parse_float(v).unwrap_or_else(|| bad(prop, v)),
            "clear" => s.clear = parse_clear(v).unwrap_or_else(|| bad(prop, v)),
            "width" => s.size.width = dim(v),
            "height" => s.size.height = dim(v),
            "min-width" => s.min_size.width = lpa(v),
            "min-height" => s.min_size.height = lpa(v),
            "max-width" => s.max_size.width = lpa(v),
            "max-height" => s.max_size.height = lpa(v),
            "aspect-ratio" => s.aspect_ratio = Some(v.parse().unwrap_or_else(|_| bad(prop, v))),
            "margin" => {
                let [t, r, b, l] = sides(v);
                s.margin = Rect { top: lpa(t), right: lpa(r), bottom: lpa(b), left: lpa(l) };
            }
            "margin-left" => s.margin.left = lpa(v),
            "margin-top" => s.margin.top = lpa(v),
            "padding" => {
                let [t, r, b, l] = sides(v);
                s.padding = Rect { top: lp(t), right: lp(r), bottom: lp(b), left: lp(l) };
            }
            "border-width" => s.border = Rect::splat(lp(v)),
            "gap" => s.gap = Size { width: lp(v), height: lp(v) },
            "row-gap" => s.gap.height = lp(v),
            "column-gap" => s.gap.width = lp(v),
            "left" => s.inset.left = lpa(v),
            "top" => s.inset.top = lpa(v),
            "right" => s.inset.right = lpa(v),
            "bottom" => s.inset.bottom = lpa(v),
            "flex-direction" => s.flex_direction = parse_flex_direction(v).unwrap_or_else(|| bad(prop, v)),
            "flex-wrap" => s.flex_wrap = parse_flex_wrap(v).unwrap_or_else(|| bad(prop, v)),
            "flex" => {
                let w: Vec<&str> = v.split_whitespace().collect();
                let [g, sh, b] = w.as_slice() else { bad(prop, v) };
                s.flex_grow = g.parse().unwrap_or_else(|_| bad(prop, v));
                s.flex_shrink = sh.parse().unwrap_or_else(|_| bad(prop, v));
                s.flex_basis = dim(b);
            }
            "flex-grow" => s.flex_grow = v.parse().unwrap_or_else(|_| bad(prop, v)),
            "flex-shrink" => s.flex_shrink = v.parse().unwrap_or_else(|_| bad(prop, v)),
            "flex-basis" => s.flex_basis = dim(v),
            "align-items" => s.align_items = Some(parse_align_items(v).unwrap_or_else(|| bad(prop, v))),
            "align-self" => s.align_self = Some(parse_align_items(v).unwrap_or_else(|| bad(prop, v))),
            "justify-items" => s.justify_items = Some(parse_align_items(v).unwrap_or_else(|| bad(prop, v))),
            "justify-self" => s.justify_self = Some(parse_align_items(v).unwrap_or_else(|| bad(prop, v))),
            "align-content" => s.align_content = Some(parse_align_content(v).unwrap_or_else(|| bad(prop, v))),
            "justify-content" => s.justify_content = Some(parse_align_content(v).unwrap_or_else(|| bad(prop, v))),
            "grid-template-columns" | "grid-template-rows" => {
                let (tracks, names) = parse_grid_template(v, &mut |n| t.intern(n)).unwrap_or_else(|| bad(prop, v));
                let g = s.grid_mut();
                if prop.ends_with("columns") {
                    g.template_columns = tracks;
                    g.template_column_names = names;
                } else {
                    g.template_rows = tracks;
                    g.template_row_names = names;
                }
            }
            "grid-auto-rows" => s.grid_mut().auto_rows = parse_grid_auto_tracks(v).unwrap_or_else(|| bad(prop, v)),
            "grid-auto-columns" => s.grid_mut().auto_columns = parse_grid_auto_tracks(v).unwrap_or_else(|| bad(prop, v)),
            "grid-auto-flow" => s.grid_mut().auto_flow = parse_grid_auto_flow(v).unwrap_or_else(|| bad(prop, v)),
            "grid-column" | "grid-row" => {
                let (a, b) = v.split_once('/').map_or((v, "auto"), |(a, b)| (a.trim(), b.trim()));
                let mut p = |x: &str| parse_grid_placement(x, &mut |n| t.intern(n)).unwrap_or_else(|| bad(prop, v));
                let line = Line { start: p(a), end: p(b) };
                if prop == "grid-column" {
                    s.grid_column = line;
                } else {
                    s.grid_row = line;
                }
            }
            _ => panic!("unsupported property {prop}"),
        }
    }
    s
}

fn add(t: &mut Tree<Text>, parent: NodeId, style: &Style) -> NodeId {
    let n = t.new_leaf(style.clone());
    t.append_child(parent, n);
    n
}

fn text(t: &mut Tree<Text>, parent: NodeId, style: &Style, glyphs: u32) -> NodeId {
    let n = t.new_leaf_with_context(style.clone(), Text(glyphs));
    t.append_child(parent, n);
    n
}

fn root(t: &mut Tree<Text>, decls: &str) -> NodeId {
    let s = css(t, decls);
    t.new_leaf(s)
}

/// A row container of `n` copies of `child`, with `one` replacing the child in the middle.
fn row_of(t: &mut Tree<Text>, container: &str, child: &str, one: Option<&str>, n: usize) -> NodeId {
    let r = root(t, container);
    let c = css(t, child);
    let o = one.map(|d| css(t, &format!("{child}; {d}")));
    for i in 0..n {
        match (&o, i == n / 2) {
            (Some(o), true) => add(t, r, o),
            _ => add(t, r, &c),
        };
    }
    r
}

// ---- Flex -------------------------------------------------------------------------------------

const ROW: &str = "display: flex; width: 1000px; height: 100px";
const FIXED: &str = "width: 10px; height: 10px";

fn row_fixed(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, ROW, FIXED, None, n)
}
fn row_one_baseline(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, ROW, FIXED, Some("align-self: baseline"), n)
}
fn row_one_auto_margin(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, ROW, FIXED, Some("margin-left: auto"), n)
}
fn row_one_percent(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, ROW, FIXED, Some("width: 1%"), n)
}
fn row_one_aspect(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, ROW, "width: 10px", Some("aspect-ratio: 1"), n)
}
fn row_one_content_basis(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, ROW, FIXED, Some("flex-basis: content"), n)
}
fn row_wrap(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, "display: flex; flex-wrap: wrap; width: 1000px", FIXED, None, n)
}
fn row_reverse(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, "display: flex; flex-direction: row-reverse; width: 1000px; height: 100px", FIXED, None, n)
}
fn row_expand(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, ROW, "flex: 1 1 0px; min-width: 0; height: 10px", None, n)
}
fn row_empty_leaves(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, ROW, "flex: 0 1 auto", None, n)
}
fn row_measured_leaves(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, ROW);
    let c = css(t, "flex: 0 1 auto");
    for _ in 0..n {
        text(t, r, &c, 1);
    }
    r
}
fn column_stack(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, "display: flex; flex-direction: column; width: 200px; padding: 10px; gap: 5px", "height: 1px", None, n)
}
fn wrap_text(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; flex-wrap: wrap; width: 1000px");
    let c = Style::DEFAULT;
    for i in 0..n {
        text(t, r, &c, 3 + (i % 5) as u32);
    }
    r
}
fn wrap_stretch_content(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; flex-wrap: wrap; align-content: stretch; width: 1000px; height: 2000px");
    let c = css(t, "width: 37px");
    for i in 0..n {
        text(t, r, &c, 2 + (i % 4) as u32);
    }
    r
}
fn min_max_row(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; flex-direction: column; width: 500px");
    let row = css(t, "display: flex; height: 10px");
    let a = css(t, "flex: 1 1 0px; min-width: 30px; max-width: 120px");
    let b = css(t, "flex: 2 1 0px; max-width: 200px");
    let c = css(t, "flex: 1 1 0px; min-width: 150px");
    for _ in 0..n / 3 {
        let rr = add(t, r, &row);
        add(t, rr, &a);
        add(t, rr, &b);
        add(t, rr, &c);
    }
    r
}
/// Rows of auto-width columns of text: every level asks min- and max-content of the one below.
fn nested_intrinsic_rows(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; flex-direction: column; width: 1200px");
    let row = css(t, "display: flex; gap: 4px");
    let col = css(t, "display: flex; flex-direction: column; padding: 2px");
    let leaf = Style::DEFAULT;
    for i in 0..n / 16 {
        let rr = add(t, r, &row);
        for j in 0..4 {
            let cc = add(t, rr, &col);
            for k in 0..3 {
                text(t, cc, &leaf, 2 + ((i + j + k) % 9) as u32);
            }
        }
    }
    r
}

// ---- Grid -------------------------------------------------------------------------------------

fn side(n: usize) -> usize {
    (n as f64).sqrt().round().max(1.0) as usize
}

/// An `s × s` grid; `cell(t, grid, i)` adds item `i`.
fn grid(t: &mut Tree<Text>, n: usize, container: &str, cell: impl Fn(&mut Tree<Text>, NodeId, usize)) -> NodeId {
    let s = side(n);
    let g = root(t, &container.replace("{s}", &s.to_string()));
    for i in 0..s * s {
        cell(t, g, i);
    }
    g
}
fn grid_fixed(t: &mut Tree<Text>, n: usize) -> NodeId {
    let c = css(t, "width: 10px; height: 10px");
    grid(t, n, "display: grid; grid-template-columns: repeat({s}, 10px); grid-template-rows: repeat({s}, 10px)", |t, g, _| {
        add(t, g, &c);
    })
}
fn grid_one_auto_column(t: &mut Tree<Text>, n: usize) -> NodeId {
    let c = css(t, "width: 10px; height: 10px");
    grid(t, n, "display: grid; grid-template-columns: auto repeat({s}, 10px); grid-template-rows: repeat({s}, 10px)", |t, g, _| {
        add(t, g, &c);
    })
}
fn grid_auto_cheap(t: &mut Tree<Text>, n: usize) -> NodeId {
    let styles: Vec<Style> = (0..7).map(|k| css(t, &format!("width: {}px; height: 10px", 10 + k))).collect();
    grid(t, n, "display: grid; grid-template-columns: repeat({s}, auto)", |t, g, i| {
        add(t, g, &styles[i % 7]);
    })
}
fn grid_auto_text(t: &mut Tree<Text>, n: usize) -> NodeId {
    grid(t, n, "display: grid; grid-template-columns: repeat({s}, auto)", |t, g, i| {
        text(t, g, &Style::DEFAULT, 2 + (i % 7) as u32);
    })
}
fn grid_fixed_cols_auto_rows_text(t: &mut Tree<Text>, n: usize) -> NodeId {
    grid(t, n, "display: grid; grid-template-columns: repeat({s}, 60px)", |t, g, i| {
        text(t, g, &Style::DEFAULT, 2 + (i % 9) as u32);
    })
}
fn grid_mixed_fixed_auto(t: &mut Tree<Text>, n: usize) -> NodeId {
    grid(t, n, "display: grid; grid-template-columns: repeat({s}, 100px auto)", |t, g, i| {
        text(t, g, &Style::DEFAULT, 2 + (i % 7) as u32);
    })
}
fn grid_fr_definite(t: &mut Tree<Text>, n: usize) -> NodeId {
    grid(t, n, "display: grid; width: 1000px; grid-template-columns: repeat({s}, 1fr)", |t, g, i| {
        text(t, g, &Style::DEFAULT, 2 + (i % 7) as u32);
    })
}
/// `1fr` columns in a grid sized by its content: the flexible tracks need every item's contribution.
fn grid_fr_indefinite(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; align-items: flex-start");
    let g = grid(t, n, "display: grid; grid-template-columns: repeat({s}, 1fr)", |t, g, i| {
        text(t, g, &Style::DEFAULT, 2 + (i % 7) as u32);
    });
    t.append_child(r, g);
    r
}
fn grid_spanning(t: &mut Tree<Text>, n: usize) -> NodeId {
    let span = css(t, "grid-column: span 2");
    grid(t, n, "display: grid; grid-template-columns: repeat({s}, auto)", |t, g, i| {
        let style = if i % 3 == 0 { span.clone() } else { Style::DEFAULT };
        text(t, g, &style, 2 + (i % 5) as u32);
    })
}
fn grid_dense(t: &mut Tree<Text>, n: usize) -> NodeId {
    let styles: Vec<Style> = ["grid-column: span 3", "grid-column: span 2; grid-row: span 2", "", "grid-row: span 3"].iter().map(|d| css(t, d)).collect();
    grid(t, n, "display: grid; grid-auto-flow: row dense; grid-template-columns: repeat({s}, 20px); grid-auto-rows: 20px", |t, g, i| {
        add(t, g, &styles[i % 4]);
    })
}
fn grid_minmax_fit_content(t: &mut Tree<Text>, n: usize) -> NodeId {
    grid(t, n, "display: grid; width: 1200px; grid-template-columns: repeat({s}, minmax(30px, 1fr) fit-content(80px))", |t, g, i| {
        text(t, g, &Style::DEFAULT, 2 + (i % 11) as u32);
    })
}
/// Items with a fixed size whose content is a container: the known size is the contribution.
fn grid_known_size_items(t: &mut Tree<Text>, n: usize) -> NodeId {
    let item = css(t, "display: flex; width: 40px; height: 20px");
    grid(t, n / 2, "display: grid; grid-template-columns: repeat({s}, auto)", |t, g, i| {
        let it = add(t, g, &item);
        text(t, it, &Style::DEFAULT, 3 + (i % 4) as u32);
    })
}
/// Page shells in a column, each a grid placed by line numbers: header, sidebar, main, aside, footer.
fn grid_holy_grail(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; flex-direction: column; width: 1000px");
    let shell = css(t, "display: grid; grid-template-columns: 150px 1fr 120px; grid-template-rows: auto 1fr auto; min-height: 200px");
    let parts: Vec<Style> = ["grid-column: 1 / 4", "grid-column: 1; grid-row: 2", "grid-column: 2; grid-row: 2", "grid-column: 3; grid-row: 2", "grid-column: 1 / 4; grid-row: 3"]
        .iter()
        .map(|d| css(t, d))
        .collect();
    for i in 0..n / 6 {
        let s = add(t, r, &shell);
        for (k, p) in parts.iter().enumerate() {
            text(t, s, p, 3 + ((i + k) % 13) as u32);
        }
    }
    r
}
/// Grids as items of an auto-sized row: the row asks each grid for its width alone first.
fn grid_in_flex_row(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; flex-wrap: wrap; width: 1500px");
    let g = css(t, "display: grid; grid-template-columns: auto auto auto; gap: 2px");
    for i in 0..n / 10 {
        let gg = add(t, r, &g);
        for k in 0..9 {
            text(t, gg, &Style::DEFAULT, 1 + ((i + k) % 6) as u32);
        }
    }
    r
}
/// Grids as fixed-height items of a column: asked for their height with the height already known.
fn grid_known_height_in_column(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; flex-direction: column; width: 600px");
    let g = css(t, "display: grid; height: 40px; flex-shrink: 0; grid-template-columns: auto 1fr auto");
    for i in 0..n / 4 {
        let gg = add(t, r, &g);
        for k in 0..3 {
            text(t, gg, &Style::DEFAULT, 2 + ((i + k) % 8) as u32);
        }
    }
    r
}
fn nested_grids(t: &mut Tree<Text>, n: usize) -> NodeId {
    let inner = css(t, "display: grid; grid-template-columns: auto 1fr; gap: 1px");
    grid(t, n / 5, "display: grid; width: 1600px; grid-template-columns: repeat({s}, auto)", |t, g, i| {
        let gg = add(t, g, &inner);
        for k in 0..4 {
            text(t, gg, &Style::DEFAULT, 1 + ((i + k) % 5) as u32);
        }
    })
}

// ---- Block ------------------------------------------------------------------------------------

fn block_stack(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, "width: 1000px", "height: 10px", None, n)
}
fn block_paragraphs(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "width: 600px; padding: 8px");
    let p = css(t, "margin: 6px 0px");
    for i in 0..n {
        text(t, r, &p, 5 + (i % 40) as u32);
    }
    r
}
fn float_flow(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "width: 800px");
    let f = css(t, "float: left; width: 120px; height: 60px; margin: 4px");
    let p = Style::DEFAULT;
    for i in 0..n {
        if i % 4 == 0 {
            add(t, r, &f);
        } else {
            text(t, r, &p, 8 + (i % 30) as u32);
        }
    }
    r
}
fn collapsing_margins(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "width: 800px");
    let outer = css(t, "margin: 10px 0px");
    let inner = css(t, "margin: 6px 0px");
    for i in 0..n / 3 {
        let o = add(t, r, &outer);
        let m = add(t, o, &inner);
        text(t, m, &Style::DEFAULT, 4 + (i % 20) as u32);
    }
    r
}

// ---- Mixed: cephal's home ground ----------------------------------------------------------------

/// A dashboard: a grid of cards, each a flex column of a title row, a body of text and a footer row
/// of buttons. Intrinsic questions cross three formatting contexts.
fn dashboard(t: &mut Tree<Text>, n: usize) -> NodeId {
    let page = root(t, "display: grid; width: 1400px; grid-template-columns: repeat(4, minmax(0px, 1fr)); gap: 12px; padding: 12px");
    let card = css(t, "display: flex; flex-direction: column; padding: 8px; border-width: 1px; gap: 6px");
    let title_row = css(t, "display: flex; align-items: center; gap: 4px");
    let title = css(t, "flex: 1 1 auto");
    let badge = css(t, "padding: 2px 6px");
    let body = css(t, "display: flex; flex-direction: column; gap: 2px");
    let footer = css(t, "display: flex; justify-content: flex-end; gap: 4px");
    let button = css(t, "display: flex; padding: 4px 8px");
    // About 16 nodes per card.
    for i in 0..n.div_ceil(16) {
        let c = add(t, page, &card);
        let tr = add(t, c, &title_row);
        text(t, tr, &title, 6 + (i % 13) as u32);
        let b = add(t, tr, &badge);
        text(t, b, &Style::DEFAULT, 2);
        let bd = add(t, c, &body);
        for k in 0..3 {
            text(t, bd, &Style::DEFAULT, 10 + ((i * 7 + k * 5) % 50) as u32);
        }
        let f = add(t, c, &footer);
        for k in 0..2 {
            let bt = add(t, f, &button);
            text(t, bt, &Style::DEFAULT, 3 + k);
        }
    }
    page
}
/// Columns as wide as their widest line, in rows: min- and max-content answers are shared through
/// canonical keys.
fn intrinsic_columns(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; flex-direction: column; width: 1600px");
    let row = css(t, "display: flex; align-items: flex-start");
    let col = css(t, "display: flex; flex-direction: column; flex: 0 0 auto; padding: 3px");
    let cell = css(t, "padding: 1px");
    for i in 0..n / 25 {
        let rr = add(t, r, &row);
        for j in 0..6 {
            let cc = add(t, rr, &col);
            for k in 0..3 {
                text(t, cc, &cell, 2 + ((i + 3 * j + k) % 12) as u32);
            }
        }
    }
    r
}
/// A deep document: sections of articles of paragraphs, mostly block flow with flex headers.
fn deep_text_tree(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "width: 900px");
    let section = css(t, "padding: 10px; margin: 8px 0px");
    let header = css(t, "display: flex; justify-content: space-between; align-items: center");
    let article = css(t, "margin: 4px 0px; padding: 4px");
    let para = css(t, "margin: 2px 0px");
    let mut count = 1;
    let mut i = 0;
    while count < n {
        let s = add(t, r, &section);
        let h = add(t, s, &header);
        text(t, h, &Style::DEFAULT, 12);
        text(t, h, &Style::DEFAULT, 4);
        count += 4;
        for a in 0..8 {
            let ar = add(t, s, &article);
            count += 1;
            for p in 0..6 {
                text(t, ar, &para, 20 + ((i + a * 6 + p) % 60) as u32);
                count += 1;
            }
        }
        i += 1;
    }
    r
}
/// Independent heavy subtrees under one root: what parallel layout splits.
fn parallel_forest(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; flex-direction: column; width: 1400px");
    let per = 2_000;
    for k in 0..n.div_ceil(per) {
        let sub = dashboard(t, per);
        let _ = k;
        t.append_child(r, sub);
    }
    r
}
/// A list: a column of fixed-height rows of fixed-width text cells (see the incremental scenarios).
fn list(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; flex-direction: column; width: 1000px");
    let row = css(t, "display: flex; width: 100%; height: 20px; flex-shrink: 0; overflow: hidden");
    let cell = css(t, "width: 40px; overflow: hidden");
    for i in 0..n / 10 {
        let rr = add(t, r, &row);
        for c in 0..9 {
            text(t, rr, &cell, 3 + ((i + c) % 4) as u32);
        }
    }
    r
}

// ---- Taffy's home ground (or neutral) ----------------------------------------------------------

/// A toolbar of buttons: tiny trees, where per-call overhead dominates.
fn toolbar(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; align-items: center; gap: 4px; padding: 4px; width: 800px");
    let button = css(t, "display: flex; padding: 4px 8px; border-width: 1px");
    for i in 0..n.div_ceil(2).saturating_sub(1).max(1) {
        let b = add(t, r, &button);
        text(t, b, &Style::DEFAULT, 3 + (i % 6) as u32);
    }
    r
}
fn random_mixed(t: &mut Tree<Text>, n: usize) -> NodeId {
    let mut rng = Rng(3);
    let r = root(t, "display: flex");
    while t.node_count() < n {
        let sub = build_subtree(&mut rng, t, 1);
        t.append_child(r, sub);
    }
    r
}
/// A single chain of nested boxes, alternating flex and block, ending in text.
fn deep_chain(t: &mut Tree<Text>, n: usize) -> NodeId {
    let styles = [css(t, "display: flex; padding: 1px"), css(t, "padding: 1px")];
    let r = root(t, "width: 1000px");
    let mut at = r;
    for i in 0..n.saturating_sub(2) {
        at = add(t, at, &styles[i % 2]);
    }
    text(t, at, &Style::DEFAULT, 7);
    r
}

// ---- PanGui's published workloads (see benches/pangui notes in the suite) ----------------------

fn expand(min0: bool, weight: u32) -> String {
    format!("flex: {weight} 1 0px{}", if min0 { "; min-width: 0; min-height: 0" } else { "" })
}
fn pangui_wide(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, "display: flex; width: 100px; height: 100px", FIXED, None, n)
}
fn pangui_expand_equal(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, "display: flex; width: 10000px; height: 100px", &expand(false, 1), None, n)
}
fn pangui_expand_equal_min0(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, "display: flex; width: 10000px; height: 100px", &expand(true, 1), None, n)
}
fn pangui_expand_weights_impl(t: &mut Tree<Text>, n: usize, min0: bool) -> NodeId {
    let r = root(t, "display: flex; width: 10000px; height: 100px");
    let m = css(t, "display: flex; width: 100px; height: 100px");
    let a = css(t, &expand(min0, 1));
    let b = css(t, &expand(min0, 2));
    for _ in 0..n / 3 {
        let mm = add(t, r, &m);
        add(t, mm, &a);
        add(t, mm, &b);
    }
    r
}
fn pangui_expand_weights(t: &mut Tree<Text>, n: usize) -> NodeId {
    pangui_expand_weights_impl(t, n, false)
}
fn pangui_expand_weights_min0(t: &mut Tree<Text>, n: usize) -> NodeId {
    pangui_expand_weights_impl(t, n, true)
}
fn pangui_nested_vertical_stack(t: &mut Tree<Text>, n: usize) -> NodeId {
    column_stack(t, n)
}
fn pangui_percentage_and_ratio(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, "display: flex; flex-direction: column; width: 1000px; height: 1000px", "width: 50%; aspect-ratio: 2; flex-shrink: 0", None, n)
}
fn pangui_expand_constraint(t: &mut Tree<Text>, n: usize, max: bool, min0: bool) -> NodeId {
    let r = root(t, "display: flex; width: 100px");
    let m = css(t, "display: flex; width: 100px");
    let a = css(t, &format!("{}; height: 10px", expand(min0, 1)));
    let b = css(t, &format!("{}; height: 10px; {}", expand(min0, 1), if max { "max-width: 40px" } else { "min-width: 60px" }));
    for _ in 0..n / 3 {
        let mm = add(t, r, &m);
        add(t, mm, &a);
        add(t, mm, &b);
    }
    r
}
fn pangui_expand_max(t: &mut Tree<Text>, n: usize) -> NodeId {
    pangui_expand_constraint(t, n, true, false)
}
fn pangui_expand_max_min0(t: &mut Tree<Text>, n: usize) -> NodeId {
    pangui_expand_constraint(t, n, true, true)
}
fn pangui_expand_min(t: &mut Tree<Text>, n: usize) -> NodeId {
    pangui_expand_constraint(t, n, false, false)
}
fn pangui_fit_nesting_impl(t: &mut Tree<Text>, n: usize, min0: bool) -> NodeId {
    let r = root(t, "display: flex; flex-direction: column; width: 1000px; padding: 5px");
    let row = css(t, "display: flex");
    let col = css(t, &format!("display: flex; flex-direction: column; {}", expand(min0, 1)));
    let leaf = css(t, &format!("{}; height: 10px", expand(min0, 1)));
    let per_row2 = 100;
    let rows2 = n.div_ceil(per_row2).max(1);
    let (a_n, b_n) = (10.min(rows2), 10.min(rows2.div_ceil(10)));
    let c_n = rows2.div_ceil(a_n * b_n).max(1);
    for _ in 0..a_n {
        let a = add(t, r, &row);
        for _ in 0..b_n {
            let b = add(t, a, &col);
            for _ in 0..c_n {
                let c = add(t, b, &row);
                for _ in 0..per_row2 {
                    add(t, c, &leaf);
                }
            }
        }
    }
    r
}
fn pangui_fit_nesting(t: &mut Tree<Text>, n: usize) -> NodeId {
    pangui_fit_nesting_impl(t, n, false)
}
fn pangui_fit_nesting_min0(t: &mut Tree<Text>, n: usize) -> NodeId {
    pangui_fit_nesting_impl(t, n, true)
}
fn pangui_wide_wrapping(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, "display: flex; flex-wrap: wrap; width: 100px", "width: 10px; height: 10px; flex-shrink: 0", None, n)
}
fn pangui_perpendicular(t: &mut Tree<Text>, n: usize) -> NodeId {
    let r = root(t, "display: flex; flex-wrap: wrap; width: 100px");
    let thin = css(t, "width: 1px; flex-shrink: 0");
    let sized: Vec<Style> = [10, 20, 30, 40, 50, 60].iter().map(|s| css(t, &format!("width: {s}px; height: {s}px; flex-shrink: 0"))).collect();
    for _ in 0..n / 12 {
        for s in &sized {
            add(t, r, s);
            add(t, r, &thin);
        }
    }
    r
}
fn pangui_padding_and_margin(t: &mut Tree<Text>, n: usize) -> NodeId {
    row_of(t, "display: flex; flex-direction: column; width: 1000px; padding: 20px; gap: 10px", "height: 20px; margin: 2px", None, n)
}

// ---- The catalogue ------------------------------------------------------------------------------

const MAX: (Option<f32>, Option<f32>) = (None, None);
const VIEW: (Option<f32>, Option<f32>) = (Some(1600.0), Some(1000.0));
const SCALE: &[usize] = &[100, 1_000, 10_000, 100_000];
const SCALE_SMALL: &[usize] = &[100, 1_000, 10_000];
const TEN_K: &[usize] = &[10_000];

macro_rules! w {
    ($name:literal, $family:literal, [$($tag:literal),*], $sizes:expr, $viewport:expr, $build:path) => {
        w!($name, $family, [$($tag),*], $sizes, $viewport, $build, &[])
    };
    ($name:literal, $family:literal, [$($tag:literal),*], $sizes:expr, $viewport:expr, $build:path, $scenarios:expr) => {
        Workload { name: $name, family: $family, tags: &[$($tag),*], sizes: $sizes, viewport: $viewport, build: $build, scenarios: $scenarios }
    };
}

pub const WORKLOADS: &[Workload] = &[
    // PanGui's set, at its published sizes.
    w!("wide_no_wrap_simple_few", "pangui_wide", ["pangui", "flex", "hit:fused"], &[1_000], MAX, pangui_wide),
    w!("wide_no_wrap_simple_many", "pangui_wide", ["pangui", "flex", "hit:fused"], &[100_000], MAX, pangui_wide),
    w!("flex_expand_equal_weights", "pangui_expand_equal", ["pangui", "flex"], &[15_000], MAX, pangui_expand_equal),
    w!("flex_expand_equal_weights_min0", "pangui_expand_equal_min0", ["pangui", "flex", "hit:fused"], &[15_000], MAX, pangui_expand_equal_min0),
    w!("flex_expand_weights", "pangui_expand_weights", ["pangui", "flex"], &[15_000], MAX, pangui_expand_weights),
    w!("flex_expand_weights_min0", "pangui_expand_weights_min0", ["pangui", "flex", "hit:fused"], &[15_000], MAX, pangui_expand_weights_min0),
    w!("nested_vertical_stack", "pangui_stack", ["pangui", "flex", "hit:fused"], &[10_000], MAX, pangui_nested_vertical_stack),
    w!("percentage_and_ratio", "pangui_percent", ["pangui", "flex", "miss:aspect"], &[10_000], MAX, pangui_percentage_and_ratio),
    w!("expand_with_max_constraint", "pangui_expand_max", ["pangui", "flex"], &[3_000], MAX, pangui_expand_max),
    w!("expand_with_max_constraint_min0", "pangui_expand_max_min0", ["pangui", "flex"], &[3_000], MAX, pangui_expand_max_min0),
    w!("expand_with_min_constraint", "pangui_expand_min", ["pangui", "flex"], &[3_000], MAX, pangui_expand_min),
    w!("fit_nesting", "pangui_fit_nesting", ["pangui", "flex", "cephal"], &[100_000], MAX, pangui_fit_nesting),
    w!("fit_nesting_min0", "pangui_fit_nesting_min0", ["pangui", "flex", "cephal"], &[100_000], MAX, pangui_fit_nesting_min0),
    w!("wide_wrapping", "pangui_wrap", ["pangui", "flex", "miss:wrap"], &[10_000], MAX, pangui_wide_wrapping),
    w!("perpendicular_expand_with_wrap", "pangui_perpendicular", ["pangui", "flex", "miss:wrap"], &[12_000], MAX, pangui_perpendicular),
    w!("padding_and_margin", "pangui_padding", ["pangui", "flex"], &[100], MAX, pangui_padding_and_margin),
    // Flex, with fast-path pairs that differ from `row_fixed` by one item's property.
    w!("row_fixed", "row_fixed", ["flex", "hit:fused"], SCALE, VIEW, row_fixed),
    w!("row_one_baseline", "row_one_baseline", ["flex", "miss:baseline"], TEN_K, VIEW, row_one_baseline),
    w!("row_one_auto_margin", "row_one_auto_margin", ["flex", "miss:auto-margin"], TEN_K, VIEW, row_one_auto_margin),
    w!("row_one_percent", "row_one_percent", ["flex", "miss:percent"], TEN_K, VIEW, row_one_percent),
    w!("row_one_aspect", "row_one_aspect", ["flex", "miss:aspect"], TEN_K, VIEW, row_one_aspect),
    w!("row_one_content_basis", "row_one_content_basis", ["flex", "miss:content-basis"], TEN_K, VIEW, row_one_content_basis),
    w!("row_wrap", "row_wrap", ["flex", "miss:wrap"], TEN_K, VIEW, row_wrap),
    w!("row_reverse", "row_reverse", ["flex", "hit:fused"], TEN_K, VIEW, row_reverse),
    w!("row_expand", "row_expand", ["flex", "hit:fused"], SCALE_SMALL, VIEW, row_expand),
    w!("row_empty_leaves", "row_empty_leaves", ["flex", "hit:cheap-leaf"], TEN_K, VIEW, row_empty_leaves),
    w!("row_measured_leaves", "row_measured_leaves", ["flex", "text", "miss:measured"], TEN_K, VIEW, row_measured_leaves),
    w!("column_stack", "column_stack", ["flex", "hit:fused"], SCALE, VIEW, column_stack),
    w!("wrap_text", "wrap_text", ["flex", "text", "taffy", "miss:wrap"], SCALE_SMALL, VIEW, wrap_text),
    w!("wrap_stretch_content", "wrap_stretch_content", ["flex", "text", "miss:wrap"], TEN_K, VIEW, wrap_stretch_content),
    w!("min_max_row", "min_max_row", ["flex"], TEN_K, VIEW, min_max_row),
    w!("nested_intrinsic_rows", "nested_intrinsic_rows", ["flex", "text", "cephal"], SCALE_SMALL, VIEW, nested_intrinsic_rows),
    // Grid, items given as leaves; the pairs differ from `grid_fixed` by one track.
    w!("grid_fixed", "grid_fixed", ["grid", "hit:fixed-tracks"], SCALE_SMALL, MAX, grid_fixed),
    w!("grid_one_auto_column", "grid_one_auto_column", ["grid", "miss:fixed-tracks"], TEN_K, MAX, grid_one_auto_column),
    w!("grid_auto_cheap", "grid_auto_cheap", ["grid"], SCALE_SMALL, MAX, grid_auto_cheap),
    w!("grid_auto_text", "grid_auto_text", ["grid", "text"], SCALE_SMALL, MAX, grid_auto_text, &[Scenario::EditText, Scenario::Idle]),
    w!("grid_fixed_cols_auto_rows_text", "grid_fixed_cols_auto_rows_text", ["grid", "text"], SCALE_SMALL, MAX, grid_fixed_cols_auto_rows_text),
    w!("grid_mixed_fixed_auto", "grid_mixed_fixed_auto", ["grid", "text"], TEN_K, MAX, grid_mixed_fixed_auto),
    w!("grid_fr_definite", "grid_fr_definite", ["grid", "text"], TEN_K, MAX, grid_fr_definite),
    w!("grid_fr_indefinite", "grid_fr_indefinite", ["grid", "text"], TEN_K, VIEW, grid_fr_indefinite),
    w!("grid_spanning", "grid_spanning", ["grid", "text"], TEN_K, MAX, grid_spanning),
    w!("grid_dense", "grid_dense", ["grid"], TEN_K, MAX, grid_dense),
    w!("grid_minmax_fit_content", "grid_minmax_fit_content", ["grid", "text"], TEN_K, MAX, grid_minmax_fit_content),
    w!("grid_known_size_items", "grid_known_size_items", ["grid", "text"], TEN_K, MAX, grid_known_size_items),
    w!("grid_holy_grail", "grid_holy_grail", ["grid", "text"], SCALE_SMALL, VIEW, grid_holy_grail),
    w!("grid_in_flex_row", "grid_in_flex_row", ["grid", "flex", "text"], SCALE_SMALL, VIEW, grid_in_flex_row),
    w!("grid_known_height_in_column", "grid_known_height_in_column", ["grid", "flex", "text"], TEN_K, VIEW, grid_known_height_in_column),
    w!("nested_grids", "nested_grids", ["grid", "text"], TEN_K, MAX, nested_grids),
    // Block flow.
    w!("block_stack", "block_stack", ["block"], SCALE, VIEW, block_stack),
    w!("block_paragraphs", "block_paragraphs", ["block", "text"], SCALE_SMALL, VIEW, block_paragraphs),
    w!("float_flow", "float_flow", ["block", "text"], &[1_000], VIEW, float_flow),
    w!("collapsing_margins", "collapsing_margins", ["block", "text"], TEN_K, VIEW, collapsing_margins),
    // Mixed documents.
    w!("dashboard", "dashboard", ["mixed", "text", "cephal"], SCALE, VIEW, dashboard, ALL_EDITS),
    w!("intrinsic_columns", "intrinsic_columns", ["mixed", "text", "cephal"], SCALE_SMALL, VIEW, intrinsic_columns),
    w!("deep_text_tree", "deep_text_tree", ["block", "text", "cephal"], SCALE, VIEW, deep_text_tree, &[Scenario::EditText, Scenario::InsertRemove, Scenario::ResizeRoot, Scenario::Idle]),
    w!("parallel_forest", "parallel_forest", ["mixed", "text", "cephal"], &[100_000], VIEW, parallel_forest),
    w!("list", "list", ["flex", "text", "cephal"], SCALE, VIEW, list, &[Scenario::EditText, Scenario::HoverWidth, Scenario::InsertRemove, Scenario::Idle]),
    w!("random_mixed", "random_mixed", ["mixed", "text"], SCALE, VIEW, random_mixed, ALL_EDITS),
    // Small or uncacheable shapes, where per-call overhead shows.
    w!("toolbar", "toolbar", ["flex", "text", "taffy"], &[10, 30, 100], VIEW, toolbar),
    w!("deep_chain", "deep_chain", ["mixed", "taffy"], &[100, 1_000], VIEW, deep_chain),
];

/// Every `Text` leaf, in tree order.
pub fn text_leaves(t: &Tree<Text>, root: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if t.context(n).is_some() {
            out.push(n);
        }
        let kids: Vec<NodeId> = t.children(n).collect();
        stack.extend(kids.into_iter().rev());
    }
    out
}

/// `k` items spread evenly through `items`.
pub fn spread<T: Copy>(items: &[T], k: usize) -> Vec<T> {
    if items.is_empty() {
        return Vec::new();
    }
    (1..=k).map(|i| items[(i * items.len() / (k + 1)).min(items.len() - 1)]).collect()
}

/// Containers with at least two children, at the shallowest depth that has `k` of them.
pub fn containers(t: &Tree<Text>, root: NodeId, k: usize) -> Vec<NodeId> {
    let mut level = vec![root];
    while !level.is_empty() {
        let found: Vec<NodeId> = level.iter().copied().filter(|&n| n != root && t.child_count(n) >= 2).collect();
        if found.len() >= k {
            return spread(&found, k);
        }
        level = level.iter().flat_map(|&n| t.children(n).collect::<Vec<_>>()).collect();
    }
    Vec::new()
}
