use super::table::{self, TableSpec, hover_row};
use super::{
    colored, dim, draw_scrollable, fmt_date, fmt_duration, fmt_time, label, pane, ready, row_style, value,
};
use crate::app::{App, Target, hint};
use crate::sf::push::{Counts, PushData};
use crate::theme::*;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Cell, Paragraph, Row};

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    if super::setup_hint(frame, app, area) {
        return;
    }
    if !ready(frame, app, area, &app.push, "Querying push upgrades") {
        return;
    }
    // Take the data out so panes can borrow it while mutating table state.
    let data = app.push.value.take().expect("checked by ready");
    draw_tab(frame, app, &data, area);
    app.push.value = Some(data);
}

fn draw_tab(frame: &mut Frame, app: &mut App, data: &PushData, area: Rect) {
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

fn draw_requests(frame: &mut Frame, app: &mut App, data: &PushData, area: Rect) {
    let hover = hover_row(app.hover, area, &app.requests);
    let rows = data
        .requests
        .iter()
        .enumerate()
        .map(|(i, request)| {
            let counts = data.counts(&request.id);
            Row::new([
                Cell::from(colored("●", status_color(&request.status))),
                Cell::from(Span::styled(
                    data.version_label(&request.version_id),
                    Style::new().fg(TEXT).bold(),
                )),
                Cell::from(colored(request.status.clone(), status_color(&request.status))),
                Cell::from(dim(fmt_date(request.start.or(request.scheduled)))),
                Cell::from(counts_line(counts)),
                Cell::from(colored(fmt_duration(request.start, request.end), OVERLAY0)),
            ])
            .style(row_style(hover == Some(i)))
        })
        .collect();

    table::draw(
        frame,
        &mut app.hits,
        Target::Requests,
        area,
        TableSpec {
            title: format!("Push Requests · {}", data.requests.len()),
            focused: app.focus == Target::Requests,
            header: &["", "Version", "Status", "Started", "Result", "Took"],
            widths: &[
                Constraint::Length(1),
                Constraint::Length(9),
                Constraint::Length(10),
                Constraint::Length(12),
                Constraint::Length(8),
                Constraint::Fill(1),
            ],
            empty: "No push requests yet, press s to schedule one",
        },
        rows,
        &mut app.requests,
    );
}

fn draw_summary(frame: &mut Frame, app: &App, data: &PushData, area: Rect) {
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
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" {} ", request.status.to_uppercase()),
                Style::new().bg(color).fg(CRUST).bold(),
            ),
            Span::raw("  "),
            colored(request.id.clone(), OVERLAY0),
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
            colored("● ", GREEN),
            value(format!("{} succeeded   ", counts.succeeded)),
            colored("● ", RED),
            value(format!("{} failed   ", counts.failed)),
            colored("● ", BLUE),
            value(format!("{} other", counts.other)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The org's own name from the config, with a star for marked orgs.
fn org_title(app: &App, data: &PushData, org_key: &str) -> String {
    let name = app.org_label(data, org_key);
    if app.cfg.is_important(org_key) {
        format!("★ {name}")
    } else {
        name
    }
}

fn draw_jobs(frame: &mut Frame, app: &mut App, data: &PushData, area: Rect) {
    let jobs = app.jobs_in(data);
    let hover = hover_row(app.hover, area, &app.jobs);
    let rows = jobs
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let subscriber = data.subscriber(&job.org_key);
            let alias = data
                .alias(&job.org_key)
                .map(|o| o.alias.clone())
                .unwrap_or_default();
            Row::new([
                Cell::from(colored("●", status_color(&job.status))),
                Cell::from(value(org_title(app, data, &job.org_key))),
                Cell::from(colored(alias, LAVENDER)),
                Cell::from(dim(subscriber.map(|s| s.org_type.clone()).unwrap_or_default())),
                Cell::from(dim(subscriber.map(|s| s.instance.clone()).unwrap_or_default())),
                Cell::from(colored(job.org_key.clone(), OVERLAY0)),
                Cell::from(colored(job.status.clone(), status_color(&job.status))),
                Cell::from(colored(fmt_duration(job.start, job.end), OVERLAY0)),
            ])
            .style(row_style(hover == Some(i)))
        })
        .collect::<Vec<_>>();
    let count = rows.len();

    table::draw(
        frame,
        &mut app.hits,
        Target::Jobs,
        area,
        TableSpec {
            title: format!("Jobs · {count}"),
            focused: app.focus == Target::Jobs,
            header: &["", "Org", "Alias", "Type", "Instance", "Org ID", "Status", "Took"],
            widths: &[
                Constraint::Length(1),
                Constraint::Fill(2),
                Constraint::Fill(1),
                Constraint::Length(10),
                Constraint::Length(8),
                Constraint::Length(15),
                Constraint::Length(10),
                Constraint::Length(7),
            ],
            empty: "No jobs for this request",
        },
        rows,
        &mut app.jobs,
    );
}

fn draw_details(frame: &mut Frame, app: &mut App, data: &PushData, area: Rect) {
    let Some(job) = app.job_in(data) else {
        app.hits.register(Target::Details, area);
        frame.render_widget(pane("Details".into(), app.focus == Target::Details), area);
        super::empty_message(frame, area, "Select a job to see what happened");
        return;
    };

    let subscriber = data.subscriber(&job.org_key);
    let mut lines = vec![Line::from(vec![
        Span::styled(org_title(app, data, &job.org_key), Style::new().fg(TEXT).bold()),
        colored(format!("  {}", job.org_key), OVERLAY0),
    ])];

    let mut facts = vec![
        colored("● ", status_color(&job.status)),
        colored(job.status.clone(), status_color(&job.status)),
    ];
    if let Some(local) = data.alias(&job.org_key) {
        let name = if local.alias.is_empty() {
            local.username.clone()
        } else {
            local.alias.clone()
        };
        facts.push(colored("   sf alias ", OVERLAY0));
        facts.push(colored(name, LAVENDER));
    }
    if let Some(s) = subscriber {
        let installed = data.version_label(&s.version_id);
        let color = if data.is_latest(&s.version_id) {
            GREEN
        } else {
            YELLOW
        };
        facts.push(colored("   installed ", OVERLAY0));
        facts.push(colored(installed, color));
        facts.push(colored(
            format!("   {} · {} · {}", s.org_type, s.org_status, s.instance),
            OVERLAY0,
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
            colored(format!("   {} · {}", error.kind, error.severity), OVERLAY0),
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
    draw_scrollable(frame, app, Target::Details, area, "Details", Text::from(lines));
}

fn counts_line(counts: Counts) -> Line<'static> {
    if counts.total() == 0 {
        return Line::styled("—", Style::new().fg(OVERLAY0));
    }
    let mut spans = vec![colored(format!("{}✓", counts.succeeded), GREEN)];
    if counts.failed > 0 {
        spans.push(colored(format!(" {}✗", counts.failed), RED));
    }
    if counts.other > 0 {
        spans.push(colored(format!(" {}…", counts.other), BLUE));
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
        colored("━".repeat(succeeded), GREEN),
        colored("━".repeat(failed), RED),
        colored("━".repeat(other), if counts.other > 0 { BLUE } else { SURFACE0 }),
    ])
}
