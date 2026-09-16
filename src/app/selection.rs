use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};

/// A text selection made by dragging the mouse, limited to the pane where the drag started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Position,
    pub cursor: Position,
    pub region: Rect,
}

impl Selection {
    /// Selected cells per screen row as `(y, first x, last x)`, in reading order.
    pub fn rows(&self) -> Vec<(u16, u16, u16)> {
        let (start, end) = if (self.anchor.y, self.anchor.x) <= (self.cursor.y, self.cursor.x) {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        };
        let last_x = self.region.right().saturating_sub(1);
        (start.y..=end.y)
            .map(|y| {
                let first = if y == start.y { start.x } else { self.region.x };
                let last = if y == end.y { end.x } else { last_x };
                (y, first, last)
            })
            .collect()
    }

    pub fn text(&self, screen: &Buffer) -> String {
        self.rows()
            .into_iter()
            .map(|(y, first, last)| {
                let line: String = (first..=last)
                    .filter_map(|x| {
                        screen
                            .cell(Position::new(x, y))
                            .map(|cell| cell.symbol().to_string())
                    })
                    .collect();
                line.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn clamp(&self, pos: Position) -> Position {
        let region = self.region;
        Position::new(
            pos.x.clamp(region.x, region.right().saturating_sub(1)),
            pos.y.clamp(region.y, region.bottom().saturating_sub(1)),
        )
    }
}
