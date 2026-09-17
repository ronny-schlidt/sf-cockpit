use super::table::{self, TableSpec, hover_row};
use super::{colored, dim, pane, row_style};
use crate::app::settings::{ROWS, SettingKey};
use crate::app::{App, Target};
use crate::config::Origin;
use crate::theme::*;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Wrap};

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let [table_area, help_area] =
        Layout::vertical([Constraint::Length(ROWS.len() as u16 + 3), Constraint::Fill(1)]).areas(area);
    let hover = hover_row(app.hover, table_area, &app.setting_rows);

    let rows = ROWS
        .iter()
        .enumerate()
        .map(|(i, key)| {
            let editable = key.file_key().is_some();
            let missing = *key == SettingKey::DevHub && app.cfg.needs_setup();
            let value_color = if missing {
                RED
            } else if editable {
                LAVENDER
            } else {
                SUBTEXT
            };
            let from_flag = key
                .file_key()
                .is_some_and(|file_key| app.cfg.origin(file_key) == Origin::Flag);
            Row::new([
                Cell::from(Span::styled(
                    key.label(),
                    Style::new().fg(if editable { TEXT } else { SUBTEXT }).bold(),
                )),
                Cell::from(colored(app.setting_value(*key), value_color)),
                Cell::from(colored(
                    app.setting_source(*key),
                    if from_flag { YELLOW } else { OVERLAY0 },
                )),
            ])
            .style(row_style(hover == Some(i)))
        })
        .collect();

    table::draw(
        frame,
        &mut app.hits,
        Target::Settings,
        table_area,
        TableSpec {
            title: "Settings".into(),
            focused: app.focus == Target::Settings,
            header: &["Setting", "Value", "Comes from"],
            widths: &[Constraint::Length(22), Constraint::Fill(3), Constraint::Fill(2)],
            empty: "",
        },
        rows,
        &mut app.setting_rows,
    );

    let selected = app.setting_rows.selected().and_then(|i| ROWS.get(i)).copied();
    let mut lines = vec![Line::from(vec![
        colored("Enter", MAUVE),
        dim(" changes the selected setting. "),
        dim(match selected {
            Some(SettingKey::DevHub) => "You pick the Dev Hub from the orgs the sf CLI knows.",
            Some(SettingKey::Package) => "You pick one of the packages the Dev Hub owns.",
            Some(SettingKey::ScratchOrg) => {
                "Deploys, tests and installs go to this org by default. Installing there asks without a warning."
            }
            Some(SettingKey::Limit) => "More requests make the Push tab load a little slower.",
            Some(SettingKey::Cache) => "Enter or C clears the cache.",
            Some(SettingKey::Version) => {
                "sf-cockpit looks for a new release once a day and shows it in the top bar. Enter or N opens it."
            }
            _ => "",
        }),
    ])];
    lines.push(Line::default());
    lines.push(Line::from(match &app.cfg.save_path {
        Some(_) => dim(
            "Changes are written into the file above right away, and only the changed line is touched. \
             A project file wins over the global one, and command-line flags win over both.",
        ),
        None => dim("Demo mode: changes only last until you quit."),
    }));
    lines.push(Line::default());
    lines.push(Line::from(dim(
        "At start every tab shows the data of the last run at once and refreshes itself in the background. \
         A spinner next to a tab name means that tab is being refreshed.",
    )));
    if ROWS
        .iter()
        .filter_map(|key| key.file_key())
        .any(|key| app.cfg.origin(key) == Origin::Flag)
    {
        lines.push(Line::default());
        lines.push(Line::from(colored(
            "Values marked as command-line flag cannot be changed here until you start without that flag.",
            YELLOW,
        )));
    }

    let problems = app.setup_problems();
    let title = if problems.is_empty() { "Help" } else { "Setup" };
    let mut top: Vec<Line> = problems
        .into_iter()
        .map(|(blocking, text)| {
            let color = if blocking { RED } else { YELLOW };
            Line::from(vec![
                colored(if blocking { "✗ " } else { "! " }, color),
                colored(text, color),
            ])
        })
        .collect();
    if !top.is_empty() {
        top.push(Line::default());
        lines.splice(0..0, top);
    }

    let block = pane(title.into(), false);
    let inner = block.inner(help_area).inner(Margin::new(1, 0));
    frame.render_widget(block, help_area);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}
