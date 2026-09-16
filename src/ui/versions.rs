use super::table::{self, TableSpec, hover_row};
use super::{colored, dim, draw_scrollable, label, ready, row_style, value};
use crate::app::{App, Target};
use crate::sf::versions::{PackageVersion, install_url};
use crate::theme::*;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Cell, Row};

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    if super::setup_hint(frame, app, area) {
        return;
    }
    if !ready(frame, app, area, &app.versions, "Listing package versions") {
        return;
    }
    let versions = app.versions.value.take().expect("checked by ready");
    draw_tab(frame, app, &versions, area);
    app.versions.value = Some(versions);
}

fn draw_tab(frame: &mut Frame, app: &mut App, versions: &[PackageVersion], area: Rect) {
    let [table_area, details_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(8)]).areas(area);
    let push = app.push.value.as_ref();
    let hover = hover_row(app.hover, table_area, &app.version_rows);

    let rows = versions
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let color = if v.released { GREEN } else { BLUE };
            let subscribers = push.map(|d| d.subscribers_on(&v.id)).unwrap_or_default();
            let coverage = match v.coverage {
                Some(c) => colored(
                    format!("{c:.0}%"),
                    if v.coverage_ok == Some(false) || c < 75.0 {
                        RED
                    } else {
                        SUBTEXT
                    },
                ),
                None => colored("—", OVERLAY0),
            };
            Row::new([
                Cell::from(colored("●", color)),
                Cell::from(Span::styled(v.version.clone(), Style::new().fg(TEXT).bold())),
                Cell::from(value(v.name.clone())),
                Cell::from(colored(v.state(), color)),
                Cell::from(dim(v.created.clone())),
                Cell::from(coverage),
                Cell::from(if subscribers > 0 {
                    colored(subscribers.to_string(), LAVENDER)
                } else {
                    colored("—", OVERLAY0)
                }),
                Cell::from(colored(v.id.clone(), OVERLAY0)),
                Cell::from(dim(v.ancestor_version.clone())),
            ])
            .style(row_style(hover == Some(i)))
        })
        .collect::<Vec<_>>();
    let released = versions.iter().filter(|v| v.released).count();

    table::draw(
        frame,
        &mut app.hits,
        Target::Versions,
        table_area,
        TableSpec {
            title: format!("Package Versions · {} · {released} released", versions.len()),
            focused: app.focus == Target::Versions,
            header: &[
                "",
                "Version",
                "Name",
                "State",
                "Created",
                "Coverage",
                "Subscribers",
                "04t",
                "Ancestor",
            ],
            widths: &[
                Constraint::Length(1),
                Constraint::Length(9),
                Constraint::Fill(1),
                Constraint::Length(8),
                Constraint::Length(16),
                Constraint::Length(8),
                Constraint::Length(11),
                Constraint::Length(18),
                Constraint::Length(9),
            ],
            empty: "No package versions yet, press n to create one",
        },
        rows,
        &mut app.version_rows,
    );

    draw_details(frame, app, versions, details_area);
}

fn draw_details(frame: &mut Frame, app: &mut App, versions: &[PackageVersion], area: Rect) {
    let Some(v) = app.version_in(versions) else {
        app.hits.register(Target::VersionDetails, area);
        frame.render_widget(
            super::pane("Version".into(), app.focus == Target::VersionDetails),
            area,
        );
        return;
    };
    let color = if v.released { GREEN } else { BLUE };
    let mut lines = vec![Line::from(vec![
        Span::styled(v.label(), Style::new().fg(TEXT).bold()),
        colored(format!("   {}", v.state()), color),
        colored(format!("   created {}", v.created), OVERLAY0),
    ])];
    let mut facts = vec![label("04t "), value(v.id.clone())];
    if !v.ancestor_version.is_empty() {
        facts.push(label("   ancestor "));
        facts.push(value(format!("{} ({})", v.ancestor_version, v.ancestor_id)));
    }
    if let Some(coverage) = v.coverage {
        facts.push(label("   coverage "));
        facts.push(value(format!("{coverage:.1}%")));
    }
    if let Some(seconds) = v.build_seconds {
        facts.push(label("   build "));
        facts.push(value(format!("{}m {}s", seconds / 60, seconds % 60)));
    }
    if !v.branch.is_empty() {
        facts.push(label("   branch "));
        facts.push(value(v.branch.clone()));
    }
    if !v.tag.is_empty() {
        facts.push(label("   tag "));
        facts.push(value(v.tag.clone()));
    }
    lines.push(Line::from(facts));
    lines.push(Line::from(vec![
        label("Install (production) "),
        value(install_url(&v.id, false)),
    ]));
    lines.push(Line::from(vec![
        label("Install (sandbox)    "),
        value(install_url(&v.id, true)),
    ]));
    if let Some(data) = app.push.value.as_ref() {
        let names: Vec<String> = data
            .subscribers
            .iter()
            .filter(|s| s.version_id == v.id)
            .map(|s| s.name.clone())
            .collect();
        let text = if names.is_empty() {
            "no subscriber org".to_string()
        } else {
            format!("{} · {}", names.len(), names.join(", "))
        };
        lines.push(Line::from(vec![label("Installed in "), value(text)]));
    }
    draw_scrollable(
        frame,
        app,
        Target::VersionDetails,
        area,
        "Version",
        Text::from(lines),
    );
}
