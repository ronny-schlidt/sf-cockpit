use crate::app::{Action, App, Pane, Tab, hint};
use crate::sf::{Counts, Data, Timestamp};
use crate::theme::*;
use chrono::{Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Cell, Clear, HighlightSpacing, Paragraph, Row, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Table, TableState, Wrap,
};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(BASE).fg(TEXT)), area);
    app.hits = Default::default();

    let [top, body, bottom] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    let body = body.inner(Margin::new(1, 0));

    draw_top_bar(frame, app, top);

    // Take the data out so panes can borrow it while mutating table state.
    let data = app.data.take();
    match &data {
        Some(data) => match app.tab {
            Tab::Push => draw_push_tab(frame, app, data, body),
            Tab::Subscribers => draw_subscribers_tab(frame, app, data, body),
        },
        None if app.loading => draw_loading(frame, app, body),
        None => draw_load_error(frame, app, body),
    }
    app.data = data;

    draw_bottom_bar(frame, app, bottom);
    draw_selection(frame, app);
}

fn draw_selection(frame: &mut Frame, app: &App) {
    let Some(selection) = app.selection else {
        return;
    };
    let buffer = frame.buffer_mut();
    for (y, first, last) in selection.rows() {
        for x in first..=last {
            if let Some(cell) = buffer.cell_mut(Position::new(x, y)) {
                cell.set_bg(SELECTION).set_fg(TEXT);
            }
        }
    }
}

// ─── Bars ────────────────────────────────────────────────────────────────────

fn draw_top_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    frame.render_widget(Block::new().style(Style::new().bg(MANTLE)), area);

    let mut x = area.x;
    let mut spans = vec![
        Span::styled(" ◆ ", Style::new().fg(MAUVE).bold()),
        Span::styled("sf-push-inspector", Style::new().fg(TEXT).bold()),
        Span::raw("   "),
    ];
    x += spans.iter().map(|s| s.width() as u16).sum::<u16>();

    for (tab, key, label) in [
        (Tab::Push, "1", "Push Upgrades"),
        (Tab::Subscribers, "2", "Subscribers"),
    ] {
        let text = format!(" {key} {label} ");
        let width = text.chars().count() as u16;
        let rect = Rect::new(x, area.y, width, 1);
        let hovered = app.hover.is_some_and(|p| rect.contains(p));
        let style = if app.tab == tab {
            Style::new().bg(MAUVE).fg(CRUST).bold()
        } else if hovered {
            Style::new().bg(SURFACE0).fg(TEXT)
        } else {
            Style::new().fg(SUBTEXT)
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::raw(" "));
        app.hits.buttons.push((rect, Action::ShowTab(tab)));
        x += width + 1;
    }
    frame.render_widget(Line::from(spans), area);

    let status = if app.loading {
        Line::from(vec![
            Span::styled(
                format!("{} ", SPINNER[app.tick / 2 % SPINNER.len()]),
                Style::new().fg(MAUVE),
            ),
            Span::styled("loading ", Style::new().fg(SUBTEXT)),
            Span::styled(format!("{} ", app.org), Style::new().fg(TEXT).bold()),
        ])
    } else {
        let (dot, color) = if app.load_error.is_some() {
            ("● ", RED)
        } else {
            ("● ", GREEN)
        };
        let mut spans = vec![
            Span::styled(dot, Style::new().fg(color)),
            Span::styled(format!("{} ", app.org), Style::new().fg(TEXT).bold()),
        ];
        if app.demo {
            spans.insert(
                0,
                Span::styled(" DEMO DATA ", Style::new().bg(PEACH).fg(CRUST).bold()),
            );
            spans.insert(1, Span::raw("  "));
        }
        if let Some(at) = app.loaded_at {
            spans.push(Span::styled(
                format!("updated {} ", at.format("%H:%M:%S")),
                Style::new().fg(OVERLAY0),
            ));
        }
        Line::from(spans)
    };
    frame.render_widget(status.alignment(Alignment::Right), area);
}

fn draw_bottom_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    frame.render_widget(Block::new().style(Style::new().bg(MANTLE)), area);

    let mut buttons = vec![("r", "Reload", Some(Action::Reload))];
    match app.tab {
        Tab::Push => {
            buttons.push(("c", "Copy details", Some(Action::Copy)));
            buttons.push(("tab", "Next pane", Some(Action::NextPane)));
            buttons.push(("[ ]", "Resize", None));
        }
        Tab::Subscribers => buttons.push(("/", "Filter", Some(Action::Filter))),
    }
    buttons.push(("q", "Quit", Some(Action::Quit)));

    let mut spans = vec![Span::raw(" ")];
    let mut x = area.x + 1;
    for (key, label, action) in buttons {
        let key_text = format!(" {key} ");
        let label_text = format!(" {label}  ");
        let width = (key_text.chars().count() + label_text.chars().count()) as u16;
        let rect = Rect::new(x, area.y, width, 1);
        let hovered = action.is_some() && app.hover.is_some_and(|p| rect.contains(p));
        spans.push(Span::styled(
            key_text,
            Style::new()
                .bg(if hovered { MAUVE } else { SURFACE0 })
                .fg(if hovered { CRUST } else { TEXT })
                .bold(),
        ));
        spans.push(Span::styled(
            label_text,
            Style::new().fg(if hovered { TEXT } else { OVERLAY0 }),
        ));
        if let Some(action) = action {
            app.hits.buttons.push((rect, action));
        }
        x += width;
    }
    frame.render_widget(Line::from(spans), area);

    if let Some(toast) = &app.toast {
        let color = if toast.is_error { RED } else { GREEN };
        let line = Line::from(vec![
            Span::styled(
                if toast.is_error { "✗ " } else { "✓ " },
                Style::new().fg(color).bold(),
            ),
            Span::styled(format!("{} ", toast.text), Style::new().fg(TEXT)),
        ]);
        frame.render_widget(line.alignment(Alignment::Right), area);
    }
}

// ─── Push upgrades tab ───────────────────────────────────────────────────────

fn draw_push_tab(frame: &mut Frame, app: &mut App, data: &Data, area: Rect) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(app.split), Constraint::Fill(1)]).areas(area);
    app.hits.body = area;
    app.hits.divider_x = right.x;

    draw_requests(frame, app, data, left);

    let [summary, jobs, details] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Fill(1),
        Constraint::Percentage(45),
    ])
    .areas(right);
    draw_summary(frame, app, data, summary);
    draw_jobs(frame, app, data, jobs);
    draw_details(frame, app, data, details);

    let divider_active = app.dragging || app.hover.is_some_and(|p| app.on_divider(p));
    if divider_active {
        let color = if app.dragging { MAUVE } else { LAVENDER };
        let buffer = frame.buffer_mut();
        for y in area.y + 1..area.bottom().saturating_sub(1) {
            buffer[Position::new(right.x, y)].set_symbol("┃").set_fg(color);
        }
    }
}

fn draw_requests(frame: &mut Frame, app: &mut App, data: &Data, area: Rect) {
    app.hits.requests = area;
    let hover = hovered_row(app, area, &app.requests);

    let rows = data.requests.iter().enumerate().map(|(i, request)| {
        let counts = data.counts(&request.id);
        Row::new([
            Cell::from(Span::styled("●", Style::new().fg(status_color(&request.status)))),
            Cell::from(Span::styled(
                data.version_label(&request.version_id),
                Style::new().fg(TEXT).bold(),
            )),
            Cell::from(Span::styled(
                request.status.clone(),
                Style::new().fg(status_color(&request.status)),
            )),
            Cell::from(Span::styled(
                fmt_date(request.start.or(request.scheduled)),
                Style::new().fg(SUBTEXT),
            )),
            Cell::from(counts_line(counts)),
            Cell::from(Span::styled(
                fmt_duration(request.start, request.end),
                Style::new().fg(OVERLAY0),
            )),
        ])
        .style(row_style(hover == Some(i)))
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Length(9),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Fill(1),
        ],
    )
    .header(header(["", "Version", "Status", "Started", "Result", "Took"]))
    .block(pane(
        format!("Push Requests · {}", data.requests.len()),
        app.pane == Pane::Requests,
    ))
    .row_highlight_style(selected_style(app.pane == Pane::Requests))
    .highlight_symbol(Span::styled("▌", Style::new().fg(MAUVE)))
    .highlight_spacing(HighlightSpacing::Always)
    .column_spacing(1);

    frame.render_stateful_widget(table, area, &mut app.requests);
    if data.requests.is_empty() {
        empty_message(frame, area, "No push requests yet");
    }
}

fn draw_summary(frame: &mut Frame, app: &App, data: &Data, area: Rect) {
    let Some(request) = app.request_in(data) else {
        frame.render_widget(pane("Summary".into(), false), area);
        return;
    };
    let version = data.versions.get(&request.version_id);
    let title = match version {
        Some(v) => format!("{} · {}", v.label(), v.name),
        None => "Unknown version".into(),
    };
    let block = pane(title, false);
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);

    let color = status_color(&request.status);
    let counts = data.counts(&request.id);
    let label = |text: &'static str| Span::styled(text, Style::new().fg(OVERLAY0));
    let value = |text: String| Span::styled(text, Style::new().fg(TEXT));

    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" {} ", request.status.to_uppercase()),
                Style::new().bg(color).fg(CRUST).bold(),
            ),
            Span::raw("  "),
            Span::styled(request.id.clone(), Style::new().fg(OVERLAY0)),
        ]),
        Line::from(vec![
            label("Scheduled "),
            value(fmt_date(request.scheduled)),
            label("   Started "),
            value(fmt_time(request.start)),
            label("   Ended "),
            value(fmt_time(request.end)),
            label("   Took "),
            value(fmt_duration(request.start, request.end)),
        ]),
        progress_bar(counts, inner.width),
        Line::from(vec![
            Span::styled("● ", Style::new().fg(GREEN)),
            value(format!("{} succeeded   ", counts.succeeded)),
            Span::styled("● ", Style::new().fg(RED)),
            value(format!("{} failed   ", counts.failed)),
            Span::styled("● ", Style::new().fg(BLUE)),
            value(format!("{} other", counts.other)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_jobs(frame: &mut Frame, app: &mut App, data: &Data, area: Rect) {
    app.hits.jobs = area;
    let jobs = app.jobs_in(data);
    let hover = hovered_row(app, area, &app.jobs);

    let rows: Vec<Row> = jobs
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let subscriber = data.subscriber(&job.org_key);
            let alias = data
                .alias(&job.org_key)
                .map(|o| o.alias.clone())
                .unwrap_or_default();
            Row::new([
                Cell::from(Span::styled("●", Style::new().fg(status_color(&job.status)))),
                Cell::from(Span::styled(data.org_name(&job.org_key), Style::new().fg(TEXT))),
                Cell::from(Span::styled(alias, Style::new().fg(LAVENDER))),
                Cell::from(Span::styled(
                    subscriber.map(|s| s.org_type.clone()).unwrap_or_default(),
                    Style::new().fg(SUBTEXT),
                )),
                Cell::from(Span::styled(
                    subscriber.map(|s| s.instance.clone()).unwrap_or_default(),
                    Style::new().fg(SUBTEXT),
                )),
                Cell::from(Span::styled(job.org_key.clone(), Style::new().fg(OVERLAY0))),
                Cell::from(Span::styled(
                    job.status.clone(),
                    Style::new().fg(status_color(&job.status)),
                )),
                Cell::from(Span::styled(
                    fmt_duration(job.start, job.end),
                    Style::new().fg(OVERLAY0),
                )),
            ])
            .style(row_style(hover == Some(i)))
        })
        .collect();
    let count = rows.len();

    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Fill(2),
            Constraint::Fill(1),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(15),
            Constraint::Length(10),
            Constraint::Length(7),
        ],
    )
    .header(header([
        "", "Org", "Alias", "Type", "Instance", "Org ID", "Status", "Took",
    ]))
    .block(pane(format!("Jobs · {count}"), app.pane == Pane::Jobs))
    .row_highlight_style(selected_style(app.pane == Pane::Jobs))
    .highlight_symbol(Span::styled("▌", Style::new().fg(MAUVE)))
    .highlight_spacing(HighlightSpacing::Always)
    .column_spacing(1);

    frame.render_stateful_widget(table, area, &mut app.jobs);
    if count == 0 {
        empty_message(frame, area, "No jobs for this request");
    }
}

fn draw_details(frame: &mut Frame, app: &mut App, data: &Data, area: Rect) {
    app.hits.details = area;
    let block = pane("Details".into(), app.pane == Pane::Details);
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);

    let Some(job) = app.job_in(data) else {
        empty_message(frame, area, "Select a job to see what happened");
        return;
    };

    let dim = Style::new().fg(OVERLAY0);
    let subscriber = data.subscriber(&job.org_key);
    let mut lines = vec![Line::from(vec![
        Span::styled(data.org_name(&job.org_key), Style::new().fg(TEXT).bold()),
        Span::styled(format!("  {}", job.org_key), dim),
    ])];

    let mut facts = vec![
        Span::styled("● ", Style::new().fg(status_color(&job.status))),
        Span::styled(job.status.clone(), Style::new().fg(status_color(&job.status))),
    ];
    if let Some(local) = data.alias(&job.org_key) {
        let name = if local.alias.is_empty() {
            local.username.clone()
        } else {
            local.alias.clone()
        };
        facts.push(Span::styled("   sf alias ", dim));
        facts.push(Span::styled(name, Style::new().fg(LAVENDER)));
    }
    if let Some(s) = subscriber {
        let installed = data.version_label(&s.version_id);
        let color = if data.is_latest(&s.version_id) {
            GREEN
        } else {
            YELLOW
        };
        facts.push(Span::styled("   installed ", dim));
        facts.push(Span::styled(installed, Style::new().fg(color)));
        facts.push(Span::styled(
            format!("   {} · {} · {}", s.org_type, s.org_status, s.instance),
            dim,
        ));
    }
    lines.push(Line::from(facts));
    lines.push(Line::default());

    let errors = data.errors_for(&job.id);
    if errors.is_empty() {
        lines.push(match job.status.as_str() {
            "Succeeded" => Line::styled("✓ Upgrade succeeded, no errors.", Style::new().fg(GREEN)),
            "Failed" => Line::styled("No error records returned for this job.", Style::new().fg(YELLOW)),
            _ => Line::styled("No errors so far.", Style::new().fg(SUBTEXT)),
        });
    }
    for error in errors {
        lines.push(Line::from(vec![
            Span::styled("✗ ", Style::new().fg(RED).bold()),
            Span::styled(error.title.clone(), Style::new().fg(RED).bold()),
            Span::styled(format!("   {} · {}", error.kind, error.severity), dim),
        ]));
        lines.push(Line::styled(error.message.clone(), Style::new().fg(TEXT)));
        if !error.details.is_empty() {
            lines.push(Line::styled(error.details.clone(), Style::new().fg(SUBTEXT)));
        }
        lines.push(Line::default());
        lines.push(Line::styled("▸ Next step", Style::new().fg(PEACH).bold()));
        lines.push(Line::styled(hint(error), Style::new().fg(SUBTEXT)));
        lines.push(Line::default());
    }

    let text = Text::from(lines);
    let content_height = wrapped_height(&text, inner.width);
    app.detail_max_scroll = content_height.saturating_sub(inner.height);
    app.detail_scroll = app.detail_scroll.min(app.detail_max_scroll);

    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((app.detail_scroll, 0)),
        inner,
    );

    if app.detail_max_scroll > 0 {
        let mut state =
            ScrollbarState::new(app.detail_max_scroll as usize).position(app.detail_scroll as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .track_style(Style::new().fg(SURFACE0))
                .thumb_style(Style::new().fg(MAUVE)),
            area.inner(Margin::new(0, 1)),
            &mut state,
        );
    }
}

// ─── Subscribers tab ─────────────────────────────────────────────────────────

fn draw_subscribers_tab(frame: &mut Frame, app: &mut App, data: &Data, area: Rect) {
    let [filter_area, table_area] =
        Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(area);
    app.hits.filter = filter_area;
    app.hits.subscribers = table_area;

    let filter_line = if app.filter.is_empty() && !app.editing_filter {
        Line::styled(
            "Click here or press / to filter by org, alias, org ID, instance or version",
            Style::new().fg(OVERLAY0),
        )
    } else {
        let cursor = if app.editing_filter && (app.tick / 6).is_multiple_of(2) {
            "▏"
        } else {
            " "
        };
        Line::from(vec![
            Span::styled("› ", Style::new().fg(MAUVE).bold()),
            Span::styled(app.filter.clone(), Style::new().fg(TEXT)),
            Span::styled(cursor, Style::new().fg(MAUVE)),
        ])
    };
    let filter_block = pane("Filter".into(), app.editing_filter);
    let filter_inner = filter_block.inner(filter_area).inner(Margin::new(1, 0));
    frame.render_widget(filter_block, filter_area);
    frame.render_widget(filter_line, filter_inner);

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
    let hover = hovered_row(app, table_area, &app.subscribers);

    let rows: Vec<Row> = subscribers
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let current = data.is_latest(&s.version_id);
            let color = if current { GREEN } else { YELLOW };
            let alias = data
                .alias(&s.org_key)
                .map(|o| o.alias.clone())
                .unwrap_or_default();
            Row::new([
                Cell::from(Span::styled("●", Style::new().fg(color))),
                Cell::from(Span::styled(s.name.clone(), Style::new().fg(TEXT))),
                Cell::from(Span::styled(alias, Style::new().fg(LAVENDER))),
                Cell::from(Span::styled(s.org_type.clone(), Style::new().fg(SUBTEXT))),
                Cell::from(Span::styled(s.org_status.clone(), Style::new().fg(SUBTEXT))),
                Cell::from(Span::styled(s.instance.clone(), Style::new().fg(SUBTEXT))),
                Cell::from(Span::styled(s.org_key.clone(), Style::new().fg(OVERLAY0))),
                Cell::from(Span::styled(
                    data.version_label(&s.version_id),
                    Style::new().fg(color).bold(),
                )),
                Cell::from(Span::styled(
                    if current { "up to date" } else { "behind" },
                    Style::new().fg(color),
                )),
            ])
            .style(row_style(hover == Some(i)))
        })
        .collect();
    let count = rows.len();

    let table = Table::new(
        rows,
        [
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
    )
    .header(header([
        "", "Org", "Alias", "Type", "Status", "Instance", "Org ID", "Version", "",
    ]))
    .block(pane(
        format!("Subscribers · {count} orgs · {behind} behind latest released {latest}"),
        !app.editing_filter,
    ))
    .row_highlight_style(selected_style(!app.editing_filter))
    .highlight_symbol(Span::styled("▌", Style::new().fg(MAUVE)))
    .highlight_spacing(HighlightSpacing::Always)
    .column_spacing(1);

    frame.render_stateful_widget(table, table_area, &mut app.subscribers);
    if count == 0 {
        empty_message(frame, table_area, "No subscribers match the filter");
    }
}

// ─── Loading and errors ──────────────────────────────────────────────────────

fn draw_loading(frame: &mut Frame, app: &App, area: Rect) {
    let rect = centered(area, 48, 5);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(MAUVE))
        .style(Style::new().bg(MANTLE));
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{}  ", SPINNER[app.tick / 2 % SPINNER.len()]),
                Style::new().fg(MAUVE).bold(),
            ),
            Span::styled("Querying push upgrades on ", Style::new().fg(TEXT)),
            Span::styled(app.org.clone(), Style::new().fg(TEXT).bold()),
        ]),
        Line::default(),
        Line::styled(
            "Runs sf data query, takes a few seconds",
            Style::new().fg(OVERLAY0),
        ),
    ];
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center).block(block),
        rect,
    );
}

fn draw_load_error(frame: &mut Frame, app: &App, area: Rect) {
    let rect = centered(area, area.width.saturating_mul(7) / 10, 9);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(RED))
        .title(Line::styled(" Could not load data ", Style::new().fg(RED).bold()))
        .style(Style::new().bg(MANTLE));
    let lines = vec![
        Line::styled(app.load_error.clone().unwrap_or_default(), Style::new().fg(TEXT)),
        Line::default(),
        Line::from(vec![
            Span::styled("Check that ", Style::new().fg(OVERLAY0)),
            Span::styled(app.org.clone(), Style::new().fg(TEXT).bold()),
            Span::styled(
                " is logged in (sf org list), then press ",
                Style::new().fg(OVERLAY0),
            ),
            Span::styled(" r ", Style::new().bg(SURFACE0).fg(TEXT).bold()),
        ]),
    ];
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(block),
        rect.inner(Margin::new(0, 0)),
    );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn pane(title: String, focused: bool) -> Block<'static> {
    let (border, title_style) = if focused {
        (Style::new().fg(MAUVE), Style::new().fg(MAUVE).bold())
    } else {
        (Style::new().fg(SURFACE1), Style::new().fg(SUBTEXT).bold())
    };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(Line::styled(format!(" {title} "), title_style))
}

fn header<const N: usize>(titles: [&'static str; N]) -> Row<'static> {
    Row::new(titles).style(Style::new().fg(OVERLAY0).add_modifier(Modifier::BOLD))
}

fn selected_style(focused: bool) -> Style {
    Style::new().bg(if focused { SURFACE1 } else { SURFACE0 })
}

fn row_style(hovered: bool) -> Style {
    if hovered {
        Style::new().bg(Color::Rgb(40, 40, 58))
    } else {
        Style::new()
    }
}

fn hovered_row(app: &App, area: Rect, state: &TableState) -> Option<usize> {
    let pos = app.hover?;
    let first = area.y + 2;
    (area.contains(pos) && pos.y >= first && pos.y + 1 < area.bottom())
        .then(|| state.offset() + (pos.y - first) as usize)
}

fn counts_line(counts: Counts) -> Line<'static> {
    if counts.total() == 0 {
        return Line::styled("—", Style::new().fg(OVERLAY0));
    }
    let mut spans = vec![Span::styled(
        format!("{}✓", counts.succeeded),
        Style::new().fg(GREEN),
    )];
    if counts.failed > 0 {
        spans.push(Span::styled(format!(" {}✗", counts.failed), Style::new().fg(RED)));
    }
    if counts.other > 0 {
        spans.push(Span::styled(format!(" {}…", counts.other), Style::new().fg(BLUE)));
    }
    Line::from(spans)
}

fn progress_bar(counts: Counts, width: u16) -> Line<'static> {
    let width = width as usize;
    let total = counts.total();
    if total == 0 || width == 0 {
        return Line::styled("━".repeat(width), Style::new().fg(SURFACE0));
    }
    let succeeded = (counts.succeeded * width).div_ceil(total).min(width);
    let failed = (counts.failed * width).div_ceil(total).min(width - succeeded);
    let other = width - succeeded - failed;
    Line::from(vec![
        Span::styled("━".repeat(succeeded), Style::new().fg(GREEN)),
        Span::styled("━".repeat(failed), Style::new().fg(RED)),
        Span::styled(
            "━".repeat(other),
            Style::new().fg(if counts.other > 0 { BLUE } else { SURFACE0 }),
        ),
    ])
}

fn empty_message(frame: &mut Frame, area: Rect, message: &'static str) {
    let rect = centered(area, area.width.saturating_sub(4), 1);
    frame.render_widget(
        Line::styled(message, Style::new().fg(OVERLAY0)).alignment(Alignment::Center),
        rect,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// Approximate height of wrapped text; word wrapping can need a little more than character wrapping.
fn wrapped_height(text: &Text, width: u16) -> u16 {
    let width = width.max(1) as usize;
    text.lines
        .iter()
        .map(|line| (line.width() * 11 / 10).div_ceil(width).max(1))
        .sum::<usize>()
        .min(u16::MAX as usize) as u16
}

fn fmt_date(time: Option<Timestamp>) -> String {
    time.map(|t| t.with_timezone(&Local).format("%d.%m. %H:%M").to_string())
        .unwrap_or_else(|| "—".into())
}

fn fmt_time(time: Option<Timestamp>) -> String {
    time.map(|t| t.with_timezone(&Local).format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "—".into())
}

pub fn fmt_duration(start: Option<Timestamp>, end: Option<Timestamp>) -> String {
    let Some(start) = start else {
        return "—".into();
    };
    let end = end.map(|e| e.with_timezone(&Utc)).unwrap_or_else(Utc::now);
    let seconds = (end - start.with_timezone(&Utc)).num_seconds().max(0);
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, s % 3600 / 60),
    }
}
