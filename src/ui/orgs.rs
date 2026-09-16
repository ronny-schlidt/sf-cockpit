use super::table::{self, TableSpec, hover_row};
use super::{colored, dim, draw_filter, draw_scrollable, label, ready, row_style, value};
use crate::app::{App, Target};
use crate::sf::orgs::{Health, OrgInfo, OrgKind};
use crate::theme::*;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Cell, Row};

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    if !ready(frame, app, area, &app.orgs, "Listing orgs with sf org list") {
        return;
    }
    let orgs = app.orgs.value.take().expect("checked by ready");
    draw_tab(frame, app, &orgs, area);
    app.orgs.value = Some(orgs);
}

fn draw_tab(frame: &mut Frame, app: &mut App, orgs: &[OrgInfo], area: Rect) {
    let [filter_area, table_area, details_area] =
        Layout::vertical([Constraint::Length(3), Constraint::Fill(1), Constraint::Length(7)]).areas(area);
    draw_filter(
        frame,
        app,
        filter_area,
        "Click here or press / to filter by alias, username, org ID, type or instance",
    );

    let filtered = app.orgs_in(orgs);
    let hover = hover_row(app.hover, table_area, &app.org_rows);
    let rows = filtered
        .iter()
        .enumerate()
        .map(|(i, org)| {
            let alias_color = if org.has_duplicate_aliases() {
                YELLOW
            } else {
                LAVENDER
            };
            let mut alias = Span::styled(org.aliases.join(", "), Style::new().fg(alias_color));
            if org.is_default || org.is_default_hub {
                alias = alias.bold();
            }
            Row::new([
                Cell::from(colored("●", health_color(org.health()))),
                Cell::from(alias),
                Cell::from(dim(org.kind.label())),
                Cell::from(value(org.username.clone())),
                Cell::from(colored(org.org_id.clone(), OVERLAY0)),
                Cell::from(dim(org.host())),
                Cell::from(colored(org.status.clone(), health_color(org.health()))),
                Cell::from(dim(org
                    .expires
                    .map(|d| d.format("%d.%m.%Y").to_string())
                    .unwrap_or_default())),
                Cell::from(installed_span(app, org)),
            ])
            .style(row_style(hover == Some(i)))
        })
        .collect::<Vec<_>>();
    let count = rows.len();
    let scratch = filtered.iter().filter(|o| o.kind == OrgKind::Scratch).count();

    table::draw(
        frame,
        &mut app.hits,
        Target::Orgs,
        table_area,
        TableSpec {
            title: format!("Orgs · {count} · {scratch} scratch"),
            focused: app.focus == Target::Orgs && !app.editing_filter,
            header: &[
                "",
                "Alias",
                "Type",
                "Username",
                "Org ID",
                "Instance",
                "Status",
                "Expires",
                "Installed",
            ],
            widths: &[
                Constraint::Length(1),
                Constraint::Fill(1),
                Constraint::Length(10),
                Constraint::Fill(2),
                Constraint::Length(18),
                Constraint::Fill(2),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(13),
            ],
            empty: "No orgs match the filter",
        },
        rows,
        &mut app.org_rows,
    );

    draw_details(frame, app, orgs, details_area);
}

fn installed_span(app: &App, org: &OrgInfo) -> Span<'static> {
    match app.installed_label(&org.username) {
        Some(text) if text == "not installed" => colored(text, OVERLAY0),
        Some(text) if text.starts_with("error") => colored("error", RED),
        Some(text) => colored(text, GREEN),
        None => colored("press i", SURFACE1),
    }
}

fn draw_details(frame: &mut Frame, app: &mut App, orgs: &[OrgInfo], area: Rect) {
    let Some(org) = app.org_in(orgs) else {
        app.hits.register(Target::OrgDetails, area);
        frame.render_widget(super::pane("Org".into(), app.focus == Target::OrgDetails), area);
        return;
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(org.alias().to_string(), Style::new().fg(TEXT).bold()),
        colored(format!("  {}", org.kind.label()), OVERLAY0),
        colored(
            if org.is_default {
                "  default org"
            } else if org.is_default_hub {
                "  default dev hub"
            } else {
                ""
            },
            BLUE,
        ),
    ])];
    if org.has_duplicate_aliases() {
        lines.push(Line::from(vec![
            colored("⚠ ", YELLOW),
            colored(
                format!(
                    "{} aliases point at this org: {}. Remove the extra ones with sf alias unset.",
                    org.aliases.len(),
                    org.aliases.join(", ")
                ),
                YELLOW,
            ),
        ]));
    }
    lines.push(Line::from(vec![
        label("Username "),
        value(org.username.clone()),
        label("   Org ID "),
        value(org.org_id.clone()),
        label("   Instance "),
        value(org.instance_url.clone()),
    ]));
    let mut facts = vec![
        label("Status "),
        colored(org.status.clone(), health_color(org.health())),
    ];
    if let Some(expires) = org.expires {
        facts.push(label("   Expires "));
        facts.push(value(expires.format("%d.%m.%Y").to_string()));
    }
    if !org.dev_hub.is_empty() {
        facts.push(label("   Dev Hub "));
        facts.push(value(org.dev_hub.clone()));
    }
    if !org.org_name.is_empty() {
        facts.push(label("   Org name "));
        facts.push(value(org.org_name.clone()));
    }
    lines.push(Line::from(facts));
    let installed = match app.installed_label(&org.username) {
        Some(text) => text,
        None => "unknown, press i to check".into(),
    };
    lines.push(Line::from(vec![label("Installed package "), value(installed)]));
    draw_scrollable(frame, app, Target::OrgDetails, area, "Org", Text::from(lines));
}

fn health_color(health: Health) -> Color {
    match health {
        Health::Ok => GREEN,
        Health::Unknown => OVERLAY0,
        Health::Down => RED,
        Health::Expired => YELLOW,
    }
}
