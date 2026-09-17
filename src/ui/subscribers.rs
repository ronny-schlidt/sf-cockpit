use super::table::{self, TableSpec, hover_row};
use super::{colored, dim, draw_filter, ready, row_style, value};
use crate::app::{App, Target};
use crate::sf::push::PushData;
use crate::theme::*;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Row};

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    if super::setup_hint(frame, app, area) {
        return;
    }
    if !ready(frame, app, area, &app.push, "Querying subscribers") {
        return;
    }
    let data = app.push.value.take().expect("checked by ready");
    draw_tab(frame, app, &data, area);
    app.push.value = Some(data);
}

fn draw_tab(frame: &mut Frame, app: &mut App, data: &PushData, area: Rect) {
    let [filter_area, table_area] =
        Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(area);
    draw_filter(
        frame,
        app,
        filter_area,
        "Click here or press / to filter by org, name, alias, org ID, instance or version (★ = marked only)",
    );

    let subscribers = app.subscribers_in(data);
    let latest = data
        .latest_released
        .as_deref()
        .map(|id| data.version_label(id))
        .unwrap_or_else(|| "?".into());
    let behind = subscribers
        .iter()
        .filter(|s| !data.is_latest(&s.version_id))
        .count();
    let marked = subscribers
        .iter()
        .filter(|s| app.cfg.is_important(&s.org_key))
        .count();
    let hover = hover_row(app.hover, table_area, &app.subscribers);

    let rows = subscribers
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let current = data.is_latest(&s.version_id);
            let color = if current { GREEN } else { YELLOW };
            let alias = data
                .alias(&s.org_key)
                .map(|o| o.alias.clone())
                .unwrap_or_default();
            let own_name = app.cfg.org_display_name(&s.org_key, "");
            let name = if own_name.is_empty() {
                Line::from(value(s.name.clone()))
            } else {
                Line::from(vec![
                    Span::styled(own_name, Style::new().fg(TEXT).bold()),
                    dim(format!("  {}", s.name)),
                ])
            };
            let star = if app.cfg.is_important(&s.org_key) {
                "★"
            } else {
                ""
            };
            Row::new([
                Cell::from(colored("●", color)),
                Cell::from(colored(star, YELLOW)),
                Cell::from(name),
                Cell::from(colored(alias, LAVENDER)),
                Cell::from(dim(s.org_type.clone())),
                Cell::from(dim(s.org_status.clone())),
                Cell::from(dim(s.instance.clone())),
                Cell::from(colored(s.org_key.clone(), OVERLAY0)),
                Cell::from(Span::styled(
                    data.version_label(&s.version_id),
                    Style::new().fg(color).bold(),
                )),
                Cell::from(colored(if current { "up to date" } else { "behind" }, color)),
            ])
            .style(row_style(hover == Some(i)))
        })
        .collect::<Vec<_>>();
    let count = rows.len();

    table::draw(
        frame,
        &mut app.hits,
        Target::Subscribers,
        table_area,
        TableSpec {
            title: format!(
                "Subscribers · {count} orgs · {marked} marked · {behind} behind latest released {latest}"
            ),
            focused: !app.editing_filter,
            header: &[
                "", "★", "Org", "Alias", "Type", "Status", "Instance", "Org ID", "Version", "",
            ],
            widths: &[
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Fill(2),
                Constraint::Fill(1),
                Constraint::Length(11),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(15),
                Constraint::Length(9),
                Constraint::Length(10),
            ],
            empty: "No subscribers match the filter",
        },
        rows,
        &mut app.subscribers,
    );
}
