//! Grid container and item properties.

mod placement;
mod track;

pub use placement::{GridLine, GridPlacement, OriginZeroLine};
pub use track::{
    GridTemplateComponent, GridTemplateRepetition, MaxTrackSizingFunction, MinTrackSizingFunction, RepetitionCount,
    TrackSizingFunction,
};

use crate::geometry::AbsoluteAxis;
use crate::style::Ident;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GridAutoFlow {
    #[default]
    Row,
    Column,
    RowDense,
    ColumnDense,
}

impl GridAutoFlow {
    #[inline]
    pub const fn is_dense(self) -> bool {
        matches!(self, Self::RowDense | Self::ColumnDense)
    }
    #[inline]
    pub const fn primary_axis(self) -> AbsoluteAxis {
        match self {
            Self::Row | Self::RowDense => AbsoluteAxis::Horizontal,
            Self::Column | Self::ColumnDense => AbsoluteAxis::Vertical,
        }
    }
}

/// One named area; lines are 1-based and end-exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GridTemplateArea {
    pub name: Ident,
    pub row_start: u16,
    pub row_end: u16,
    pub column_start: u16,
    pub column_end: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GridTemplateAreas {
    pub areas: Vec<GridTemplateArea>,
    pub row_count: u16,
    pub column_count: u16,
}

/// Container-side grid properties, boxed out of [`crate::style::Style`] because most boxes are not grids.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GridContainerStyle {
    pub template_rows: Vec<GridTemplateComponent>,
    pub template_columns: Vec<GridTemplateComponent>,
    /// Line names between template tracks; empty or `tracks + 1` entries.
    pub template_row_names: Vec<Vec<Ident>>,
    pub template_column_names: Vec<Vec<Ident>>,
    pub auto_rows: Vec<TrackSizingFunction>,
    pub auto_columns: Vec<TrackSizingFunction>,
    pub auto_flow: GridAutoFlow,
    pub template_areas: Option<GridTemplateAreas>,
}

impl GridContainerStyle {
    #[inline]
    pub fn template_tracks(&self, axis: AbsoluteAxis) -> &[GridTemplateComponent] {
        match axis {
            AbsoluteAxis::Horizontal => &self.template_columns,
            AbsoluteAxis::Vertical => &self.template_rows,
        }
    }
    #[inline]
    pub fn template_names(&self, axis: AbsoluteAxis) -> &[Vec<Ident>] {
        match axis {
            AbsoluteAxis::Horizontal => &self.template_column_names,
            AbsoluteAxis::Vertical => &self.template_row_names,
        }
    }
    #[inline]
    pub fn auto_tracks(&self, axis: AbsoluteAxis) -> &[TrackSizingFunction] {
        match axis {
            AbsoluteAxis::Horizontal => &self.auto_columns,
            AbsoluteAxis::Vertical => &self.auto_rows,
        }
    }
}
