//! CSS grid layout (css-grid-1).

mod alignment;
mod coordinates;
mod explicit;
mod implicit;
mod item;
mod named;
mod occupancy;
mod placement;
mod track;
mod track_sizing;

use alignment::{ContainerAlignment, ItemPrep, align_tracks, place_item, prepare_item};
use coordinates::{MAX_GRID_TRACKS, NonNamedPlacement, PlacementLine};
use explicit::{AutoRepeatStrategy, compute_explicit_grid_size_in_axis, initialize_grid_tracks};
use implicit::compute_grid_size_estimate;
use named::NamedLineResolver;
use occupancy::CellOccupancyMatrix;
use track::{GridTrack, GridTrackKind};
use track_sizing::{
    TrackSizingParams, determine_if_item_crosses_flexible_or_intrinsic_tracks, estimate_from_base_size,
    estimate_from_max_sizing_function, resolve_item_track_indexes, sort_order_by_key, track_sizing_algorithm,
};

use crate::compute::LayoutTreeExt;
use crate::compute::scratch::Scratch;
use crate::compute::common::overflow::union;
use crate::compute::oof::{AxisStaticPosition, OofEntry, position_bit};
use crate::geometry::{AbsoluteAxis, AbstractAxis, AvailableSpace, Line, MaybeMath, Rect, Size};
use crate::style::{AlignContent, BoxSizing, Direction, JustifyContent, Overflow};
use crate::tree::{
    Baselines, CacheAccess, DetailedGridInfo, DetailedGridItem, DetailedGridTracks, Layout, LayoutInput, LayoutOutput,
    LayoutTree, NodeId, RequestedAxis, RunMode, SizingMode,
};

pub fn compute_grid_layout<T: LayoutTree + CacheAccess + ?Sized>(tree: &mut T, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
    let LayoutInput { known_dimensions, parent_size, mut available_space, run_mode, .. } = inputs;
    // The min-content block size equals the max-content block size (CSS Sizing 3 §5.1). Rows sized
    // under a min-content constraint get no free space, so rows of scroll-container items would
    // stay at their zero minimum.
    if available_space.height == AvailableSpace::MinContent {
        available_space.height = AvailableSpace::MaxContent;
    }
    let style = tree.style(node);
    let calc = |id, basis| tree.resolve_calc(id, basis);
    let direction = style.direction;
    let contain = style.contain;
    let aspect_ratio = style.aspect_ratio;
    let padding = style.padding.map(|p| p.resolve_or_zero(parent_size.width, &calc));
    let border = style.border.map(|b| b.resolve_or_zero(parent_size.width, &calc));
    let padding_border = padding + border;
    let padding_border_size = padding_border.sum_axes();
    let box_sizing_adjustment = if style.box_sizing == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };
    let resolve = |s: Size<crate::style::Length>| crate::compute::block::resolve_size(s, parent_size, &calc);
    let min_size = resolve(style.min_size.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment);
    let max_size = resolve(style.max_size.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment);
    let preferred_size = if inputs.sizing_mode == SizingMode::InherentSize {
        resolve(style.size.map(|v| v.raw())).maybe_apply_aspect_ratio(aspect_ratio).maybe_add(box_sizing_adjustment)
    } else {
        Size::NONE
    };
    let scrollbar_gutter = style.overflow.transpose().map(|o| if o == Overflow::Scroll { style.scrollbar_width } else { 0.0 });
    let is_scroll_container = style.is_scroll_container();
    let mut content_box_inset = padding_border;
    content_box_inset.bottom += scrollbar_gutter.y;
    match direction {
        Direction::Ltr => content_box_inset.right += scrollbar_gutter.x,
        Direction::Rtl => content_box_inset.left += scrollbar_gutter.x,
    }
    let align_content = style.align_content.unwrap_or(AlignContent::STRETCH);
    let justify_content = style.justify_content.unwrap_or(JustifyContent::STRETCH);
    let align_items = style.align_items;
    let justify_items = style.justify_items;
    let gap = style.gap;
    let grid_style = style.grid_shared();

    // A known axis transfers through `aspect-ratio`, as in block layout.
    let styled =
        known_dimensions.or(preferred_size).transfer_aspect_ratio(aspect_ratio, min_size, max_size, padding_border_size);
    let constrained_available_space = Size {
        width: styled.width.map_or(available_space.width, AvailableSpace::Definite),
        height: styled.height.map_or(available_space.height, AvailableSpace::Definite),
    }
    .maybe_clamp(min_size, max_size)
    .maybe_max(padding_border_size);
    let available_grid_space = Size {
        width: constrained_available_space.width.map_definite(|s| s - content_box_inset.horizontal_axis_sum()),
        height: constrained_available_space.height.map_definite(|s| s - content_box_inset.vertical_axis_sum()),
    };
    let outer_node_size = styled.maybe_clamp(min_size, max_size).maybe_max(padding_border_size.map(Some));
    let inner_min_size = min_size.maybe_sub(content_box_inset.sum_axes());
    let inner_max_size = max_size.maybe_sub(content_box_inset.sum_axes());
    let mut inner_node_size = Size {
        width: outer_node_size.width.map(|s| s - content_box_inset.horizontal_axis_sum()),
        height: outer_node_size.height.map(|s| s - content_box_inset.vertical_axis_sum()),
    };

    if run_mode == RunMode::ComputeSize {
        if let Size { width: Some(width), height: Some(height) } = outer_node_size {
            return LayoutOutput::from_outer_size(Size { width, height });
        }
        if inputs.axis == RequestedAxis::Horizontal
            && let Some(width) = outer_node_size.width
        {
            return LayoutOutput::from_outer_size(Size { width, height: 0.0 });
        }
    }

    // 2. Explicit grid.
    let auto_fit_container_size = outer_node_size
        .or(max_size)
        .or(min_size)
        .maybe_clamp(min_size, max_size)
        .maybe_max(padding_border_size.map(Some))
        .maybe_sub(content_box_inset.sum_axes());
    let strategy = |v: Option<f32>| {
        if v.is_some() { AutoRepeatStrategy::MaxRepetitionsThatDoNotOverflow } else { AutoRepeatStrategy::MinRepetitionsThatDoOverflow }
    };
    let (col_auto_repetition_count, grid_template_col_count) = compute_explicit_grid_size_in_axis(
        &grid_style,
        gap,
        auto_fit_container_size.width,
        strategy(outer_node_size.width.or(max_size.width)),
        &calc,
        AbsoluteAxis::Horizontal,
    );
    let (row_auto_repetition_count, grid_template_row_count) = compute_explicit_grid_size_in_axis(
        &grid_style,
        gap,
        auto_fit_container_size.height,
        strategy(outer_node_size.height.or(max_size.height)),
        &calc,
        AbsoluteAxis::Vertical,
    );
    let mut name_resolver = NamedLineResolver::new(tree, &grid_style, col_auto_repetition_count, row_auto_repetition_count);
    let explicit_col_count = grid_template_col_count.max(name_resolver.area_column_count()).min(MAX_GRID_TRACKS);
    let explicit_row_count = grid_template_row_count.max(name_resolver.area_row_count()).min(MAX_GRID_TRACKS);
    name_resolver.set_explicit_column_count(explicit_col_count);
    name_resolver.set_explicit_row_count(explicit_row_count);
    let wants_detailed = tree.wants_detailed_grid_info();
    let detailed_column_line_names = if wants_detailed { name_resolver.detailed_line_names(AbsoluteAxis::Horizontal) } else { Vec::new() };
    let detailed_row_line_names = if wants_detailed { name_resolver.detailed_line_names(AbsoluteAxis::Vertical) } else { Vec::new() };

    // 3. Implicit grid estimate and 4. placement.
    let placements = placement::resolve_placements(tree, node, &name_resolver, explicit_col_count, explicit_row_count);
    let (est_col_counts, est_row_counts) = compute_grid_size_estimate(explicit_col_count, explicit_row_count, placements.iter().map(|(_, _, p)| p));
    let mut items = Scratch::with_capacity(placements.len());
    let mut matrix = CellOccupancyMatrix::with_track_counts(est_col_counts, est_row_counts);
    placement::place_grid_items(
        tree,
        &mut matrix,
        &mut items,
        &placements,
        grid_style.auto_flow,
        align_items,
        justify_items,
    );
    drop(placements);
    let final_col_counts = *matrix.track_counts(AbsoluteAxis::Horizontal);
    let final_row_counts = *matrix.track_counts(AbsoluteAxis::Vertical);

    // 5. Tracks.
    let mut columns: Scratch<GridTrack> = Scratch::with_capacity(2 * final_col_counts.len() + 1);
    let mut rows: Scratch<GridTrack> = Scratch::with_capacity(2 * final_row_counts.len() + 1);
    initialize_grid_tracks(&mut columns, final_col_counts, &grid_style, gap.width, AbsoluteAxis::Horizontal, col_auto_repetition_count, |i| {
        matrix.column_is_occupied(i)
    });
    initialize_grid_tracks(&mut rows, final_row_counts, &grid_style, gap.height, AbsoluteAxis::Vertical, row_auto_repetition_count, |i| {
        matrix.row_is_occupied(i)
    });
    resolve_item_track_indexes(&mut items, final_col_counts, final_row_counts);
    determine_if_item_crosses_flexible_or_intrinsic_tracks(&mut items, &columns, &rows);
    let has_baseline_aligned_item = items.iter().any(|i| i.participates_in_baseline_alignment());
    // The items stay in placement order; the passes sort this permutation of them instead.
    let mut item_order: Scratch<u32> = Scratch::collect(0..items.len() as u32);

    // 6. Track sizing, inline then block.
    track_sizing_algorithm(
        tree,
        TrackSizingParams {
            axis: AbstractAxis::Inline,
            axis_min_size: inner_min_size.width,
            axis_max_size: inner_max_size.width,
            axis_alignment: justify_content,
            other_axis_alignment: align_content,
            available_grid_space,
            inner_node_size,
            axis_tracks: &mut columns,
            other_axis_tracks: &mut rows,
            items: &mut items,
            order: &mut item_order,
            get_track_size_estimate: estimate_from_max_sizing_function,
            has_baseline_aligned_item,
            run_mode,
        },
    );
    let initial_column_sum: f32 = columns.iter().map(|t| t.base_size).sum();
    inner_node_size.width = inner_node_size.width.or(Some(initial_column_sum));
    // A width question needs no rows: sizing them measures every item's height. The width is the
    // one the full run returns below; `verify-cache` runs on and checks it there.
    let width_only = (run_mode == RunMode::ComputeSize && inputs.axis == RequestedAxis::Horizontal).then(|| {
        styled
            .width
            .unwrap_or(initial_column_sum + content_box_inset.horizontal_axis_sum())
            .maybe_clamp(min_size.width, max_size.width)
            .max(padding_border_size.width)
    });
    #[cfg(not(feature = "verify-cache"))]
    if let Some(width) = width_only {
        return LayoutOutput::from_outer_size(Size { width, height: 0.0 });
    }
    for item in items.iter_mut() {
        item.grid_area_size_cache = None;
    }
    track_sizing_algorithm(
        tree,
        TrackSizingParams {
            axis: AbstractAxis::Block,
            axis_min_size: inner_min_size.height,
            axis_max_size: inner_max_size.height,
            axis_alignment: align_content,
            other_axis_alignment: justify_content,
            available_grid_space,
            inner_node_size,
            axis_tracks: &mut rows,
            other_axis_tracks: &mut columns,
            items: &mut items,
            order: &mut item_order,
            get_track_size_estimate: estimate_from_base_size,
            has_baseline_aligned_item: false,
            run_mode,
        },
    );
    let initial_row_sum: f32 = rows.iter().map(|t| t.base_size).sum();

    let resolved_style_size = styled;
    let mut container_border_box = Size {
        width: resolved_style_size
            .width
            .unwrap_or(initial_column_sum + content_box_inset.horizontal_axis_sum())
            .maybe_clamp(min_size.width, max_size.width)
            .max(padding_border_size.width),
        height: resolved_style_size
            .height
            .unwrap_or(initial_row_sum + content_box_inset.vertical_axis_sum())
            .maybe_clamp(min_size.height, max_size.height)
            .max(padding_border_size.height),
    };
    let mut container_content_box = Size {
        width: (container_border_box.width - content_box_inset.horizontal_axis_sum()).max(0.0),
        height: (container_border_box.height - content_box_inset.vertical_axis_sum()).max(0.0),
    };
    if run_mode == RunMode::ComputeSize {
        #[cfg(feature = "verify-cache")]
        if let Some(width) = width_only {
            assert!(width.to_bits() == container_border_box.width.to_bits(), "grid width-only answer {width:?} differs: {container_border_box:?}");
        }
        return LayoutOutput::from_outer_size(container_border_box);
    }

    // 7. With the container's size known, percentages resolve against its content box: rerun an
    // axis whose size was indefinite and that has percentage tracks or gaps, and an axis whose
    // items' contributions moved.
    inner_node_size = container_content_box.map(Some);
    let has_percentage_column = columns.iter().any(|t| t.uses_percentage());
    let has_percentage_row = rows.iter().any(|t| t.uses_percentage());
    let mut intrinsic_column_contribution_changed = false;
    let mut rerun_column_sizing = outer_node_size.width.is_none() && has_percentage_column;
    if !rerun_column_sizing {
        intrinsic_column_contribution_changed = recheck_contributions(tree, &mut items, &item_order, AbstractAxis::Inline, &columns, &rows, inner_node_size);
        rerun_column_sizing = intrinsic_column_contribution_changed;
    } else {
        for item in items.iter_mut() {
            item.grid_area_size_cache = None;
            item.min_content_contribution_cache.width = None;
            item.max_content_contribution_cache.width = None;
            item.minimum_contribution_cache.width = None;
        }
    }
    let mut intrinsic_row_contribution_changed = false;
    let mut rerun_row_sizing = outer_node_size.height.is_none() && has_percentage_row;
    if rerun_row_sizing {
        for item in items.iter_mut() {
            item.grid_area_size_cache = None;
            item.min_content_contribution_cache.height = None;
            item.max_content_contribution_cache.height = None;
            item.minimum_contribution_cache.height = None;
        }
    }
    if rerun_column_sizing {
        track_sizing_algorithm(
            tree,
            TrackSizingParams {
                axis: AbstractAxis::Inline,
                axis_min_size: inner_min_size.width,
                axis_max_size: inner_max_size.width,
                axis_alignment: justify_content,
                other_axis_alignment: align_content,
                available_grid_space,
                inner_node_size,
                axis_tracks: &mut columns,
                other_axis_tracks: &mut rows,
                items: &mut items,
                order: &mut item_order,
                get_track_size_estimate: estimate_from_base_size,
                has_baseline_aligned_item,
                run_mode,
            },
        );
        // The columns moved, so the items' heights may have; unless the rows rerun anyway.
        if !rerun_row_sizing {
            intrinsic_row_contribution_changed = recheck_contributions(tree, &mut items, &item_order, AbstractAxis::Block, &rows, &columns, inner_node_size);
            rerun_row_sizing = intrinsic_row_contribution_changed;
        }
    }
    if rerun_row_sizing {
        track_sizing_algorithm(
            tree,
            TrackSizingParams {
                axis: AbstractAxis::Block,
                axis_min_size: inner_min_size.height,
                axis_max_size: inner_max_size.height,
                axis_alignment: align_content,
                other_axis_alignment: justify_content,
                available_grid_space,
                inner_node_size,
                axis_tracks: &mut rows,
                other_axis_tracks: &mut columns,
                items: &mut items,
                order: &mut item_order,
                get_track_size_estimate: estimate_from_base_size,
                has_baseline_aligned_item: false,
                run_mode,
            },
        );
    }
    if intrinsic_column_contribution_changed && !has_percentage_column {
        let sum: f32 = columns.iter().map(|t| t.base_size).sum();
        container_border_box.width = resolved_style_size
            .width
            .unwrap_or(sum + content_box_inset.horizontal_axis_sum())
            .maybe_clamp(min_size.width, max_size.width)
            .max(padding_border_size.width);
        container_content_box.width = (container_border_box.width - content_box_inset.horizontal_axis_sum()).max(0.0);
    }
    if intrinsic_row_contribution_changed && !has_percentage_row {
        let sum: f32 = rows.iter().map(|t| t.base_size).sum();
        container_border_box.height = resolved_style_size
            .height
            .unwrap_or(sum + content_box_inset.vertical_axis_sum())
            .maybe_clamp(min_size.height, max_size.height)
            .max(padding_border_size.height);
        container_content_box.height = (container_border_box.height - content_box_inset.vertical_axis_sum()).max(0.0);
    }

    // 8. Track alignment.
    let inline_size_without_scrollbar = (container_border_box.width - padding_border_size.width).max(0.0);
    let inline_gutter_for_alignment = scrollbar_gutter.x.min(inline_size_without_scrollbar);
    let rtl = direction == Direction::Rtl;
    align_tracks(
        container_content_box.width,
        Line {
            start: padding.left + if rtl { inline_gutter_for_alignment } else { 0.0 },
            end: padding.right + if rtl { 0.0 } else { inline_gutter_for_alignment },
        },
        Line { start: border.left, end: border.right },
        &mut columns,
        justify_content,
        rtl,
    );
    align_tracks(
        container_content_box.height,
        Line { start: padding.top, end: padding.bottom },
        Line { start: border.top, end: border.bottom },
        &mut rows,
        align_content,
        false,
    );

    // 9. Items.
    let mut item_overflow_rect = Rect::ZERO;
    let mut through: Vec<OofEntry> = Vec::new();
    sort_order_by_key(&mut item_order, &items, |i| i.source_order as u64);
    let container_alignment = ContainerAlignment { horizontal: justify_items, vertical: align_items };
    let mut preps: Scratch<ItemPrep> = Scratch::with_capacity(items.len());
    for &i in item_order.iter() {
        let item = &items[i as usize];
        let grid_area = Rect {
            top: rows[item.row_indexes.start as usize + 1].offset,
            bottom: rows[item.row_indexes.end as usize].offset,
            left: if rtl { columns[item.column_indexes.end as usize - 1].offset } else { columns[item.column_indexes.start as usize + 1].offset },
            right: if rtl { columns[item.column_indexes.start as usize].offset } else { columns[item.column_indexes.end as usize].offset },
        };
        preps.push(prepare_item(tree, item.node, grid_area, &container_alignment, item.baseline_shim, direction));
    }
    // Every item's final layout is independent of the others. A tree answering one question at a
    // time lays each item out just before placing it, in the order it would ask them, and keeps no
    // answer; a distributing tree asks them as one batch.
    let streamed = !tree.batches_help();
    let mut outputs: Scratch<LayoutOutput> = Scratch::with_capacity(if streamed { 0 } else { preps.len() });
    if !streamed {
        let requests: Scratch<crate::tree::ChildRequest> = Scratch::collect(preps.iter().map(ItemPrep::request));
        tree.compute_child_layouts(&requests, &mut outputs);
    }
    for ((index, &i), prep) in item_order.iter().enumerate().zip(preps.iter()) {
        let out = if streamed {
            let request = prep.request();
            tree.compute_child_layout(request.node, request.input, None)
        } else {
            outputs[index]
        };
        let item = &mut items[i as usize];
        let last_baseline = out.baselines.last;
        let oof = out.oof;
        let (contribution, location, height) =
            place_item(tree, prep, out, index as u32, direction, container_border_box.width, border, is_scroll_container);
        if oof != 0 {
            through.push(OofEntry::Through { node: item.node, order: item.source_order as u32, bits: oof, location });
        }
        let y = location.y;
        item.y_position = y;
        item.height = height;
        // Scroll containers clamp their baseline to the border box, synthesising one from the bottom edge.
        let last_baseline = last_baseline.unwrap_or(height);
        item.last_baseline = if item.overflow.y.is_scroll_container() { last_baseline.min(height).max(0.0) } else { last_baseline };
        item_overflow_rect = union(item_overflow_rect, contribution);
    }

    // Hidden children, and the out-of-flow record: the out-of-flow children and the items passing
    // candidates through, in document order (see `crate::compute::oof`).
    let speculating = tree.speculation() > 0;
    let mut entries: Vec<OofEntry> = Vec::new();
    let mut order = items.len() as u32;
    // Every child not placed as an item is hidden or out of flow; usually there is none.
    if items.len() < tree.child_count(node) || !through.is_empty() {
        let claims = tree.containing_block_claims(node);
        let mut through = through.iter().peekable();
        let children: Scratch<NodeId> = Scratch::collect(tree.children(node));
        for (index, &child) in children.iter().enumerate() {
            if let Some(entry) = through.next_if(|e| e.order() == index as u32) {
                entries.push(*entry);
                continue;
            }
            let child_style = tree.style(child);
            if child_style.generates_no_box() {
                tree.set_unrounded_layout(child, &Layout::with_order(order));
                tree.perform_child_layout(child, Size::NONE, Size::NONE, Size::MAX_CONTENT, SizingMode::InherentSize, Line::FALSE);
                order += 1;
                continue;
            }
            if !child_style.position.is_out_of_flow() {
                continue;
            }
            if !speculating {
                let position = child_style.position;
                let item_direction = child_style.direction;
                let justify_self = child_style.justify_self.or(justify_items).map(|a| a.resolve_self_relative(item_direction, direction, true));
                let align_self = child_style.align_self.or(align_items).map(|a| a.resolve_self_relative(item_direction, direction, false));
                // The grid area its placement names when this grid is its containing block, else
                // the content box, as for a sole item spanning the grid (css-grid-1 §9.3).
                let area = if claims & position_bit(position) != 0 {
                    let (grid_row, grid_column) = (child_style.grid_row, child_style.grid_column);
                    static_position_grid_area(
                        tree,
                        &name_resolver,
                        grid_row,
                        grid_column,
                        (final_col_counts, final_row_counts),
                        (&columns, &rows),
                        direction,
                        border,
                        scrollbar_gutter,
                        container_border_box,
                    )
                } else {
                    Rect {
                        left: border.left + padding.left + if rtl { scrollbar_gutter.x } else { 0.0 },
                        right: container_border_box.width - border.right - padding.right - if rtl { 0.0 } else { scrollbar_gutter.x },
                        top: border.top + padding.top,
                        bottom: container_border_box.height - border.bottom - padding.bottom - scrollbar_gutter.y,
                    }
                };
                entries.push(OofEntry::Seed {
                    node: child,
                    order,
                    bit: position_bit(position),
                    static_position: crate::geometry::Point {
                        x: AxisStaticPosition::from_alignment(justify_self, Line { start: area.left, end: area.right }, rtl),
                        y: AxisStaticPosition::from_alignment(align_self, Line { start: area.top, end: area.bottom }, false),
                    },
                });
            }
            order += 1;
        }
    }
    let (absolute_overflow_rect, oof) = if speculating || entries.is_empty() {
        crate::compute::oof::finish_none(tree, node);
        (Rect::ZERO, 0)
    } else {
        let overflow = tree.style(node).overflow;
        let (columns, rows) = (&columns, &rows);
        crate::compute::oof::finish(tree, node, entries, || {
            let inset = border
                + Rect { left: if rtl { scrollbar_gutter.x } else { 0.0 }, right: if rtl { 0.0 } else { scrollbar_gutter.x }, top: 0.0, bottom: scrollbar_gutter.y };
            crate::compute::oof::OofGeometry {
                area_size: container_border_box - inset.sum_axes(),
                area_offset: crate::geometry::Point { x: inset.left, y: inset.top },
                direction,
                overflow,
                grid: Some(std::sync::Arc::new(GridGeometry {
                    columns: GridAxisGeometry::new(final_col_counts, columns),
                    rows: GridAxisGeometry::new(final_row_counts, rows),
                    column_auto_repetitions: col_auto_repetition_count,
                    row_auto_repetitions: row_auto_repetition_count,
                })),
            }
        })
    };

    let positions = |tracks: &[GridTrack]| -> Vec<Line<f32>> {
        tracks
            .iter()
            .filter(|t| t.kind == GridTrackKind::Track)
            .map(|t| Line { start: t.offset, end: t.offset + t.base_size })
            .collect()
    };
    if wants_detailed {
        tree.set_detailed_grid_info(
            node,
            DetailedGridInfo {
            rows: DetailedGridTracks {
                negative_implicit_tracks: final_row_counts.negative_implicit,
                explicit_tracks: final_row_counts.explicit,
                positive_implicit_tracks: final_row_counts.positive_implicit,
                positions: positions(&rows),
                line_names: detailed_row_line_names,
                offsets: rows.iter().map(|t| t.offset).collect(),
                auto_repetitions: row_auto_repetition_count,
            },
            columns: DetailedGridTracks {
                negative_implicit_tracks: final_col_counts.negative_implicit,
                explicit_tracks: final_col_counts.explicit,
                positive_implicit_tracks: final_col_counts.positive_implicit,
                positions: positions(&columns),
                line_names: detailed_column_line_names,
                offsets: columns.iter().map(|t| t.offset).collect(),
                auto_repetitions: col_auto_repetition_count,
            },
            items: item_order
                .iter()
                .map(|&i| &items[i as usize])
                .map(|i| DetailedGridItem {
                    row_start: i.row_indexes.start / 2 + 1,
                    row_end: i.row_indexes.end / 2 + 1,
                    column_start: i.column_indexes.start / 2 + 1,
                    column_end: i.column_indexes.end / 2 + 1,
                })
                .collect(),
            },
        );
    }

    let mut inflow_overflow_rect = item_overflow_rect;
    if is_scroll_container {
        inflow_overflow_rect.right += if rtl { padding.left } else { padding.right };
        inflow_overflow_rect.bottom += padding.bottom;
    }
    if items.is_empty() {
        let mut output = LayoutOutput::from_outer_size(container_border_box).with_overflow(inflow_overflow_rect, absolute_overflow_rect);
        output.oof = oof;
        return output;
    }

    // The container's baseline is its first row's first baseline-aligned item's, else the first item's.
    // No item takes part in last-baseline alignment, so the last baseline is that of the last item
    // in row-major order among those reaching the last row.
    let baselines = if contain.suppresses_baseline() {
        Baselines::NONE
    } else {
        // Items of one row keep their source order, so scanning in source order finds what a stable
        // sort by row would put first, and `max_by_key` the same last of equal keys.
        let in_source_order = || item_order.iter().map(|&i| &items[i as usize]);
        let first_row = items.iter().map(|i| i.row_indexes.start).min().unwrap_or(0);
        let first_row_items = || in_source_order().filter(|i| i.row_indexes.start == first_row);
        let item = first_row_items()
            .find(|i| i.participates_in_baseline_alignment())
            .or_else(|| first_row_items().next())
            .expect("the first row has an item");
        let first = item.y_position + item.baseline.unwrap_or(item.height);
        let last_row_end = items.iter().map(|i| i.row_indexes.end).max().unwrap_or(0);
        let last = in_source_order()
            .filter(|i| i.row_indexes.end == last_row_end)
            .max_by_key(|i| (i.row_indexes.start, i.column_indexes.start))
            .map(|i| i.y_position + i.last_baseline);
        Baselines { first: Some(first), last }
    };
    let mut output =
        LayoutOutput::from_sizes_and_baselines(container_border_box, Rect::ZERO, baselines).with_overflow(inflow_overflow_rect, absolute_overflow_rect);
    output.oof = oof;
    output
}

/// A grid containing block's tracks, against which its out-of-flow boxes resolve their grid areas.
#[derive(Clone, Debug, PartialEq)]
pub struct GridGeometry {
    columns: GridAxisGeometry,
    rows: GridAxisGeometry,
    column_auto_repetitions: u16,
    row_auto_repetitions: u16,
}

/// One axis of a [`GridGeometry`].
#[derive(Clone, Debug, PartialEq)]
struct GridAxisGeometry {
    counts: coordinates::TrackCounts,
    /// Start and end of every track (gutters excluded), from the container's border box.
    positions: Vec<Line<f32>>,
    /// Sorted runs of collapsed `auto-fit` tracks, as indices into `positions`.
    collapsed: Vec<core::ops::Range<usize>>,
    /// Where an axis without tracks has its one line.
    empty_axis_line: Option<f32>,
}

impl GridAxisGeometry {
    fn new(counts: coordinates::TrackCounts, tracks: &[GridTrack]) -> Self {
        let mut positions = Vec::with_capacity(tracks.len() / 2);
        let mut collapsed: Vec<core::ops::Range<usize>> = Vec::new();
        for track in tracks.iter().filter(|t| t.kind == GridTrackKind::Track) {
            if track.is_collapsed {
                let index = positions.len();
                match collapsed.last_mut() {
                    Some(range) if range.end == index => range.end += 1,
                    _ => collapsed.push(index..index + 1),
                }
            }
            positions.push(Line { start: track.offset, end: track.offset + track.base_size });
        }
        let empty_axis_line = if positions.is_empty() { tracks.first().map(|t| t.offset) } else { None };
        Self { counts, positions, collapsed, empty_axis_line }
    }

    /// The physical start and end of the area `placement` names, between the padding edges where
    /// a line is `auto` or outside the grid.
    fn resolve(&self, placement: Line<NonNamedPlacement>, padding_start: f32, padding_end: f32, is_reversed: bool) -> Line<f32> {
        let counts = self.counts;
        let min_line = -(counts.negative_implicit as i16);
        let max_line = (counts.explicit + counts.positive_implicit) as i16;
        // Sorted into a pair of lines first; only then does a line outside the grid become `auto`.
        let placement = placement
            .map(|p| p.into_origin_zero(counts.explicit))
            .resolve_absolutely_positioned_grid_tracks()
            .map(|line| line.filter(|l| l.0 >= min_line && l.0 <= max_line))
            .map(|line| line.and_then(|l| counts.try_into_track_vec_index(l).map(|index| index / 2)));
        let start = placement
            .start
            .and_then(|line| {
                (line < self.positions.len())
                    .then(|| self.track_edge(line, true, is_reversed))
                    .or_else(|| self.positions.last().map(|t| if is_reversed { t.start } else { t.end }))
                    .or(self.empty_axis_line)
            })
            .unwrap_or(if is_reversed { padding_end } else { padding_start });
        let end = placement
            .end
            .and_then(|line| {
                line.checked_sub(1)
                    .filter(|&l| l < self.positions.len())
                    .map(|l| self.track_edge(l, false, is_reversed))
                    .or_else(|| self.positions.first().map(|t| if is_reversed { t.end } else { t.start }))
                    .or(self.empty_axis_line)
            })
            .unwrap_or(if is_reversed { padding_start } else { padding_end });
        // Lines all within a collapsed run name a zero-size area at its start, as in Chrome.
        if is_reversed { Line { start: start.min(end), end: start } } else { Line { start, end: start.max(end) } }
    }

    /// A track's edge, skipping gutters that overlap across a run of collapsed tracks.
    fn track_edge(&self, index: usize, is_start: bool, is_reversed: bool) -> f32 {
        let run = self.collapsed.partition_point(|r| r.end <= index);
        let track = self
            .collapsed
            .get(run)
            .filter(|r| r.contains(&index))
            .and_then(|r| {
                let adjacent = if is_start { Some(r.end) } else { r.start.checked_sub(1) };
                adjacent.and_then(|i| self.positions.get(i))
            })
            .unwrap_or(&self.positions[index]);
        if is_start != is_reversed { track.start } else { track.end }
    }
}

impl GridGeometry {
    /// The grid area an out-of-flow box with these placements is placed against, from the grid
    /// `cb`'s border box; `padding_box` stands in for `auto` lines.
    pub(crate) fn resolve_absolute_grid_area<T: LayoutTree + ?Sized>(
        &self,
        tree: &T,
        cb: NodeId,
        grid_row: Line<crate::style::GridPlacement>,
        grid_column: Line<crate::style::GridPlacement>,
        direction: Direction,
        padding_box: Rect<f32>,
    ) -> Rect<f32> {
        let grid_style = tree.style(cb).grid_shared();
        let mut resolver = NamedLineResolver::new(tree, &grid_style, self.column_auto_repetitions, self.row_auto_repetitions);
        resolver.set_explicit_column_count(self.columns.counts.explicit);
        resolver.set_explicit_row_count(self.rows.counts.explicit);
        let columns =
            self.columns.resolve(resolver.resolve_column_names(tree, grid_column), padding_box.left, padding_box.right, direction == Direction::Rtl);
        let rows = self.rows.resolve(resolver.resolve_row_names(tree, grid_row), padding_box.top, padding_box.bottom, false);
        Rect { left: columns.start, right: columns.end, top: rows.start, bottom: rows.end }
    }
}

/// The static-position area of an out-of-flow child whose containing block is this grid: the grid
/// area its placement names, gutters excluded, or the padding edges where `auto`.
#[allow(clippy::too_many_arguments)]
fn static_position_grid_area<T: LayoutTree + ?Sized>(
    tree: &T,
    name_resolver: &NamedLineResolver,
    grid_row: Line<crate::style::GridPlacement>,
    grid_column: Line<crate::style::GridPlacement>,
    (col_counts, row_counts): (coordinates::TrackCounts, coordinates::TrackCounts),
    (columns, rows): (&[GridTrack], &[GridTrack]),
    direction: Direction,
    border: Rect<f32>,
    scrollbar_gutter: crate::geometry::Point<f32>,
    container_border_box: Size<f32>,
) -> Rect<f32> {
    let col_indexes = name_resolver
        .resolve_column_names(tree, grid_column)
        .map(|p| p.into_origin_zero(col_counts.explicit))
        .resolve_absolutely_positioned_grid_tracks()
        .map(|l| l.and_then(|l| col_counts.try_into_track_vec_index(l)));
    let row_indexes = name_resolver
        .resolve_row_names(tree, grid_row)
        .map(|p| p.into_origin_zero(row_counts.explicit))
        .resolve_absolutely_positioned_grid_tracks()
        .map(|l| l.and_then(|l| row_counts.try_into_track_vec_index(l)));
    fn line_as_start_edge(tracks: &[GridTrack], i: usize) -> f32 {
        tracks.get(i + 1).unwrap_or(&tracks[i]).offset
    }
    fn line_as_end_edge(tracks: &[GridTrack], i: usize) -> f32 {
        if i == 0 { tracks.get(1).unwrap_or(&tracks[0]).offset } else { tracks[i].offset }
    }
    // In RTL the tracks keep their logical order with offsets assigned right to left: a start line
    // is the right edge of the track after it, an end line the left edge of the track before it.
    fn rtl_line_as_start_edge(tracks: &[GridTrack], i: usize) -> f32 {
        if tracks.len() > i + 1 {
            tracks[i].offset
        } else if i == 0 {
            tracks[0].offset
        } else {
            tracks[i - 1].offset
        }
    }
    fn rtl_line_as_end_edge(tracks: &[GridTrack], i: usize) -> f32 {
        if i == 0 { tracks[0].offset } else { tracks[i - 1].offset }
    }
    let (left, right) = if direction == Direction::Rtl {
        (
            col_indexes.end.map(|i| rtl_line_as_end_edge(columns, i)).unwrap_or(border.left + scrollbar_gutter.x),
            col_indexes.start.map(|i| rtl_line_as_start_edge(columns, i)).unwrap_or(container_border_box.width - border.right),
        )
    } else {
        (
            col_indexes.start.map(|i| line_as_start_edge(columns, i)).unwrap_or(border.left),
            col_indexes.end.map(|i| line_as_end_edge(columns, i)).unwrap_or(container_border_box.width - border.right - scrollbar_gutter.x),
        )
    };
    Rect {
        top: row_indexes.start.map(|i| line_as_start_edge(rows, i)).unwrap_or(border.top),
        bottom: row_indexes.end.map(|i| line_as_end_edge(rows, i)).unwrap_or(container_border_box.height - border.bottom - scrollbar_gutter.y),
        left,
        right,
    }
}

/// Re-measures min-content contributions of items crossing intrinsic tracks; true when any changed.
fn recheck_contributions<T: LayoutTree + CacheAccess + ?Sized>(
    tree: &mut T,
    items: &mut [item::GridItem],
    order: &[u32],
    axis: AbstractAxis,
    axis_tracks: &[GridTrack],
    other_axis_tracks: &[GridTrack],
    inner_node_size: Size<Option<f32>>,
) -> bool {
    let mut changed = false;
    for &i in order {
        let item = &mut items[i as usize];
        if !item.crosses_intrinsic_track(axis) {
            continue;
        }
        let calc = |id, basis| tree.resolve_calc(id, basis);
        let grid_area_size = item.grid_area_size(axis, axis_tracks, other_axis_tracks, inner_node_size, |t, _| Some(t.base_size), &calc);
        let available = grid_area_size.with_abstract(axis, None);
        let new = item.min_content_contribution(axis, tree, grid_area_size, available);
        if Some(new) != item.min_content_contribution_cache.get_abstract(axis) {
            changed = true;
        }
        item.grid_area_size_cache = Some(grid_area_size);
        item.min_content_contribution_cache.set_abstract(axis, Some(new));
        item.max_content_contribution_cache.set_abstract(axis, None);
        item.minimum_contribution_cache.set_abstract(axis, None);
    }
    changed
}

crate::compute::scratch::pooled!(item::GridItem, GridTrack, ItemPrep, u32, (u64, u32));
