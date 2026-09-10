//! §9.2 flex base size, hypothetical main size and the automatic minimum size.

use super::axis::{FlexAxisRect, FlexAxisSize, FlexAxisSum, abs_main, from_cross};
use super::{AlgoConstants, FlexItem};
use crate::compute::common::sizing_keyword::{SizingKeywordResolution, resolve_sizing_keyword};
use crate::geometry::{AvailableSpace, MaybeMath, Size};
use crate::style::{AlignSelf, BoxSizing, Overflow};
use crate::compute::scratch::Scratch;
use crate::compute::size_request;
use crate::tree::{CacheAccess, ChildRequest, LayoutOutput, LayoutTree, SizingMode};

/// Per-item state carried from the basis phase to the minimum-size phase.
struct Pending {
    transferred_min_size: Size<Option<f32>>,
    transferred_max_size: Size<Option<f32>>,
    /// Index of the basis measurement in the batch, when the basis needs measuring.
    basis_request: Option<usize>,
    /// Index of the min-content measurement in the batch, when the automatic minimum needs it.
    min_request: Option<usize>,
}

pub(super) fn determine_flex_base_size<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    c: &AlgoConstants,
    available_space: Size<AvailableSpace>,
    items: &mut [FlexItem],
) {
    let dir = c.dir;
    let mut requests: Scratch<ChildRequest> = Scratch::with_capacity(items.len() * 2);
    let mut pending: Scratch<Pending> = Scratch::with_capacity(items.len());
    for child in items.iter_mut() {
        let style = tree.style(child.node);
        let calc = |id, basis| tree.resolve_calc(id, basis);
        let cross_axis_parent_size = c.node_inner_size.cross(dir);
        let child_parent_size = from_cross(dir, cross_axis_parent_size);
        let cross_axis_margin_sum = c.margin.cross_axis_sum(dir);
        let transferred_min_size = child.min_size.maybe_apply_aspect_ratio(child.aspect_ratio);
        let transferred_max_size = child.max_size.maybe_apply_aspect_ratio(child.aspect_ratio);
        let child_min_cross = transferred_min_size.cross(dir).maybe_add(cross_axis_margin_sum);
        let child_max_cross = transferred_max_size.cross(dir).maybe_add(cross_axis_margin_sum);
        let cross_axis_available_space = match available_space.cross(dir) {
            AvailableSpace::Definite(val) => AvailableSpace::Definite(
                c.divided_cross_space(cross_axis_parent_size.unwrap_or(val)).maybe_clamp(child_min_cross, child_max_cross),
            ),
            AvailableSpace::MinContent => child_min_cross.map_or(AvailableSpace::MinContent, AvailableSpace::Definite),
            AvailableSpace::MaxContent => child_max_cross.map_or(AvailableSpace::MaxContent, AvailableSpace::Definite),
        };

        let mut child_cross_size_is_definite = child.size.cross(dir).is_some();
        let child_known_dimensions = {
            let mut ckd = child.size.with_main(dir, None);
            ckd.set_cross(dir, ckd.cross(dir).maybe_clamp(transferred_min_size.cross(dir), transferred_max_size.cross(dir)));
            if child.align_self == AlignSelf::STRETCH
                && !child.margin_is_auto.cross_start(dir)
                && !child.margin_is_auto.cross_end(dir)
                && ckd.cross(dir).is_none()
            {
                ckd.set_cross(dir, cross_axis_available_space.into_option().maybe_sub(child.margin.cross_axis_sum(dir)).maybe_max(0.0));
                child_cross_size_is_definite = !c.is_wrap && c.has_definite_cross_size && cross_axis_parent_size.is_some();
            }
            ckd
        };

        let container_main = c.node_inner_size.main(dir);
        let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox {
            let padding = style.padding.map(|p| p.resolve_or_zero(container_main, &calc));
            let border = style.border.map(|b| b.resolve_or_zero(container_main, &calc));
            (padding + border).sum_axes()
        } else {
            Size::ZERO
        }
        .main(dir);
        let percent_resolution_main_size = if c.known_main_size_is_definite { c.node_inner_size.main(dir) } else { None };
        let flex_basis_style = style.flex_basis;
        let flex_basis = flex_basis_style.resolve(percent_resolution_main_size, &calc).maybe_add(Some(box_sizing_adjustment));

        let mut basis_request = None;
        child.flex_basis = 'flex_basis: {
            let main_size = child.size.main(dir);
            let main_stretch_size = percent_resolution_main_size.maybe_sub(child.margin.main_axis_sum(dir)).maybe_max(0.0);
            let keyword_main_available_space = if flex_basis_style.is_content() {
                None
            } else if flex_basis_style.is_sizing_keyword() {
                match resolve_sizing_keyword(flex_basis_style.raw(), main_stretch_size, percent_resolution_main_size) {
                    Some(SizingKeywordResolution::Exact(size)) => {
                        child.flex_basis_is_definite = true;
                        break 'flex_basis size;
                    }
                    Some(SizingKeywordResolution::Measure(a)) => Some(a),
                    None => None,
                }
            } else {
                if let Some(basis) = flex_basis.or(main_size) {
                    child.flex_basis_is_definite = true;
                    break 'flex_basis basis;
                }
                match resolve_sizing_keyword(child.size_style.main(dir).raw(), main_stretch_size, percent_resolution_main_size) {
                    Some(SizingKeywordResolution::Exact(size)) => {
                        child.flex_basis_is_definite = true;
                        break 'flex_basis size;
                    }
                    Some(SizingKeywordResolution::Measure(a)) => Some(a),
                    None => None,
                }
            };
            if child_cross_size_is_definite
                && let (Some(ratio), Some(cross)) = (child.aspect_ratio, child_known_dimensions.cross(dir))
            {
                child.flex_basis_is_definite = true;
                break 'flex_basis if dir.is_row() { cross * ratio } else { cross / ratio };
            }
            let main_available = keyword_main_available_space.unwrap_or(if available_space.main(dir) == AvailableSpace::MinContent {
                AvailableSpace::MinContent
            } else {
                AvailableSpace::MaxContent
            });
            let child_available_space =
                Size::MAX_CONTENT.with_main(dir, main_available).with_cross(dir, cross_axis_available_space);
            basis_request = Some(requests.len());
            requests.push(size_request(
                child.node,
                child_known_dimensions,
                child_parent_size,
                child_available_space,
                SizingMode::ContentSize,
                abs_main(dir).into(),
            ));
            f32::NAN
        };

        let automatic_min: Size<Option<f32>> = Size {
            width: child.overflow.x.maybe_into_automatic_min_size(),
            height: child.overflow.y.maybe_into_automatic_min_size(),
        };
        let min_request = child.min_size.or(automatic_min).main(dir).is_none().then(|| {
            let child_available_space = Size::MIN_CONTENT.with_cross(dir, cross_axis_available_space);
            requests.push(size_request(
                child.node,
                child_known_dimensions,
                child_parent_size,
                child_available_space,
                SizingMode::ContentSize,
                abs_main(dir).into(),
            ));
            requests.len() - 1
        });
        pending.push(Pending {
            transferred_min_size,
            transferred_max_size,
            basis_request,
            min_request,
        });
    }

    // Basis and min-content measurements of different items are independent.
    let mut outputs: Scratch<LayoutOutput> = Scratch::with_capacity(requests.len());
    tree.compute_child_layouts(&requests, &mut outputs);

    for (child, p) in items.iter_mut().zip(pending.drain(..)) {
        let Pending { transferred_min_size, transferred_max_size, .. } = p;
        if let Some(i) = p.basis_request {
            child.flex_basis = outputs[i].size.get(abs_main(dir));
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
                let min_content_main_size = outputs[i].size.get(abs_main(dir));
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

crate::compute::scratch::pooled!(Pending);
