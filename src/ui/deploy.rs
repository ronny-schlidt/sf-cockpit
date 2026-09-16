use super::table::{self, TableSpec, hover_row};
use super::{
    colored, dim, draw_scrollable, empty_message, fmt_date, fmt_duration, label, pane, ready, row_style,
    value,
};
use crate::app::{App, Target};
use crate::sf::deploy::{DeployRecord, TestRun};
use crate::theme::*;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Cell, Row};

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.deploy_org.is_empty() && app.deploys.value.is_none() {
        app.hits.register(Target::Deploys, area);
        frame.render_widget(pane("Deployments".into(), true), area);
        empty_message(
            frame,
            area,
            "No org chosen. Press o to pick one, or set scratch_org in sf-cockpit.toml",
        );
        return;
    }
    if !ready(frame, app, area, &app.deploys, "Querying deployments") {
        return;
    }
    let deploys = app.deploys.value.take().expect("checked by ready");
    draw_tab(frame, app, &deploys, area);
    app.deploys.value = Some(deploys);
}

fn draw_tab(frame: &mut Frame, app: &mut App, deploys: &[DeployRecord], area: Rect) {
    let [table_area, details_area] =
        Layout::vertical([Constraint::Percentage(45), Constraint::Fill(1)]).areas(area);
    let hover = hover_row(app.hover, table_area, &app.deploy_rows);

    let rows = deploys
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let color = status_color(&d.status);
            Row::new([
                Cell::from(colored("●", color)),
                Cell::from(dim(fmt_date(d.start.or(d.created)))),
                Cell::from(colored(d.status.clone(), color)),
                Cell::from(progress(
                    d.components_deployed,
                    d.components_total,
                    d.component_errors,
                )),
                Cell::from(progress(d.tests_completed, d.tests_total, d.test_errors)),
                Cell::from(dim(if d.check_only { "validate" } else { "deploy" })),
                Cell::from(dim(d.created_by.clone())),
                Cell::from(colored(fmt_duration(d.start, d.end), OVERLAY0)),
            ])
            .style(row_style(hover == Some(i)))
        })
        .collect::<Vec<_>>();

    table::draw(
        frame,
        &mut app.hits,
        Target::Deploys,
        table_area,
        TableSpec {
            title: format!("Deployments on {} · {}", app.deploy_org, deploys.len()),
            focused: app.focus == Target::Deploys,
            header: &[
                "",
                "Started",
                "Status",
                "Components",
                "Tests",
                "Kind",
                "By",
                "Took",
            ],
            widths: &[
                Constraint::Length(1),
                Constraint::Length(12),
                Constraint::Length(17),
                Constraint::Length(16),
                Constraint::Length(14),
                Constraint::Length(8),
                Constraint::Fill(1),
                Constraint::Length(8),
            ],
            empty: "No deployments yet, press D to deploy",
        },
        rows,
        &mut app.deploy_rows,
    );

    let mut lines = Vec::new();
    if let Some(run) = &app.test_run {
        test_lines(run, &mut lines);
    }
    if let Some(d) = app.deploy_in(deploys) {
        deploy_lines(d, &mut lines);
    }
    if lines.is_empty() {
        lines.push(Line::styled(
            "Select a deployment, or press t to run Apex tests",
            Style::new().fg(OVERLAY0),
        ));
    }
    let title = if app.test_run.is_some() {
        "Tests and details"
    } else {
        "Details"
    };
    draw_scrollable(
        frame,
        app,
        Target::DeployDetails,
        details_area,
        title,
        Text::from(lines),
    );
}

fn progress(done: i64, total: i64, errors: i64) -> Line<'static> {
    if total == 0 && errors == 0 {
        return Line::styled("—", Style::new().fg(OVERLAY0));
    }
    let mut spans = vec![value(format!("{done}/{total}"))];
    if errors > 0 {
        spans.push(colored(format!(" {errors}✗"), RED));
    }
    Line::from(spans)
}

fn deploy_lines(d: &DeployRecord, lines: &mut Vec<Line<'static>>) {
    let color = status_color(&d.status);
    lines.push(Line::from(vec![
        Span::styled(
            format!(" {} ", d.status.to_uppercase()),
            Style::new().bg(color).fg(CRUST).bold(),
        ),
        colored(format!("  {}", d.id), OVERLAY0),
        colored(
            format!(
                "   {}",
                if d.check_only {
                    "validation only"
                } else {
                    "deployment"
                }
            ),
            SUBTEXT,
        ),
    ]));
    let mut facts = vec![
        label("Started "),
        value(fmt_date(d.start.or(d.created))),
        label("   Took "),
        value(fmt_duration(d.start, d.end)),
        label("   By "),
        value(d.created_by.clone()),
    ];
    if !d.test_level.is_empty() {
        facts.push(label("   Test level "));
        facts.push(value(d.test_level.clone()));
    }
    lines.push(Line::from(facts));
    lines.push(Line::from(vec![
        label("Components "),
        value(format!(
            "{} of {} deployed, {} errors",
            d.components_deployed, d.components_total, d.component_errors
        )),
        label("   Tests "),
        value(format!(
            "{} of {} run, {} errors",
            d.tests_completed, d.tests_total, d.test_errors
        )),
    ]));
    if !d.error_message.is_empty() {
        lines.push(Line::styled(d.error_message.clone(), Style::new().fg(RED)));
    }
}

fn test_lines(run: &TestRun, lines: &mut Vec<Line<'static>>) {
    let color = if run.failing > 0 { RED } else { GREEN };
    lines.push(Line::from(vec![
        Span::styled(
            format!(" TESTS {} ", run.outcome.to_uppercase()),
            Style::new().bg(color).fg(CRUST).bold(),
        ),
        colored(format!("  on {}", run.org), SUBTEXT),
    ]));
    lines.push(Line::from(vec![
        colored(format!("{} passed", run.passing), GREEN),
        label(" · "),
        colored(
            format!("{} failed", run.failing),
            if run.failing > 0 { RED } else { SUBTEXT },
        ),
        label(" · "),
        dim(format!("{} skipped", run.skipped)),
        label(" · "),
        dim(format!("{} ran, pass rate {}", run.ran, run.pass_rate)),
        label("   Coverage "),
        value(run.run_coverage.clone()),
        label(" (org-wide "),
        value(run.org_coverage.clone()),
        label(")   "),
        dim(format!("{:.1}s", run.time_ms as f64 / 1000.0)),
    ]));
    for failure in &run.failures {
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("✗ ", Style::new().fg(RED).bold()),
            Span::styled(
                format!("{}.{}", failure.class, failure.method),
                Style::new().fg(RED).bold(),
            ),
        ]));
        lines.push(Line::styled(failure.message.clone(), Style::new().fg(TEXT)));
        if !failure.stack.is_empty() {
            lines.push(Line::styled(failure.stack.clone(), Style::new().fg(SUBTEXT)));
        }
    }
    if !run.coverage.is_empty() {
        lines.push(Line::default());
        lines.push(Line::styled(
            "Coverage per class, lowest first",
            Style::new().fg(PEACH).bold(),
        ));
        for class in &run.coverage {
            let color = if class.percent < 75.0 { RED } else { GREEN };
            lines.push(Line::from(vec![
                colored(format!("{:>5.1}%  ", class.percent), color),
                value(class.name.clone()),
                dim(format!("  {}/{} lines", class.covered, class.total_lines)),
            ]));
        }
    }
    lines.push(Line::default());
}
