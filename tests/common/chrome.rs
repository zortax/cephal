//! Random trees as Chrome fixtures, so Chrome can adjudicate where cephal and taffy differ.
//!
//! [`to_html`] writes a tree in the format of taffy's gentest (see `scripts/gen_fixtures.sh`), with
//! every property the base stylesheet would otherwise supply written inline, and pins
//! `box-sizing` and `direction` on every box so the four variants gentest renders are the same
//! tree. [`expectations`] reads Chrome's layout back, parent-relative like cephal's.

use super::Text;
use cephal::geometry::{AvailableSpace, Size};
use cephal::style::{
    AlignItems, AlignItemsKeyword, AlignContent, AlignContentKeyword, AlignmentSafety, BoxSizing, Direction, Display,
    FlexDirection, FlexWrap, GridAutoFlow, GridPlacement, GridTemplateComponent, Length, LengthKind, Overflow, Position,
    Style, TrackSizingFunction,
};
use cephal::tree::NodeId;
use cephal::Tree;
use std::fmt::Write as _;

fn num(v: f32) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// A length, percentage or keyword as CSS; `auto_word` is what `auto` is spelled as (`none` for
/// maximum sizes).
fn length(l: Length, auto_word: &str) -> String {
    match l.kind() {
        LengthKind::Auto => auto_word.into(),
        LengthKind::Length(v) => format!("{}px", num(v)),
        LengthKind::Percent(f) => format!("{}%", num(f * 100.0)),
        LengthKind::Fr(f) => format!("{}fr", num(f)),
        LengthKind::MinContent => "min-content".into(),
        LengthKind::MaxContent => "max-content".into(),
        LengthKind::FitContent => "fit-content".into(),
        // Not a valid width in Chrome; `expressible` substitutes these where it can.
        LengthKind::FitContentPx(v) => format!("fit-content({}px)", num(v)),
        LengthKind::FitContentPercent(f) => format!("fit-content({}%)", num(f * 100.0)),
        LengthKind::Stretch => "stretch".into(),
        LengthKind::Content => "content".into(),
        LengthKind::Calc(_) => panic!("calc() is not generated"),
    }
}

fn track(t: TrackSizingFunction) -> String {
    if t.min.0 == t.max.0 {
        return length(t.min.0, "auto");
    }
    match (t.min.kind(), t.max.kind()) {
        (LengthKind::Auto, LengthKind::Fr(_)) => length(t.max.0, "auto"),
        (LengthKind::Auto, LengthKind::FitContentPx(_) | LengthKind::FitContentPercent(_)) => length(t.max.0, "auto"),
        _ => format!("minmax({}, {})", length(t.min.0, "auto"), length(t.max.0, "auto")),
    }
}

fn tracks(list: &[GridTemplateComponent]) -> String {
    list.iter()
        .map(|c| match c {
            GridTemplateComponent::Single(t) => track(*t),
            GridTemplateComponent::Repeat(_) => panic!("repeat() is not generated"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn safety(s: AlignmentSafety) -> &'static str {
    match s {
        AlignmentSafety::Default => "",
        AlignmentSafety::Safe => "safe ",
        AlignmentSafety::Unsafe => "unsafe ",
    }
}

fn align_items(a: AlignItems) -> String {
    use AlignItemsKeyword::*;
    let k = match a.keyword {
        Start => "start",
        End => "end",
        FlexStart => "flex-start",
        FlexEnd => "flex-end",
        SelfStart => "self-start",
        SelfEnd => "self-end",
        Center => "center",
        Baseline => "baseline",
        Stretch => "stretch",
    };
    if matches!(a.keyword, Baseline | Stretch) { k.into() } else { format!("{}{k}", safety(a.safety)) }
}

fn align_content(a: AlignContent) -> String {
    use AlignContentKeyword::*;
    let k = match a.keyword {
        Start => "start",
        End => "end",
        FlexStart => "flex-start",
        FlexEnd => "flex-end",
        Center => "center",
        Stretch => "stretch",
        SpaceBetween => "space-between",
        SpaceEvenly => "space-evenly",
        SpaceAround => "space-around",
    };
    if matches!(a.keyword, Stretch | SpaceBetween | SpaceEvenly | SpaceAround) { k.into() } else { format!("{}{k}", safety(a.safety)) }
}

fn placement(p: GridPlacement) -> String {
    match p {
        GridPlacement::Auto => "auto".into(),
        GridPlacement::Line(n) => n.to_string(),
        GridPlacement::Span(n) => format!("span {n}"),
        GridPlacement::NamedLine(..) | GridPlacement::NamedSpan(..) => panic!("named lines are not generated"),
    }
}

/// The inline style of a box with this style, `position` and `display` (which `to_html` decides
/// for the root).
fn css(style: &Style, position: Position, display: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut put = |k: &str, v: String| out.push(format!("{k}: {v}"));
    put("display", display.into());
    put(
        "position",
        match position {
            Position::Static => "static",
            Position::Relative => "relative",
            Position::Absolute => "absolute",
            Position::Fixed => "fixed",
            Position::Sticky => "sticky",
        }
        .into(),
    );
    put("box-sizing", if style.box_sizing == BoxSizing::BorderBox { "border-box" } else { "content-box" }.into());
    put("direction", if style.direction == Direction::Ltr { "ltr" } else { "rtl" }.into());
    let size = |l: Length| !l.is_auto();
    if size(style.size.width.raw()) {
        put("width", length(style.size.width.raw(), "auto"));
    }
    if size(style.size.height.raw()) {
        put("height", length(style.size.height.raw(), "auto"));
    }
    if size(style.min_size.width.raw()) {
        put("min-width", length(style.min_size.width.raw(), "auto"));
    }
    if size(style.min_size.height.raw()) {
        put("min-height", length(style.min_size.height.raw(), "auto"));
    }
    if size(style.max_size.width.raw()) {
        put("max-width", length(style.max_size.width.raw(), "none"));
    }
    if size(style.max_size.height.raw()) {
        put("max-height", length(style.max_size.height.raw(), "none"));
    }
    if let Some(r) = style.aspect_ratio {
        put("aspect-ratio", num(r));
    }
    let zero = |l: Length| l.kind() == LengthKind::Length(0.0);
    for (side, m, p, b, i) in [
        ("top", style.margin.top, style.padding.top, style.border.top, style.inset.top),
        ("right", style.margin.right, style.padding.right, style.border.right, style.inset.right),
        ("bottom", style.margin.bottom, style.padding.bottom, style.border.bottom, style.inset.bottom),
        ("left", style.margin.left, style.padding.left, style.border.left, style.inset.left),
    ] {
        if !zero(m.raw()) {
            put(&format!("margin-{side}"), length(m.raw(), "auto"));
        }
        if !zero(p.raw()) {
            put(&format!("padding-{side}"), length(p.raw(), "auto"));
        }
        if !zero(b.raw()) {
            put(&format!("border-{side}-width"), length(b.raw(), "auto"));
        }
        // Sticky offsets need scroll positions, which a host applies; cephal lays sticky boxes out
        // without them, as Chrome does without insets.
        if matches!(position, Position::Relative | Position::Absolute | Position::Fixed) && !i.raw().is_auto() {
            put(side, length(i.raw(), "auto"));
        }
    }
    if !zero(style.gap.height.raw()) {
        put("row-gap", length(style.gap.height.raw(), "auto"));
    }
    if !zero(style.gap.width.raw()) {
        put("column-gap", length(style.gap.width.raw(), "auto"));
    }
    let overflow = |o: Overflow| match o {
        Overflow::Visible => "visible",
        Overflow::Clip => "clip",
        Overflow::Hidden => "hidden",
        Overflow::Scroll => "scroll",
    };
    if style.overflow.x != Overflow::Visible {
        put("overflow-x", overflow(style.overflow.x).into());
    }
    if style.overflow.y != Overflow::Visible {
        put("overflow-y", overflow(style.overflow.y).into());
    }
    if style.flex_direction != FlexDirection::Row {
        put(
            "flex-direction",
            match style.flex_direction {
                FlexDirection::Row => "row",
                FlexDirection::Column => "column",
                FlexDirection::RowReverse => "row-reverse",
                FlexDirection::ColumnReverse => "column-reverse",
            }
            .into(),
        );
    }
    if style.flex_wrap != FlexWrap::NoWrap {
        put("flex-wrap", if style.flex_wrap == FlexWrap::Wrap { "wrap" } else { "wrap-reverse" }.into());
    }
    if style.flex_grow != 0.0 {
        put("flex-grow", num(style.flex_grow));
    }
    if style.flex_shrink != 1.0 {
        put("flex-shrink", num(style.flex_shrink));
    }
    if !style.flex_basis.raw().is_auto() {
        put("flex-basis", length(style.flex_basis.raw(), "auto"));
    }
    if let Some(a) = style.align_items {
        put("align-items", align_items(a));
    }
    if let Some(a) = style.align_self {
        put("align-self", align_items(a));
    }
    if let Some(a) = style.justify_items {
        put("justify-items", align_items(a));
    }
    if let Some(a) = style.justify_self {
        put("justify-self", align_items(a));
    }
    if let Some(a) = style.align_content {
        put("align-content", align_content(a));
    }
    if let Some(a) = style.justify_content {
        put("justify-content", align_content(a));
    }
    if style.grid.is_some() {
        let g = style.grid();
        assert!(g.template_areas.is_none() && g.template_row_names.is_empty() && g.template_column_names.is_empty());
        if !g.template_columns.is_empty() {
            put("grid-template-columns", tracks(&g.template_columns));
        }
        if !g.template_rows.is_empty() {
            put("grid-template-rows", tracks(&g.template_rows));
        }
        if !g.auto_rows.is_empty() {
            put("grid-auto-rows", g.auto_rows.iter().map(|t| track(*t)).collect::<Vec<_>>().join(" "));
        }
        if !g.auto_columns.is_empty() {
            put("grid-auto-columns", g.auto_columns.iter().map(|t| track(*t)).collect::<Vec<_>>().join(" "));
        }
        let flow = match g.auto_flow {
            GridAutoFlow::Row => None,
            GridAutoFlow::Column => Some("column"),
            GridAutoFlow::RowDense => Some("row dense"),
            GridAutoFlow::ColumnDense => Some("column dense"),
        };
        if let Some(f) = flow {
            put("grid-auto-flow", f.into());
        }
    }
    for (k, p) in [
        ("grid-row-start", style.grid_row.start),
        ("grid-row-end", style.grid_row.end),
        ("grid-column-start", style.grid_column.start),
        ("grid-column-end", style.grid_column.end),
    ] {
        if p != GridPlacement::Auto {
            put(k, placement(p));
        }
    }
    out.join("; ")
}

fn display(d: Display) -> &'static str {
    match d {
        Display::Block => "block",
        Display::Flex => "flex",
        Display::Grid => "grid",
        Display::FlowRoot => "flow-root",
        Display::None => "none",
    }
}

/// How the root is embedded: in a `display: block` viewport of the given size (a block box that
/// contains `fixed` boxes as well), as taffy's and cephal's root is the initial containing block's
/// only box. The root's margins never collapse (CSS 2.2 §8.3.1), so a block root is `flow-root`.
/// An out-of-flow root is laid out in flow when the viewport height is indefinite, so it is
/// `relative` (without insets) then: positioned, so still the containing block of its
/// absolutely positioned descendants.
pub fn root_position(style: &Style, viewport: Size<AvailableSpace>) -> Position {
    match style.position {
        Position::Absolute | Position::Fixed if !matches!(viewport.height, AvailableSpace::Definite(_)) => Position::Relative,
        p => p,
    }
}

/// What a fixture cannot express exactly about `tree`, as notes for its title.
pub fn caveats(tree: &Tree<Text>, root: NodeId, viewport: Size<AvailableSpace>) -> Vec<String> {
    let mut notes = Vec::new();
    let mut nodes = Vec::new();
    super::all_nodes(tree, root, &mut nodes);
    let ix = |n: NodeId| format!("#{}", nodes.iter().position(|&m| m == n).unwrap());
    let hidden = |n: NodeId| std::iter::successors(Some(n), |&a| tree.parent(a)).any(|a| tree.style(a).display == Display::None);
    let icb_boxes: Vec<String> = nodes
        .iter()
        .copied()
        .filter(|&n| n != root && !hidden(n) && tree.style(n).position.is_out_of_flow() && tree.containing_block(n).is_none())
        .map(ix)
        .collect();
    let rs = tree.style(root);
    if !icb_boxes.is_empty() && rs.display != Display::None {
        let scroll = rs.overflow.x == Overflow::Scroll || rs.overflow.y == Overflow::Scroll;
        if scroll {
            notes.push(format!("the root's scrollbars shrink the initial containing block of {}", icb_boxes.join(" ")));
        }
        let in_flow_root = !(rs.position.is_out_of_flow() && matches!(viewport.height, AvailableSpace::Definite(_)));
        if !matches!(viewport.height, AvailableSpace::Definite(_)) && in_flow_root {
            let vertical = !rs.margin.top.raw().is_auto() && rs.margin.top.raw().kind() != LengthKind::Length(0.0)
                || !rs.margin.bottom.raw().is_auto() && rs.margin.bottom.raw().kind() != LengthKind::Length(0.0)
                || rs.border.top.raw().kind() != LengthKind::Length(0.0)
                || rs.border.bottom.raw().kind() != LengthKind::Length(0.0)
                || rs.overflow.x == Overflow::Scroll
                || rs.position == Position::Relative && (!rs.inset.top.raw().is_auto() || !rs.inset.bottom.raw().is_auto());
            if vertical {
                notes.push(format!(
                    "indefinite viewport height: the initial containing block of {} is the root's padding box vertically, not the viewport's",
                    icb_boxes.join(" ")
                ));
            }
        }
    }
    if !(rs.position.is_out_of_flow() && matches!(viewport.height, AvailableSpace::Definite(_))) && rs.display != Display::None {
        if [rs.margin.left, rs.margin.right, rs.margin.top, rs.margin.bottom].iter().any(|m| m.is_auto()) {
            notes.push("the engines count the root's auto margins as zero".into());
        }
        if matches!(rs.display, Display::Flex | Display::Grid) && rs.size.width.is_auto() {
            notes.push("the engines size a flex or grid root of auto width by its content; Chrome's fills the viewport".into());
        }
    }
    for &n in &nodes {
        if hidden(n) {
            continue;
        }
        let s = tree.style(n);
        for (what, l) in [("width", s.size.width.raw()), ("height", s.size.height.raw()), ("flex-basis", s.flex_basis.raw())] {
            if matches!(l.kind(), LengthKind::FitContentPx(_) | LengthKind::FitContentPercent(_)) {
                notes.push(format!("{} {what}: {} is not supported by Chrome", ix(n), length(l, "auto")));
            }
        }
        if (s.overflow.x == Overflow::Visible) != (s.overflow.y == Overflow::Visible) {
            notes.push(format!("{}: overflow visible computes to auto beside scroll", ix(n)));
        }
    }
    notes
}

/// The tree as a gentest fixture.
pub fn to_html(tree: &Tree<Text>, root: NodeId, viewport: Size<AvailableSpace>, title: &str) -> String {
    fn node(tree: &Tree<Text>, n: NodeId, root: NodeId, viewport: Size<AvailableSpace>, depth: usize, index: &mut usize, out: &mut String) {
        let style = tree.style(n);
        let indent = "  ".repeat(depth);
        let is_root = n == root;
        let position = if is_root { root_position(style, viewport) } else { style.position };
        let disp = if is_root && style.display == Display::Block { "flow-root" } else { display(style.display) };
        let mut style_text = css(style, position, disp);
        if is_root && position != style.position {
            // Laid out in flow: no insets.
            style_text = style_text.split("; ").filter(|d| !["top:", "right:", "bottom:", "left:"].iter().any(|p| d.starts_with(p))).collect::<Vec<_>>().join("; ");
        }
        let id = if is_root { " id=\"test-root\" data-test-rounding=\"false\"" } else { "" };
        let _ = write!(out, "{indent}<div{id} data-n=\"{index}\" style=\"{style_text}\">");
        *index += 1;
        let children: Vec<NodeId> = tree.children(n).collect();
        if children.is_empty() {
            if let Some(text) = tree.context(n) {
                out.push_str(&vec!["X"; text.0 as usize].join("&#8203;"));
            }
        } else {
            out.push('\n');
            for c in children {
                node(tree, c, root, viewport, depth + 1, index, out);
            }
            out.push_str(&indent);
        }
        out.push_str("</div>\n");
    }
    let width = match viewport.width {
        AvailableSpace::Definite(w) => format!("{}px", num(w)),
        AvailableSpace::MinContent => "min-content".into(),
        AvailableSpace::MaxContent => "max-content".into(),
    };
    let height = match viewport.height {
        AvailableSpace::Definite(h) => format!("{}px", num(h)),
        AvailableSpace::MinContent => "min-content".into(),
        AvailableSpace::MaxContent => "max-content".into(),
    };
    let mut body = String::new();
    node(tree, root, root, viewport, 1, &mut 0, &mut body);
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <script src=\"../../scripts/gentest/test_helper.js\"></script>\n  <link rel=\"stylesheet\" type=\"text/css\" href=\"../../scripts/gentest/test_base_style.css\">\n  <title>\n    {}\n  </title>\n</head>\n<body>\n\n<div class=\"viewport\" style=\"display: block; contain: layout; direction: ltr; box-sizing: border-box; width: {width}; height: {height}\">\n{body}</div>\n\n</body>\n</html>\n",
        title.replace('<', "&lt;").replace('>', "&gt;")
    )
}

/// One box as Chrome laid it out: location relative to the parent's border box, border-box size.
#[derive(Clone, Copy, Debug)]
pub struct Rect4 {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Chrome's layout of `tree` from the gentest XML test `name`, in `all_nodes` order with
/// parent-relative locations (gentest places out-of-flow boxes relative to their containing
/// block, or to the root when that is the initial containing block). `None` for hidden boxes.
pub fn expectations(xml: &str, name: &str, tree: &Tree<Text>, root: NodeId, viewport: Size<AvailableSpace>) -> Option<Vec<Option<Rect4>>> {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let test = doc.root_element().children().find(|n| n.has_tag_name("test") && n.attribute("name") == Some(name))?;
    let exp = test.children().find(|n| n.has_tag_name("expectations"))?.first_element_child()?;
    let mut out = Vec::new();
    fn walk(
        tree: &Tree<Text>,
        n: NodeId,
        e: roxmltree::Node,
        root: NodeId,
        viewport: Size<AvailableSpace>,
        ancestors: &mut Vec<(NodeId, (f32, f32))>,
        out: &mut Vec<Option<Rect4>>,
    ) {
        let f = |k: &str| e.attribute(k).unwrap().parse::<f32>().unwrap();
        let (x, y, w, h) = (f("x"), f("y"), f("width"), f("height"));
        let style = tree.style(n);
        let hidden = style.display == Display::None || ancestors.iter().any(|(a, _)| tree.style(*a).display == Display::None);
        let position = |a: NodeId| if a == root { root_position(tree.style(a), viewport) } else { tree.style(a).position };
        let abs = match ancestors.last() {
            None => (x, y),
            Some(&(_, parent)) => {
                let frame = match position(n) {
                    _ if hidden => None,
                    Position::Absolute => Some(ancestors.iter().rev().find(|(a, _)| position(*a) != Position::Static).unwrap_or(&ancestors[0]).1),
                    Position::Fixed => Some(ancestors[0].1),
                    _ => None,
                };
                match frame {
                    Some(cb) => (cb.0 + x, cb.1 + y),
                    None => (parent.0 + x, parent.1 + y),
                }
            }
        };
        let rel = match ancestors.last() {
            None => (x, y),
            Some(&(_, parent)) => (abs.0 - parent.0, abs.1 - parent.1),
        };
        out.push(if hidden { None } else { Some(Rect4 { x: rel.0, y: rel.1, w, h }) });
        ancestors.push((n, abs));
        for (c, ce) in tree.children(n).zip(e.children().filter(|c| c.is_element())) {
            walk(tree, c, ce, root, viewport, ancestors, out);
        }
        ancestors.pop();
    }
    walk(tree, root, exp, root, viewport, &mut Vec::new(), &mut out);
    Some(out)
}
