//! §9.2 flex base size, hypothetical main size and the automatic minimum size.

use super::axis::FlexDir;
use super::axis::{FlexAxisRect, FlexAxisSize, FlexAxisSum, abs_main, from_cross};
use super::{AlgoConstants, FlexItem};
use crate::compute::common::sizing_keyword::{SizingKeywordResolution, resolve_sizing_keyword};
use crate::geometry::{AvailableSpace, MaybeMath, Size};
use crate::style::{AlignSelf, BoxSizing, Overflow};
use crate::compute::scratch::Scratch;
use crate::compute::{Questions, size_request};
use crate::tree::{CacheAccess, LayoutOutput, LayoutTree, SizingMode};

/// Per-item state carried from the basis phase to the minimum-size phase.
struct Pending {
    transferred_min_size: Size<Option<f32>>,
    transferred_max_size: Size<Option<f32>>,
    /// Index of the basis measurement in the batch, when the basis needs measuring.
    basis_request: Option<usize>,
    /// Index of the min-content measurement in the batch, when the automatic minimum needs it.
    min_request: Option<usize>,
}

pub(super) fn determine_flex_base_size<D: FlexDir, T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    c: &AlgoConstants<D>,
    available_space: Size<AvailableSpace>,
    items: &mut [FlexItem],
) {
    let dir = c.dir;
    let read_main = |out: &LayoutOutput| out.size.get(abs_main(dir));
    let mut questions: Questions<f32> = Questions::new(tree, items.len() * 2);
    let mut pending: Scratch<Pending> = Scratch::with_capacity(items.len());
    let mut observes_cross = false;
    for child in items.iter_mut() {
        let style = tree.style(child.node);
        let calc = |id, basis| tree.resolve_calc(id, basis);
        let transferred_min_size = child.min_size.maybe_apply_aspect_ratio(child.aspect_ratio);
        let transferred_max_size = child.max_size.maybe_apply_aspect_ratio(child.aspect_ratio);

        let plan = tree.style_plan(child.node);
        // The item's padding and border resolve against the container's inline size on all four
        // sides, as they did when the item was generated.
        let box_sizing_adjustment =
            if style.box_sizing == BoxSizing::ContentBox { (child.padding + child.border).main_axis_sum(dir) } else { 0.0 };
        let percent_resolution_main_size = if c.known_main_size_is_definite { c.node_inner_size.main(dir) } else { None };
        let flex_basis_style = style.flex_basis;
        let flex_basis = match plan {
            Some(p) if !p.flex_basis_is_relative => p.flex_basis,
            _ => flex_basis_style.resolve(percent_resolution_main_size, &calc),
        }
        .maybe_add(Some(box_sizing_adjustment));

        // A definite flex basis, or the main space a keyword measures the basis in.
        let basis: Result<f32, Option<AvailableSpace>> = {
            let main_size = child.size.main(dir);
            let main_stretch_size = percent_resolution_main_size.maybe_sub(child.margin.main_axis_sum(dir)).maybe_max(0.0);
            if flex_basis_style.is_content() {
                Err(None)
            } else if flex_basis_style.is_sizing_keyword() {
                match resolve_sizing_keyword(flex_basis_style.raw(), main_stretch_size, percent_resolution_main_size) {
                    Some(SizingKeywordResolution::Exact(size)) => Ok(size),
                    Some(SizingKeywordResolution::Measure(a)) => Err(Some(a)),
                    None => Err(None),
                }
            } else if let Some(basis) = flex_basis {
                Ok(basis)
            } else if flex_basis_style.uses_percentage() {
                // A percentage basis against an indefinite main size is `content` (§7.2.3).
                Err(None)
            } else if let Some(size) = main_size {
                Ok(size)
            } else {
                match resolve_sizing_keyword(child.size_style.main(dir).raw(), main_stretch_size, percent_resolution_main_size) {
                    Some(SizingKeywordResolution::Exact(size)) => Ok(size),
                    Some(SizingKeywordResolution::Measure(a)) => Err(Some(a)),
                    None => Err(None),
                }
            }
        };
        child.flex_basis_is_definite = basis.is_ok();
        child.flex_basis = basis.unwrap_or(f32::NAN);
        let automatic_min: Size<Option<f32>> = Size {
            width: child.overflow.x.maybe_into_automatic_min_size(),
            height: child.overflow.y.maybe_into_automatic_min_size(),
        };
        let measures_min = child.min_size.or(automatic_min).main(dir).is_none();

        // The item's questions, when there are any.
        let mut basis_request = None;
        let mut min_request = None;
        if basis.is_err() || measures_min {
            let cross_axis_parent_size = c.node_inner_size.cross(dir);
            let child_parent_size = from_cross(dir, cross_axis_parent_size);
            let child_min_cross = transferred_min_size.cross(dir);
            let child_max_cross = transferred_max_size.cross(dir);
            // Clamping turns min-content and max-content cross space into different definite sizes.
            let clamp_tells_apart =
                (child_min_cross.is_some() || child_max_cross.is_some()) && !available_space.cross(dir).is_definite();
            // Whether the item's questions carry that space: as their own, or as a stretched size.
            let mut carries_cross_space = true;
            // The space for the item's border box: its margins are subtracted here, not by the item.
            let mut cross_axis_available_space = match available_space.cross(dir) {
                AvailableSpace::Definite(val) => AvailableSpace::Definite(
                    (c.divided_cross_space(cross_axis_parent_size.unwrap_or(val)) - child.margin.cross_axis_sum(dir))
                        .max(0.0)
                        .maybe_clamp(child_min_cross, child_max_cross),
                ),
                AvailableSpace::MinContent => child_min_cross.map_or(AvailableSpace::MinContent, AvailableSpace::Definite),
                AvailableSpace::MaxContent => child_max_cross.map_or(AvailableSpace::MaxContent, AvailableSpace::Definite),
            };

            let mut child_cross_size_is_definite = child.size.cross(dir).is_some();
            let mut stretched_from_space = false;
            let child_known_dimensions = {
                let mut ckd = child.size.with_main(dir, None);
                ckd.set_cross(dir, ckd.cross(dir).maybe_clamp(transferred_min_size.cross(dir), transferred_max_size.cross(dir)));
                let keyword = match ckd.cross(dir) {
                    Some(_) => None,
                    None => cross_sizing_keyword(c, child),
                };
                match keyword {
                    Some(SizingKeywordResolution::Exact(size)) => {
                        ckd.set_cross(dir, Some(size.maybe_clamp(transferred_min_size.cross(dir), transferred_max_size.cross(dir))));
                    }
                    Some(SizingKeywordResolution::Measure(space)) => {
                        cross_axis_available_space = space;
                        carries_cross_space = false;
                    }
                    None => {
                        if child.align_self == AlignSelf::STRETCH
                            && !child.margin_is_auto.cross_start(dir)
                            && !child.margin_is_auto.cross_end(dir)
                            && ckd.cross(dir).is_none()
                        {
                            let stretched = match c.indefinite_stretch_cross {
                                Some(cross) => Some(
                                    (cross - child.margin.cross_axis_sum(dir))
                                        .maybe_clamp(transferred_min_size.cross(dir), transferred_max_size.cross(dir)),
                                ),
                                None => {
                                    stretched_from_space = true;
                                    cross_axis_available_space.into_option()
                                }
                            };
                            ckd.set_cross(dir, stretched.maybe_max(0.0));
                            child_cross_size_is_definite = !c.is_wrap && c.has_definite_cross_size && cross_axis_parent_size.is_some();
                        }
                    }
                }
                ckd
            };
            // A known cross size replaces the available cross space in the item's questions.
            observes_cross |=
                clamp_tells_apart && carries_cross_space && (stretched_from_space || child_known_dimensions.cross(dir).is_none());

            if let Err(keyword_main_available_space) = basis {
                if child_cross_size_is_definite
                    && let (Some(ratio), Some(cross)) = (child.aspect_ratio, child_known_dimensions.cross(dir))
                {
                    child.flex_basis_is_definite = true;
                    child.flex_basis = if dir.is_row() { cross * ratio } else { cross / ratio };
                } else {
                    let main_available =
                        keyword_main_available_space.unwrap_or(if available_space.main(dir) == AvailableSpace::MinContent {
                            AvailableSpace::MinContent
                        } else {
                            AvailableSpace::MaxContent
                        });
                    let child_available_space =
                        Size::MAX_CONTENT.with_main(dir, main_available).with_cross(dir, cross_axis_available_space);
                    let request = size_request(
                        child.node,
                        child_known_dimensions,
                        child_parent_size,
                        child_available_space,
                        SizingMode::ContentSize,
                        abs_main(dir).into(),
                    );
                    basis_request = Some(questions.ask(tree, request, read_main));
                }
            }
            if measures_min {
                let child_available_space = Size::MIN_CONTENT.with_cross(dir, cross_axis_available_space);
                let request = size_request(
                    child.node,
                    child_known_dimensions,
                    child_parent_size,
                    child_available_space,
                    SizingMode::ContentSize,
                    abs_main(dir).into(),
                );
                min_request = Some(questions.ask(tree, request, read_main));
            }
        }
        pending.push(Pending {
            transferred_min_size,
            transferred_max_size,
            basis_request,
            min_request,
        });
    }

    if observes_cross {
        tree.note_observed(crate::compute::observed_bit(dir.cross_axis()));
    }
    // Basis and min-content measurements of different items are independent.
    questions.finish(tree, read_main);

    for (child, p) in items.iter_mut().zip(pending.drain(..)) {
        let Pending { transferred_min_size, transferred_max_size, .. } = p;
        if let Some(i) = p.basis_request {
            child.flex_basis = questions.get(i);
        }
        let padding_border_sum = child.padding.main_axis_sum(dir) + child.border.main_axis_sum(dir);
        child.flex_basis = child.flex_basis.max(padding_border_sum);
        child.inner_flex_basis = child.flex_basis - padding_border_sum;

        let padding_border_axes_sums = (child.padding + child.border).sum_axes().map(Some);
        let automatic_min: Size<Option<f32>> = Size {
            width: child.overflow.x.maybe_into_automatic_min_size(),
            height: child.overflow.y.maybe_into_automatic_min_size(),
        };
        child.resolved_minimum_main_size = match (child.min_size.or(automatic_min).main(dir), p.min_request) {
            (Some(v), _) => v,
            (None, Some(i)) => {
                let min_content_main_size = questions.get(i);
                let clamped = min_content_main_size.maybe_min(child.size.main(dir)).maybe_min(transferred_max_size.main(dir));
                clamped.maybe_max(padding_border_axes_sums.main(dir))
            }
            (None, None) => unreachable!("minimum measurement requested"),
        };
        let _ = Overflow::Visible;

        let hypothetical_inner_min_main = child
            .resolved_minimum_main_size
            .maybe_max(transferred_min_size.main(dir))
            .maybe_max(padding_border_axes_sums.main(dir));
        let hypothetical_inner_size =
            child.flex_basis.maybe_clamp(Some(hypothetical_inner_min_main), transferred_max_size.main(dir));
        let hypothetical_outer_size = hypothetical_inner_size + child.margin.main_axis_sum(dir);
        child.hypothetical_inner_size.set_main(dir, hypothetical_inner_size);
        child.hypothetical_outer_size.set_main(dir, hypothetical_outer_size);
    }
}

/// How a sizing keyword on an item's cross size resolves, measured against the container.
///
/// The same resolution the cross-size step makes, so the main size is measured at the cross size
/// the item later gets.
pub(super) fn cross_sizing_keyword<D: FlexDir>(c: &AlgoConstants<D>, child: &FlexItem) -> Option<SizingKeywordResolution> {
    let dir = c.dir;
    let parent = c.node_inner_size.cross(dir);
    let stretch_size = parent
        .map(|cross| c.divided_cross_space(cross))
        .or(c.indefinite_stretch_cross)
        .maybe_sub(child.margin.cross_axis_sum(dir))
        .maybe_max(0.0);
    resolve_sizing_keyword(child.size_style.cross(dir).raw(), stretch_size, parent)
}

/// The inner cross size of a single-line column container whose own cross size is indefinite.
///
/// A stretched size is definite only against a definite container cross size (§9.8). Otherwise the
/// container's cross size is its fit-content size: the largest outer cross contribution of its
/// items under the available cross space. That is the size a stretched item gets in the final
/// layout, so its main size is measured at it too. Without it, an item whose main size follows
/// from its cross size is measured at the whole available space.
pub(super) fn indefinite_stretch_cross<D: FlexDir, T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    c: &AlgoConstants<D>,
    available_space: Size<AvailableSpace>,
    items: &[FlexItem],
) -> Option<f32> {
    let dir = c.dir;
    if !c.is_column || c.is_wrap || c.node_inner_size.cross(dir).is_some() {
        return None;
    }
    let needs = |item: &FlexItem| {
        item.size.cross(dir).is_none()
            && (item.align_self == AlignSelf::STRETCH || item.size_style.cross(dir).is_sizing_keyword())
    };
    if !items.iter().any(needs) {
        return None;
    }
    let available_cross = available_space.cross(dir);
    let read_cross = |out: &LayoutOutput| out.size.get(dir.cross_axis());
    let mut questions: Questions<f32> = Questions::new(tree, items.len());
    for item in items.iter().filter(|item| item.size.cross(dir).is_none()) {
        let space = match resolve_sizing_keyword(item.size_style.cross(dir).raw(), None, None) {
            Some(SizingKeywordResolution::Measure(space)) => space,
            _ => available_cross.maybe_sub(item.margin.cross_axis_sum(dir)).maybe_max(0.0),
        };
        let request = size_request(
            item.node,
            item.size.with_cross(dir, None),
            c.node_inner_size,
            Size::MAX_CONTENT.with_cross(dir, space),
            SizingMode::ContentSize,
            dir.cross_axis().into(),
        );
        questions.ask(tree, request, read_cross);
    }
    questions.finish(tree, read_cross);
    let mut measured = 0;
    let mut outer = 0.0f32;
    for item in items {
        let min = item.min_size.maybe_apply_aspect_ratio(item.aspect_ratio).cross(dir);
        let max = item.max_size.maybe_apply_aspect_ratio(item.aspect_ratio).cross(dir);
        let padding_border = (item.padding + item.border).cross_axis_sum(dir);
        let inner = match item.size.cross(dir) {
            Some(size) => size,
            None => {
                measured += 1;
                questions.get(measured - 1)
            }
        };
        let size = inner.maybe_clamp(min, max).max(padding_border);
        outer = outer.max(size + item.margin.cross_axis_sum(dir));
    }
    let inset = c.content_box_inset.cross_axis_sum(dir);
    let cross = outer.maybe_clamp(c.min_size.cross(dir).maybe_sub(inset), c.max_size.cross(dir).maybe_sub(inset));
    Some(cross.max(0.0))
}

crate::compute::scratch::pooled!(Pending);
