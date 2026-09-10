//! Parsers for CSS-like property strings, as used by fixtures and simple embedders.

use super::grid::{
    GridPlacement, GridTemplateComponent, GridTemplateRepetition, MaxTrackSizingFunction, MinTrackSizingFunction,
    RepetitionCount, TrackSizingFunction,
};
use super::{
    AlignContent, AlignContentKeyword, AlignItems, AlignItemsKeyword, AlignmentSafety, BoxSizing, Clear, Contain,
    Dimension, Direction, Display, FlexDirection, FlexWrap, Float, GridAutoFlow, Ident, Length, LengthPercentage,
    LengthPercentageAuto, Overflow, Position, TextAlign,
};
use crate::geometry::AvailableSpace;

/// `10px`, `10`, `-5.5px` or `50%`.
fn parse_length_or_percent(s: &str) -> Option<Length> {
    let s = s.trim();
    if let Some(p) = s.strip_suffix('%') {
        return p.trim().parse::<f32>().ok().map(|v| Length::percent(v / 100.0));
    }
    let n = s.strip_suffix("px").unwrap_or(s);
    n.trim().parse::<f32>().ok().map(Length::length)
}

pub fn parse_length_percentage(s: &str) -> Option<LengthPercentage> {
    parse_length_or_percent(s).map(LengthPercentage)
}

pub fn parse_length_percentage_auto(s: &str) -> Option<LengthPercentageAuto> {
    if s.trim() == "auto" {
        return Some(LengthPercentageAuto::AUTO);
    }
    parse_length_or_percent(s).map(LengthPercentageAuto)
}

pub fn parse_dimension(s: &str) -> Option<Dimension> {
    let s = s.trim();
    Some(match s {
        "auto" => Dimension::AUTO,
        "min-content" => Dimension::MIN_CONTENT,
        "max-content" => Dimension::MAX_CONTENT,
        "fit-content" => Dimension::FIT_CONTENT,
        "stretch" => Dimension::STRETCH,
        "content" => Dimension::CONTENT,
        _ => {
            if let Some(inner) = s.strip_prefix("fit-content(").and_then(|r| r.strip_suffix(')')) {
                return match parse_length_or_percent(inner)?.kind() {
                    super::LengthKind::Length(px) => Some(Dimension::fit_content_px(px)),
                    super::LengthKind::Percent(f) => Some(Dimension::fit_content_percent(f)),
                    _ => None,
                };
            }
            Dimension(parse_length_or_percent(s)?)
        }
    })
}

pub fn parse_available_space(s: &str) -> Option<AvailableSpace> {
    match s.trim() {
        "min-content" => Some(AvailableSpace::MinContent),
        "max-content" => Some(AvailableSpace::MaxContent),
        other => match parse_length_or_percent(other)?.kind() {
            super::LengthKind::Length(px) => Some(AvailableSpace::Definite(px)),
            _ => None,
        },
    }
}

fn split_safety(s: &str) -> (AlignmentSafety, &str) {
    let s = s.trim();
    if let Some(r) = s.strip_prefix("safe ") {
        (AlignmentSafety::Safe, r.trim())
    } else if let Some(r) = s.strip_prefix("unsafe ") {
        (AlignmentSafety::Unsafe, r.trim())
    } else {
        (AlignmentSafety::Unsafe, s)
    }
}

pub fn parse_align_items(s: &str) -> Option<AlignItems> {
    let (safety, kw) = split_safety(s);
    let keyword = match kw {
        "start" => AlignItemsKeyword::Start,
        "end" => AlignItemsKeyword::End,
        "flex-start" => AlignItemsKeyword::FlexStart,
        "flex-end" => AlignItemsKeyword::FlexEnd,
        "self-start" => AlignItemsKeyword::SelfStart,
        "self-end" => AlignItemsKeyword::SelfEnd,
        "center" => AlignItemsKeyword::Center,
        "baseline" => AlignItemsKeyword::Baseline,
        "stretch" | "normal" => AlignItemsKeyword::Stretch,
        _ => return None,
    };
    Some(AlignItems { keyword, safety })
}

pub fn parse_align_content(s: &str) -> Option<AlignContent> {
    let (safety, kw) = split_safety(s);
    let keyword = match kw {
        "start" => AlignContentKeyword::Start,
        "end" => AlignContentKeyword::End,
        "flex-start" => AlignContentKeyword::FlexStart,
        "flex-end" => AlignContentKeyword::FlexEnd,
        "center" => AlignContentKeyword::Center,
        "stretch" => AlignContentKeyword::Stretch,
        "space-between" => AlignContentKeyword::SpaceBetween,
        "space-evenly" => AlignContentKeyword::SpaceEvenly,
        "space-around" => AlignContentKeyword::SpaceAround,
        _ => return None,
    };
    Some(AlignContent { keyword, safety })
}

pub fn parse_display(s: &str) -> Option<Display> {
    Some(match s.trim() {
        "block" => Display::Block,
        "flow-root" => Display::FlowRoot,
        "flex" => Display::Flex,
        "grid" => Display::Grid,
        "none" => Display::None,
        _ => return None,
    })
}

pub fn parse_position(s: &str) -> Option<Position> {
    Some(match s.trim() {
        "relative" => Position::Relative,
        "absolute" => Position::Absolute,
        _ => return None,
    })
}

pub fn parse_overflow(s: &str) -> Option<Overflow> {
    Some(match s.trim() {
        "visible" => Overflow::Visible,
        "clip" => Overflow::Clip,
        "hidden" => Overflow::Hidden,
        "scroll" | "auto" => Overflow::Scroll,
        _ => return None,
    })
}

pub fn parse_box_sizing(s: &str) -> Option<BoxSizing> {
    Some(match s.trim() {
        "border-box" => BoxSizing::BorderBox,
        "content-box" => BoxSizing::ContentBox,
        _ => return None,
    })
}

pub fn parse_direction(s: &str) -> Option<Direction> {
    Some(match s.trim() {
        "ltr" => Direction::Ltr,
        "rtl" => Direction::Rtl,
        _ => return None,
    })
}

pub fn parse_flex_direction(s: &str) -> Option<FlexDirection> {
    Some(match s.trim() {
        "row" => FlexDirection::Row,
        "column" => FlexDirection::Column,
        "row-reverse" => FlexDirection::RowReverse,
        "column-reverse" => FlexDirection::ColumnReverse,
        _ => return None,
    })
}

pub fn parse_flex_wrap(s: &str) -> Option<FlexWrap> {
    let mut reverse = false;
    let mut balance = false;
    let mut wrap = false;
    for word in s.split_whitespace() {
        match word {
            "nowrap" => {}
            "wrap" => wrap = true,
            "wrap-reverse" => {
                wrap = true;
                reverse = true;
            }
            "balance" => {
                wrap = true;
                balance = true;
            }
            _ => return None,
        }
    }
    Some(match (wrap, reverse, balance) {
        (false, _, _) => FlexWrap::NoWrap,
        (true, false, false) => FlexWrap::Wrap,
        (true, true, false) => FlexWrap::WrapReverse,
        (true, false, true) => FlexWrap::Balance,
        (true, true, true) => FlexWrap::BalanceReverse,
    })
}

pub fn parse_float(s: &str) -> Option<Float> {
    Some(match s.trim() {
        "none" => Float::None,
        "left" => Float::Left,
        "right" => Float::Right,
        _ => return None,
    })
}

pub fn parse_clear(s: &str) -> Option<Clear> {
    Some(match s.trim() {
        "none" => Clear::None,
        "left" => Clear::Left,
        "right" => Clear::Right,
        "both" => Clear::Both,
        _ => return None,
    })
}

pub fn parse_text_align(s: &str) -> Option<TextAlign> {
    Some(match s.trim() {
        "auto" => TextAlign::Auto,
        "-webkit-left" | "-moz-left" => TextAlign::LegacyLeft,
        "-webkit-right" | "-moz-right" => TextAlign::LegacyRight,
        "-webkit-center" | "-moz-center" => TextAlign::LegacyCenter,
        _ => return None,
    })
}

pub fn parse_contain(s: &str) -> Option<Contain> {
    let mut c = Contain::NONE;
    for word in s.split_whitespace() {
        c = c.union(match word {
            "none" => Contain::NONE,
            "layout" => Contain::LAYOUT,
            "paint" => Contain::PAINT,
            "content" | "strict" => Contain::CONTENT,
            "size" | "inline-size" | "style" => Contain::NONE,
            _ => return None,
        });
    }
    Some(c)
}

pub fn parse_grid_auto_flow(s: &str) -> Option<GridAutoFlow> {
    let mut column = false;
    let mut dense = false;
    for word in s.split_whitespace() {
        match word {
            "row" => {}
            "column" => column = true,
            "dense" => dense = true,
            _ => return None,
        }
    }
    Some(match (column, dense) {
        (false, false) => GridAutoFlow::Row,
        (true, false) => GridAutoFlow::Column,
        (false, true) => GridAutoFlow::RowDense,
        (true, true) => GridAutoFlow::ColumnDense,
    })
}

/// `auto`, `3`, `-1`, `span 2`, `foo`, `2 foo`, `span foo`, `span 2 foo`.
pub fn parse_grid_placement(s: &str, intern: &mut impl FnMut(&str) -> Ident) -> Option<GridPlacement> {
    let words: Vec<&str> = s.split_whitespace().collect();
    match words.as_slice() {
        ["auto"] => Some(GridPlacement::Auto),
        [n] => match n.parse::<i16>() {
            Ok(i) => Some(GridPlacement::Line(i)),
            Err(_) => Some(GridPlacement::NamedLine(intern(n), 1)),
        },
        ["span", n] => match n.parse::<u16>() {
            Ok(i) => Some(GridPlacement::Span(i)),
            Err(_) => Some(GridPlacement::NamedSpan(intern(n), 1)),
        },
        [n, name] => n.parse::<i16>().ok().map(|i| GridPlacement::NamedLine(intern(name), i)),
        ["span", n, name] => n.parse::<u16>().ok().map(|i| GridPlacement::NamedSpan(intern(name), i)),
        _ => None,
    }
}

fn parse_min_track(s: &str) -> Option<MinTrackSizingFunction> {
    Some(match s.trim() {
        "auto" => MinTrackSizingFunction::AUTO,
        "min-content" => MinTrackSizingFunction::MIN_CONTENT,
        "max-content" => MinTrackSizingFunction::MAX_CONTENT,
        other => MinTrackSizingFunction(parse_length_or_percent(other)?),
    })
}

fn parse_max_track(s: &str) -> Option<MaxTrackSizingFunction> {
    let s = s.trim();
    Some(match s {
        "auto" => MaxTrackSizingFunction::AUTO,
        "min-content" => MaxTrackSizingFunction::MIN_CONTENT,
        "max-content" => MaxTrackSizingFunction::MAX_CONTENT,
        _ => {
            if let Some(fr) = s.strip_suffix("fr") {
                return fr.trim().parse::<f32>().ok().map(MaxTrackSizingFunction::fr);
            }
            if let Some(inner) = s.strip_prefix("fit-content(").and_then(|r| r.strip_suffix(')')) {
                return match parse_length_or_percent(inner)?.kind() {
                    super::LengthKind::Length(px) => Some(MaxTrackSizingFunction::fit_content_px(px)),
                    super::LengthKind::Percent(f) => Some(MaxTrackSizingFunction::fit_content_percent(f)),
                    _ => None,
                };
            }
            MaxTrackSizingFunction(parse_length_or_percent(s)?)
        }
    })
}

/// One track: `<max>` or `minmax(<min>, <max>)`.
pub fn parse_track_sizing_function(s: &str) -> Option<TrackSizingFunction> {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("minmax(").and_then(|r| r.strip_suffix(')')) {
        let (a, b) = inner.split_once(',')?;
        return Some(TrackSizingFunction::minmax(parse_min_track(a)?, parse_max_track(b)?));
    }
    let max = parse_max_track(s)?;
    let min = if max.is_fr() || max.is_fit_content() { MinTrackSizingFunction::AUTO } else { MinTrackSizingFunction(max.0) };
    Some(TrackSizingFunction { min, max })
}

/// Splits on whitespace outside brackets and parentheses.
fn tokens(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' | '[' => {
                depth += 1;
                start.get_or_insert(i);
            }
            ')' | ']' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => {
                if let Some(st) = start.take() {
                    out.push(&s[st..i]);
                }
            }
            _ => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

/// Tracks with their line names: `[a] 10px [b c] repeat(2, [x] 1fr) [d]`.
fn parse_track_list_inner(
    s: &str,
    intern: &mut impl FnMut(&str) -> Ident,
    allow_repeat: bool,
) -> Option<(Vec<GridTemplateComponent>, Vec<Vec<Ident>>)> {
    let mut tracks = Vec::new();
    let mut names: Vec<Vec<Ident>> = vec![Vec::new()];
    for tok in tokens(s) {
        if let Some(inner) = tok.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            names.last_mut().unwrap().extend(inner.split_whitespace().map(&mut *intern));
        } else if let Some(inner) = tok.strip_prefix("repeat(").and_then(|r| r.strip_suffix(')')) {
            if !allow_repeat {
                return None;
            }
            let (count, rest) = inner.split_once(',')?;
            let count = match count.trim() {
                "auto-fill" => RepetitionCount::AutoFill,
                "auto-fit" => RepetitionCount::AutoFit,
                n => RepetitionCount::Count(n.parse().ok()?),
            };
            let (inner_tracks, inner_names) = parse_track_list_inner(rest, intern, false)?;
            let inner_tracks: Vec<TrackSizingFunction> = inner_tracks
                .into_iter()
                .map(|c| match c {
                    GridTemplateComponent::Single(t) => t,
                    GridTemplateComponent::Repeat(_) => unreachable!(),
                })
                .collect();
            let line_names = if inner_names.iter().all(Vec::is_empty) { Vec::new() } else { inner_names };
            tracks.push(GridTemplateComponent::Repeat(GridTemplateRepetition { count, tracks: inner_tracks, line_names }));
            names.push(Vec::new());
        } else {
            tracks.push(GridTemplateComponent::Single(parse_track_sizing_function(tok)?));
            names.push(Vec::new());
        }
    }
    Some((tracks, names))
}

/// `grid-template-*`: components plus `tracks + 1` line-name sets (empty when unnamed).
pub fn parse_grid_template(
    s: &str,
    intern: &mut impl FnMut(&str) -> Ident,
) -> Option<(Vec<GridTemplateComponent>, Vec<Vec<Ident>>)> {
    if s.trim() == "none" {
        return Some((Vec::new(), Vec::new()));
    }
    let (tracks, names) = parse_track_list_inner(s, intern, true)?;
    let names = if names.iter().all(Vec::is_empty) { Vec::new() } else { names };
    Some((tracks, names))
}

/// `grid-auto-*`.
pub fn parse_grid_auto_tracks(s: &str) -> Option<Vec<TrackSizingFunction>> {
    tokens(s).into_iter().map(parse_track_sizing_function).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_template_with_names_and_repeat() {
        let mut names = Vec::new();
        let mut intern = |s: &str| {
            let pos = names.iter().position(|n| n == s).unwrap_or_else(|| {
                names.push(s.to_owned());
                names.len() - 1
            });
            Ident(pos as u32)
        };
        let (tracks, line_names) = parse_grid_template("[a] 10px [b c] repeat(2, [x] 1fr) [d]", &mut intern).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(line_names, vec![vec![Ident(0)], vec![Ident(1), Ident(2)], vec![Ident(4)]]);
        assert!(matches!(&tracks[1], GridTemplateComponent::Repeat(r) if r.count == RepetitionCount::Count(2) && r.line_names.len() == 2));
        assert_eq!(parse_track_sizing_function("fit-content(50%)").unwrap().max, MaxTrackSizingFunction::fit_content_percent(0.5));
        assert_eq!(parse_track_sizing_function("2fr").unwrap().min, MinTrackSizingFunction::AUTO);
    }
}
