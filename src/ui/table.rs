//! One way to draw every table: bordered pane, bold header, highlight bar, empty message.

use super::{empty_message, header, pane, selected_style};
use crate::app::{Hitboxes, Target};
use crate::theme::MAUVE;
use ratatui::Frame;
use ratatui::layout::{Constraint, Position, Rect};
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{HighlightSpacing, Row, Table, TableState};

pub struct TableSpec<'a> {
    pub title: String,
    pub focused: bool,
    pub header: &'a [&'static str],
    pub widths: &'a [Constraint],
    pub empty: &'static str,
}

/// Maps a screen row inside a bordered table with a one-line header to a row index.
pub fn row_at(rect: Rect, offset: usize, y: u16) -> Option<usize> {
    let first = rect.y + 2;
    (rect.height > 3 && y >= first && y + 1 < rect.bottom()).then(|| offset + (y - first) as usize)
}

pub fn hover_row(hover: Option<Position>, area: Rect, state: &TableState) -> Option<usize> {
    let pos = hover?;
    area.contains(pos)
        .then(|| row_at(area, state.offset(), pos.y))
        .flatten()
}

pub fn draw(
    frame: &mut Frame,
    hits: &mut Hitboxes,
    target: Target,
    area: Rect,
    spec: TableSpec,
    rows: Vec<Row<'static>>,
    state: &mut TableState,
) {
    hits.register(target, area);
    let count = rows.len();
    let table = Table::new(rows, spec.widths.to_vec())
        .header(header(spec.header))
        .block(pane(spec.title, spec.focused))
        .row_highlight_style(selected_style(spec.focused))
        .highlight_symbol(Span::styled("▌", Style::new().fg(MAUVE)))
        .highlight_spacing(HighlightSpacing::Always)
        .column_spacing(1);
    frame.render_stateful_widget(table, area, state);
    if count == 0 {
        empty_message(frame, area, spec.empty);
    }
}
