//! Converting cephal styles into taffy styles, for migration and differential testing.

use crate::style::{
    AlignContentKeyword, AlignItemsKeyword, AlignmentSafety, BoxSizing, Clear, Contain, Direction, Display, Float,
    FlexDirection, FlexWrap, GridAutoFlow, GridPlacement, GridTemplateComponent, Ident, Length, LengthKind, Overflow,
    Position, RepetitionCount, Style, TextAlign, TrackSizingFunction,
};
use taffy::style_helpers::{TaffyGridLine, TaffyGridSpan};

fn length(l: Length) -> taffy::CompactLength {
    match l.kind() {
        LengthKind::Auto => taffy::CompactLength::auto(),
        LengthKind::Length(v) => taffy::CompactLength::length(v),
        LengthKind::Percent(f) => taffy::CompactLength::percent(f),
        LengthKind::Fr(f) => taffy::CompactLength::fr(f),
        LengthKind::MinContent => taffy::CompactLength::min_content(),
        LengthKind::MaxContent => taffy::CompactLength::max_content(),
        LengthKind::FitContent => taffy::CompactLength::fit_content_keyword(),
        LengthKind::FitContentPx(v) => taffy::CompactLength::fit_content_px(v),
        LengthKind::FitContentPercent(f) => taffy::CompactLength::fit_content_percent(f),
        LengthKind::Stretch => taffy::CompactLength::stretch(),
        LengthKind::Content => taffy::CompactLength::content(),
        LengthKind::Calc(_) => panic!("calc() cannot be converted without the calc table"),
    }
}

fn dimension(l: Length) -> taffy::Dimension {
    // Every keyword above is legal for `Dimension`.
    unsafe { taffy::Dimension::from_raw(length(l)) }
}
fn lpa(l: Length) -> taffy::LengthPercentageAuto {
    unsafe { taffy::LengthPercentageAuto::from_raw(length(l)) }
}
fn lp(l: Length) -> taffy::LengthPercentage {
    unsafe { taffy::LengthPercentage::from_raw(length(l)) }
}

fn align_items(a: crate::style::AlignItems) -> taffy::AlignItems {
    let base = match a.keyword {
        AlignItemsKeyword::Start => taffy::AlignItems::START,
        AlignItemsKeyword::End => taffy::AlignItems::END,
        AlignItemsKeyword::FlexStart => taffy::AlignItems::FLEX_START,
        AlignItemsKeyword::FlexEnd => taffy::AlignItems::FLEX_END,
        AlignItemsKeyword::SelfStart => taffy::AlignItems::SELF_START,
        AlignItemsKeyword::SelfEnd => taffy::AlignItems::SELF_END,
        AlignItemsKeyword::Center => taffy::AlignItems::CENTER,
        AlignItemsKeyword::Baseline => taffy::AlignItems::BASELINE,
        AlignItemsKeyword::Stretch => taffy::AlignItems::STRETCH,
    };
    taffy::AlignItems {
        keyword: base.keyword,
        safety: if a.safety == AlignmentSafety::Safe { taffy::AlignmentSafety::Safe } else { taffy::AlignmentSafety::Unsafe },
    }
}

fn align_content(a: crate::style::AlignContent) -> taffy::AlignContent {
    let base = match a.keyword {
        AlignContentKeyword::Start => taffy::AlignContent::START,
        AlignContentKeyword::End => taffy::AlignContent::END,
        AlignContentKeyword::FlexStart => taffy::AlignContent::FLEX_START,
        AlignContentKeyword::FlexEnd => taffy::AlignContent::FLEX_END,
        AlignContentKeyword::Center => taffy::AlignContent::CENTER,
        AlignContentKeyword::Stretch => taffy::AlignContent::STRETCH,
        AlignContentKeyword::SpaceBetween => taffy::AlignContent::SPACE_BETWEEN,
        AlignContentKeyword::SpaceEvenly => taffy::AlignContent::SPACE_EVENLY,
        AlignContentKeyword::SpaceAround => taffy::AlignContent::SPACE_AROUND,
    };
    taffy::AlignContent {
        keyword: base.keyword,
        safety: if a.safety == AlignmentSafety::Safe { taffy::AlignmentSafety::Safe } else { taffy::AlignmentSafety::Unsafe },
    }
}

fn track(t: TrackSizingFunction) -> taffy::TrackSizingFunction {
    unsafe {
        taffy::TrackSizingFunction {
            min: taffy::MinTrackSizingFunction::from_raw(length(t.min.0)),
            max: taffy::MaxTrackSizingFunction::from_raw(length(t.max.0)),
        }
    }
}

fn placement(p: GridPlacement, name: &impl Fn(Ident) -> String) -> taffy::GridPlacement {
    match p {
        GridPlacement::Auto => taffy::GridPlacement::Auto,
        GridPlacement::Line(l) => taffy::GridPlacement::from_line_index(l),
        GridPlacement::Span(s) => taffy::GridPlacement::from_span(s),
        GridPlacement::NamedLine(id, idx) => taffy::GridPlacement::NamedLine(name(id), idx),
        GridPlacement::NamedSpan(id, n) => taffy::GridPlacement::NamedSpan(name(id), n),
    }
}

/// Converts a style; `name` resolves grid idents to their strings.
pub fn to_taffy(s: &Style, name: &impl Fn(Ident) -> String) -> taffy::Style {
    let mut t = taffy::Style::DEFAULT;
    t.display = match s.display {
        Display::Block => taffy::Display::Block,
        Display::FlowRoot => taffy::Display::FlowRoot,
        Display::Flex => taffy::Display::Flex,
        Display::Grid => taffy::Display::Grid,
        Display::None => taffy::Display::None,
    };
    t.item_is_table = s.item_is_table;
    t.item_is_replaced = s.item_is_replaced;
    t.box_sizing = match s.box_sizing {
        BoxSizing::BorderBox => taffy::BoxSizing::BorderBox,
        BoxSizing::ContentBox => taffy::BoxSizing::ContentBox,
    };
    t.direction = match s.direction {
        Direction::Ltr => taffy::Direction::Ltr,
        Direction::Rtl => taffy::Direction::Rtl,
    };
    let overflow = |o: Overflow| match o {
        Overflow::Visible => taffy::Overflow::Visible,
        Overflow::Clip => taffy::Overflow::Clip,
        Overflow::Hidden => taffy::Overflow::Hidden,
        Overflow::Scroll => taffy::Overflow::Scroll,
    };
    t.overflow = taffy::Point { x: overflow(s.overflow.x), y: overflow(s.overflow.y) };
    t.scrollbar_width = s.scrollbar_width;
    t.contain = match (s.contain.contains(Contain::LAYOUT), s.contain.contains(Contain::PAINT)) {
        (false, false) => taffy::Contain::NONE,
        (true, false) => taffy::Contain::LAYOUT,
        (false, true) => taffy::Contain::PAINT,
        (true, true) => taffy::Contain::CONTENT,
    };
    t.float = match s.float {
        Float::None => taffy::Float::None,
        Float::Left => taffy::Float::Left,
        Float::Right => taffy::Float::Right,
    };
    t.clear = match s.clear {
        Clear::None => taffy::Clear::None,
        Clear::Left => taffy::Clear::Left,
        Clear::Right => taffy::Clear::Right,
        Clear::Both => taffy::Clear::Both,
    };
    t.position = match s.position {
        Position::Relative => taffy::Position::Relative,
        Position::Absolute | Position::Fixed => taffy::Position::Absolute,
    };
    t.inset = taffy::Rect { left: lpa(s.inset.left.0), right: lpa(s.inset.right.0), top: lpa(s.inset.top.0), bottom: lpa(s.inset.bottom.0) };
    t.size = taffy::Size { width: dimension(s.size.width.0), height: dimension(s.size.height.0) };
    t.min_size = taffy::Size { width: lpa(s.min_size.width.0), height: lpa(s.min_size.height.0) };
    t.max_size = taffy::Size { width: lpa(s.max_size.width.0), height: lpa(s.max_size.height.0) };
    t.aspect_ratio = s.aspect_ratio;
    t.margin = taffy::Rect { left: lpa(s.margin.left.0), right: lpa(s.margin.right.0), top: lpa(s.margin.top.0), bottom: lpa(s.margin.bottom.0) };
    t.padding = taffy::Rect { left: lp(s.padding.left.0), right: lp(s.padding.right.0), top: lp(s.padding.top.0), bottom: lp(s.padding.bottom.0) };
    t.border = taffy::Rect { left: lp(s.border.left.0), right: lp(s.border.right.0), top: lp(s.border.top.0), bottom: lp(s.border.bottom.0) };
    t.align_items = s.align_items.map(align_items);
    t.align_self = s.align_self.map(align_items);
    t.justify_items = s.justify_items.map(align_items);
    t.justify_self = s.justify_self.map(align_items);
    t.align_content = s.align_content.map(align_content);
    t.justify_content = s.justify_content.map(align_content);
    t.gap = taffy::Size { width: lp(s.gap.width.0), height: lp(s.gap.height.0) };
    t.text_align = match s.text_align {
        TextAlign::Auto => taffy::TextAlign::Auto,
        TextAlign::LegacyLeft => taffy::TextAlign::LegacyLeft,
        TextAlign::LegacyRight => taffy::TextAlign::LegacyRight,
        TextAlign::LegacyCenter => taffy::TextAlign::LegacyCenter,
    };
    t.flex_direction = match s.flex_direction {
        FlexDirection::Row => taffy::FlexDirection::Row,
        FlexDirection::Column => taffy::FlexDirection::Column,
        FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
        FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
    };
    t.flex_wrap = match s.flex_wrap {
        FlexWrap::NoWrap => taffy::FlexWrap::NoWrap,
        FlexWrap::Wrap => taffy::FlexWrap::Wrap,
        FlexWrap::WrapReverse => taffy::FlexWrap::WrapReverse,
        FlexWrap::Balance => taffy::FlexWrap::Balance,
        FlexWrap::BalanceReverse => taffy::FlexWrap::BalanceReverse,
    };
    t.flex_line_count = s.flex_line_count;
    t.flex_basis = dimension(s.flex_basis.0);
    t.flex_grow = s.flex_grow;
    t.flex_shrink = s.flex_shrink;
    let g = s.grid();
    let component = |c: &GridTemplateComponent| match c {
        GridTemplateComponent::Single(t) => taffy::GridTemplateComponent::Single(track(*t)),
        GridTemplateComponent::Repeat(r) => taffy::GridTemplateComponent::Repeat(taffy::GridTemplateRepetition {
            count: match r.count {
                RepetitionCount::AutoFill => taffy::RepetitionCount::AutoFill,
                RepetitionCount::AutoFit => taffy::RepetitionCount::AutoFit,
                RepetitionCount::Count(n) => taffy::RepetitionCount::Count(n),
            },
            tracks: r.tracks.iter().map(|t| track(*t)).collect(),
            line_names: r.line_names.iter().map(|set| set.iter().map(|i| name(*i)).collect()).collect(),
        }),
    };
    t.grid_template_rows = g.template_rows.iter().map(component).collect();
    t.grid_template_columns = g.template_columns.iter().map(component).collect();
    t.grid_template_row_names = g.template_row_names.iter().map(|set| set.iter().map(|i| name(*i)).collect()).collect();
    t.grid_template_column_names = g.template_column_names.iter().map(|set| set.iter().map(|i| name(*i)).collect()).collect();
    t.grid_auto_rows = g.auto_rows.iter().map(|t| track(*t)).collect();
    t.grid_auto_columns = g.auto_columns.iter().map(|t| track(*t)).collect();
    t.grid_auto_flow = match g.auto_flow {
        GridAutoFlow::Row => taffy::GridAutoFlow::Row,
        GridAutoFlow::Column => taffy::GridAutoFlow::Column,
        GridAutoFlow::RowDense => taffy::GridAutoFlow::RowDense,
        GridAutoFlow::ColumnDense => taffy::GridAutoFlow::ColumnDense,
    };
    t.grid_template_areas = g.template_areas.as_ref().map(|areas| taffy::GridTemplateAreas {
        areas: areas
            .areas
            .iter()
            .map(|a| taffy::GridTemplateArea {
                name: name(a.name),
                row_start: a.row_start,
                row_end: a.row_end,
                column_start: a.column_start,
                column_end: a.column_end,
            })
            .collect(),
        row_count: areas.row_count,
        column_count: areas.column_count,
    });
    t.grid_row = taffy::Line { start: placement(s.grid_row.start, name), end: placement(s.grid_row.end, name) };
    t.grid_column = taffy::Line { start: placement(s.grid_column.start, name), end: placement(s.grid_column.end, name) };
    t
}
