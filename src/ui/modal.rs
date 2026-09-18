//! Dialogs drawn above the tabs. While a dialog is open, only its own buttons and rows are clickable.

use super::{SPINNER, centered, colored, dim, fmt_elapsed, label, value, wrapped_height};
use crate::app::modal::{Confirm, Input, Modal, Picker, Step, Wizard};
use crate::app::{Action, App};
use crate::sf::runner::TaskId;
use crate::theme::*;
use crate::update::{self, ReleaseInfo};
use chrono::{Local, NaiveDateTime};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let Some(modal) = app.modal.take() else {
        return;
    };
    app.hits.buttons.clear();
    app.hits.regions.clear();
    let area = frame.area();
    match &modal {
        Modal::Confirm(confirm) => draw_confirm(frame, app, area, confirm),
        Modal::Input(input) => draw_input(frame, app, area, input),
        Modal::Picker(picker) => draw_picker(frame, app, area, picker),
        Modal::Wizard(wizard) => draw_wizard(frame, app, area, wizard),
        Modal::TaskLog { id, scroll } => draw_log(frame, app, area, *id, *scroll),
        Modal::Update { release, installed } => draw_update(frame, app, area, release, *installed),
        Modal::Message { title, body, error } => draw_message(frame, app, area, title, body, *error),
    }
    app.modal = Some(modal);
}

/// Clears a centered box, draws its border and returns `(content, button row)`.
fn dialog(frame: &mut Frame, area: Rect, width: u16, height: u16, title: &str, color: Color) -> (Rect, Rect) {
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(color))
        .title(Line::styled(format!(" {title} "), Style::new().fg(color).bold()))
        .style(Style::new().bg(MANTLE).fg(TEXT));
    let inner = block.inner(rect).inner(Margin::new(1, 0));
    frame.render_widget(block, rect);
    let [content, _, buttons] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
    (content, buttons)
}

fn buttons(frame: &mut Frame, app: &mut App, area: Rect, items: &[(&str, &str, Option<Action>)]) {
    let mut spans = Vec::new();
    let mut x = area.x;
    for (key, text, action) in items {
        let key_text = format!(" {key} ");
        let label_text = format!(" {text}  ");
        let width = (key_text.chars().count() + label_text.chars().count()) as u16;
        let rect = Rect::new(x, area.y, width.min(area.right().saturating_sub(x)), 1);
        let hovered = action.is_some() && app.hover.is_some_and(|p| rect.contains(p));
        spans.push(Span::styled(
            key_text,
            Style::new()
                .bg(if hovered { MAUVE } else { SURFACE1 })
                .fg(if hovered { CRUST } else { TEXT })
                .bold(),
        ));
        spans.push(Span::styled(
            label_text,
            Style::new().fg(if hovered { TEXT } else { SUBTEXT }),
        ));
        if let Some(action) = action {
            app.hits.buttons.push((rect, *action));
        }
        x += width;
    }
    frame.render_widget(Line::from(spans), area);
}

fn width_for(area: Rect, preferred: u16) -> u16 {
    preferred.min(area.width.saturating_sub(4))
}

fn command_lines(argv: &[String], lines: &mut Vec<Line<'static>>) {
    lines.push(Line::default());
    lines.push(Line::styled("Command", Style::new().fg(OVERLAY0).bold()));
    lines.push(Line::styled(
        format!("sf {}", shell_join(argv)),
        Style::new().fg(SUBTEXT),
    ));
}

/// Quotes arguments with spaces or commas so the shown command can be pasted into a shell.
pub fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            if arg.is_empty() || arg.contains(|c: char| c.is_whitespace() || "'\"$`\\;&|<>()".contains(c)) {
                format!("'{}'", arg.replace('\'', r"'\''"))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn draw_confirm(frame: &mut Frame, app: &mut App, area: Rect, confirm: &Confirm) {
    let color = if confirm.danger { RED } else { MAUVE };
    let mut lines: Vec<Line> = confirm
        .body
        .iter()
        .enumerate()
        .map(|(i, text)| {
            if i == 0 {
                Line::styled(text.clone(), Style::new().fg(TEXT).bold())
            } else {
                Line::styled(
                    text.clone(),
                    Style::new().fg(if confirm.danger { PEACH } else { SUBTEXT }),
                )
            }
        })
        .collect();
    command_lines(&confirm.argv, &mut lines);
    let text = Text::from(lines);
    let width = width_for(area, 100);
    let height = wrapped_height(&text, width.saturating_sub(4)) + 5;
    let (content, row) = dialog(frame, area, width, height, &confirm.title, color);
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), content);
    let run = if confirm.danger { "Yes, run it" } else { "Run" };
    buttons(
        frame,
        app,
        row,
        &[
            ("Enter", run, Some(Action::ModalConfirm)),
            ("Esc", "Cancel", Some(Action::ModalCancel)),
            ("c", "Copy command", None),
        ],
    );
}

fn draw_input(frame: &mut Frame, app: &mut App, area: Rect, input: &Input) {
    let (content, row) = dialog(frame, area, width_for(area, 80), 8, &input.title, MAUVE);
    let on = (app.tick / 6).is_multiple_of(2);
    let mut field = vec![Span::styled("› ", Style::new().fg(MAUVE).bold())];
    let (before, rest) = input.value.split_at(input.cursor.min(input.value.len()));
    field.push(value(before.to_string()));
    match rest.chars().next() {
        // Mid-text the caret highlights the character it sits on, so nothing shifts as it blinks.
        Some(c) => {
            let style = if on {
                Style::new().fg(CRUST).bg(MAUVE)
            } else {
                Style::new().fg(TEXT)
            };
            field.push(Span::styled(c.to_string(), style));
            field.push(value(rest[c.len_utf8()..].to_string()));
        }
        None => field.push(colored(if on { "▏" } else { " " }, MAUVE)),
    }
    let lines = vec![
        Line::styled(input.prompt.clone(), Style::new().fg(SUBTEXT)),
        Line::default(),
        Line::from(field),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), content);
    buttons(
        frame,
        app,
        row,
        &[
            ("Enter", "Continue", Some(Action::ModalConfirm)),
            ("Esc", "Cancel", Some(Action::ModalCancel)),
        ],
    );
}

/// A selectable list with a highlighted cursor that keeps the cursor visible and registers row hitboxes.
fn draw_list(frame: &mut Frame, app: &mut App, area: Rect, rows: Vec<Line<'static>>, cursor: usize) {
    let visible = area.height as usize;
    if visible == 0 {
        return;
    }
    let offset = cursor.saturating_sub(visible - 1);
    for (i, line) in rows.into_iter().enumerate().skip(offset).take(visible) {
        let rect = Rect::new(area.x, area.y + (i - offset) as u16, area.width, 1);
        let selected = i == cursor;
        let hovered = app.hover.is_some_and(|p| rect.contains(p));
        let style = if selected {
            Style::new().bg(SURFACE1)
        } else if hovered {
            Style::new().bg(SURFACE0)
        } else {
            Style::new()
        };
        let mut spans = vec![Span::styled(
            if selected { "▌ " } else { "  " },
            Style::new().fg(MAUVE),
        )];
        spans.extend(line.spans);
        frame.render_widget(Paragraph::new(Line::from(spans)).style(style), rect);
        app.hits.modal_rows.push((rect, i));
    }
}

fn draw_picker(frame: &mut Frame, app: &mut App, area: Rect, picker: &Picker) {
    let height = (picker.items.len() as u16 + 5).min(area.height.saturating_sub(2));
    let (content, row) = dialog(frame, area, width_for(area, 90), height, &picker.title, MAUVE);
    let rows = picker
        .items
        .iter()
        .map(|item| {
            Line::from(vec![
                Span::styled(format!("{:<24}", item.label), Style::new().fg(LAVENDER).bold()),
                dim(item.detail.clone()),
            ])
        })
        .collect();
    draw_list(frame, app, content, rows, picker.cursor);
    buttons(
        frame,
        app,
        row,
        &[
            ("Enter", "Choose", Some(Action::ModalConfirm)),
            ("Esc", "Cancel", Some(Action::ModalCancel)),
        ],
    );
}

fn draw_wizard(frame: &mut Frame, app: &mut App, area: Rect, wizard: &Wizard) {
    let step = wizard.step;
    let mut title = format!(
        "Schedule push upgrade · step {} of 4: {}",
        step.number(),
        step.title()
    );
    if let Some(request) = &wizard.retry_of {
        title.push_str(&format!(" · retry of {request}"));
    }
    let color = if step == Step::Confirm { RED } else { MAUVE };
    let width = width_for(area, 116);
    let height = area.height.saturating_sub(4).min(32);
    let (content, row) = dialog(frame, area, width, height, &title, color);

    match step {
        Step::Version => {
            let [intro, list] = Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(content);
            frame.render_widget(
                Line::styled(
                    "Which released version should the orgs get? Only released versions can be pushed.",
                    Style::new().fg(SUBTEXT),
                ),
                intro,
            );
            let rows = wizard
                .versions
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let mut spans = vec![
                        Span::styled(format!("{:<10}", v.label), Style::new().fg(TEXT).bold()),
                        dim(v.name.clone()),
                        colored(format!("   {}", v.id), OVERLAY0),
                    ];
                    if i == 0 {
                        spans.push(colored("   latest", GREEN));
                    }
                    Line::from(spans)
                })
                .collect();
            draw_list(frame, app, list, rows, wizard.version);
            buttons(
                frame,
                app,
                row,
                &[
                    ("Enter", "Next", Some(Action::ModalConfirm)),
                    ("↑↓", "Choose", None),
                    ("Esc", "Cancel", Some(Action::ModalCancel)),
                ],
            );
        }
        Step::Orgs => {
            let [intro, list] = Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(content);
            let checked = wizard.checked().len();
            let version = wizard.selected_version();
            let mut intro_spans = vec![
                label("Push "),
                Span::styled(version.label.clone(), Style::new().fg(TEXT).bold()),
                label(" to "),
                Span::styled(
                    format!("{checked} of {} orgs", wizard.orgs.len()),
                    Style::new().fg(if checked > 0 { TEXT } else { RED }).bold(),
                ),
                label(if wizard.has_important() {
                    ". Marked orgs (★) behind this version are preselected."
                } else {
                    ". Orgs behind this version are preselected."
                }),
            ];
            if checked == 0 {
                intro_spans.push(colored("  Select at least one org.", RED));
            }
            frame.render_widget(Line::from(intro_spans), intro);
            let rows = wizard
                .orgs
                .iter()
                .map(|org| {
                    let mut spans = vec![
                        Span::styled(
                            if org.checked { "[x] " } else { "[ ] " },
                            Style::new().fg(if org.checked { GREEN } else { OVERLAY0 }).bold(),
                        ),
                        colored(if org.important { "★ " } else { "  " }, YELLOW),
                        Span::styled(format!("{:<30}", truncate(&org.name, 29)), Style::new().fg(TEXT)),
                        dim(format!("{:<12}", org.org_type)),
                        colored(format!("{:<10}", org.installed), LAVENDER),
                        colored(format!("{:<16}", org.key), OVERLAY0),
                    ];
                    if let Some(warn) = org.warn {
                        spans.push(colored(warn, YELLOW));
                    }
                    Line::from(spans)
                })
                .collect();
            draw_list(frame, app, list, rows, wizard.cursor);
            buttons(
                frame,
                app,
                row,
                &[
                    ("Enter", "Next", Some(Action::ModalConfirm)),
                    ("Space", "Toggle", None),
                    ("a", "All behind", None),
                    ("m", "Marked", None),
                    ("n", "None", None),
                    ("←", "Back", Some(Action::ModalBack)),
                    ("Esc", "Cancel", Some(Action::ModalCancel)),
                ],
            );
        }
        Step::Time => {
            let cursor = if (app.tick / 6).is_multiple_of(2) {
                "▏"
            } else {
                " "
            };
            let mut lines = vec![
                Line::styled(
                    "When should Salesforce start? Enter UTC as YYYY-MM-DDTHH:MM:SS, or leave it empty to start \
                     as soon as possible.",
                    Style::new().fg(SUBTEXT),
                ),
                Line::default(),
                Line::from(vec![
                    Span::styled("› ", Style::new().fg(MAUVE).bold()),
                    value(wizard.start_time.clone()),
                    colored(cursor, MAUVE),
                ]),
                Line::default(),
            ];
            let trimmed = wizard.start_time.trim();
            lines.push(match wizard.start_time_error() {
                Some(error) => Line::styled(error, Style::new().fg(RED)),
                None if trimmed.is_empty() => {
                    Line::styled("Starts as soon as possible", Style::new().fg(GREEN))
                }
                None => {
                    let local = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S")
                        .map(|t| {
                            t.and_utc()
                                .with_timezone(&Local)
                                .format("%d.%m.%Y %H:%M")
                                .to_string()
                        })
                        .unwrap_or_default();
                    Line::styled(format!("Starts {local} local time"), Style::new().fg(GREEN))
                }
            });
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), content);
            buttons(
                frame,
                app,
                row,
                &[
                    ("Enter", "Next", Some(Action::ModalConfirm)),
                    ("←", "Back", Some(Action::ModalBack)),
                    ("Esc", "Cancel", Some(Action::ModalCancel)),
                ],
            );
        }
        Step::Confirm => {
            let spec = wizard.spec();
            let mut lines = vec![
                Line::from(vec![
                    label("Version    "),
                    Span::styled(spec.version_label.clone(), Style::new().fg(TEXT).bold()),
                    colored(format!("  {}", spec.version_id), OVERLAY0),
                ]),
                Line::from(vec![
                    label("Orgs       "),
                    Span::styled(format!("{}", spec.org_keys.len()), Style::new().fg(TEXT).bold()),
                    value(format!(" orgs: {}", spec.org_names.join(", "))),
                ]),
                Line::from(vec![
                    label("Start      "),
                    value(
                        spec.start_time
                            .clone()
                            .map(|t| format!("{t} UTC"))
                            .unwrap_or_else(|| "as soon as possible".into()),
                    ),
                ]),
                Line::from(vec![label("Dev Hub    "), value(wizard.hub.clone())]),
                Line::default(),
                Line::styled(
                    "This upgrades customer orgs. It cannot be undone once the jobs have started.",
                    Style::new().fg(PEACH).bold(),
                ),
            ];
            let warned: Vec<&str> = wizard
                .checked()
                .iter()
                .filter(|o| o.warn.is_some())
                .map(|o| o.name.as_str())
                .collect();
            if !warned.is_empty() {
                lines.push(Line::styled(
                    format!(
                        "Salesforce will probably reject {}, and the request may stay in Created.",
                        warned.join(", ")
                    ),
                    Style::new().fg(YELLOW),
                ));
            }
            command_lines(&wizard.argv(), &mut lines);
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), content);
            buttons(
                frame,
                app,
                row,
                &[
                    ("Enter", "Schedule push upgrade", Some(Action::ModalConfirm)),
                    ("←", "Back", Some(Action::ModalBack)),
                    ("Esc", "Cancel", Some(Action::ModalCancel)),
                    ("c", "Copy command", None),
                ],
            );
        }
    }
}

fn draw_log(frame: &mut Frame, app: &mut App, area: Rect, id: TaskId, scroll: usize) {
    let Some(task) = app.task(id) else {
        return;
    };
    let (status, color) = if task.running() {
        (
            format!(
                "{} running · {}",
                SPINNER[app.tick / 2 % SPINNER.len()],
                fmt_elapsed(task.elapsed())
            ),
            BLUE,
        )
    } else if task.succeeded() {
        (format!("✓ finished in {}", fmt_elapsed(task.elapsed())), GREEN)
    } else {
        match task.exit {
            Some(code) => (
                format!(
                    "✗ failed with exit code {code} after {}",
                    fmt_elapsed(task.elapsed())
                ),
                RED,
            ),
            None => ("✗ cancelled".to_string(), RED),
        }
    };
    let title = task.title.clone();
    let command = format!("sf {}", shell_join(&task.argv));
    let running = task.running();
    let log: Vec<Line> = task
        .lines
        .iter()
        .map(|(stderr, line)| Line::styled(line.clone(), Style::new().fg(if *stderr { PEACH } else { TEXT })))
        .collect();

    let width = area.width.saturating_sub(6);
    let height = area.height.saturating_sub(4);
    let (content, row) = dialog(frame, area, width, height, &title, color);
    let [head, cmd, _, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(content);
    frame.render_widget(Line::styled(status, Style::new().fg(color).bold()), head);
    frame.render_widget(Line::styled(command, Style::new().fg(OVERLAY0)), cmd);

    let visible = body.height as usize;
    let total = log.len();
    let scroll = scroll.min(total.saturating_sub(visible));
    let end = total - scroll;
    let start = end.saturating_sub(visible);
    if total == 0 {
        let text = if running {
            "Waiting for output. Commands that answer in JSON only print when they finish."
        } else {
            "No output."
        };
        frame.render_widget(
            Line::styled(text, Style::new().fg(OVERLAY0)).alignment(Alignment::Left),
            body,
        );
    } else {
        frame.render_widget(Paragraph::new(log[start..end].to_vec()), body);
    }

    let mut items: Vec<(&str, &str, Option<Action>)> = vec![("Esc", "Close", Some(Action::ModalCancel))];
    if running {
        items.push(("x", "Cancel command", None));
    }
    items.push(("↑↓", "Scroll", None));
    items.push(("c", "Copy output", None));
    buttons(frame, app, row, &items);
}

fn draw_message(frame: &mut Frame, app: &mut App, area: Rect, title: &str, body: &[String], error: bool) {
    let color = if error { RED } else { MAUVE };
    let text = Text::from(
        body.iter()
            .map(|line| match line.strip_prefix("# ") {
                Some(heading) => Line::styled(heading.to_string(), Style::new().fg(PEACH).bold()),
                None => Line::styled(line.clone(), Style::new().fg(TEXT)),
            })
            .collect::<Vec<_>>(),
    );
    let width = width_for(area, 104);
    let height = wrapped_height(&text, width.saturating_sub(4)) + 5;
    let (content, row) = dialog(frame, area, width, height, title, color);
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), content);
    buttons(frame, app, row, &[("Enter", "Close", Some(Action::ModalCancel))]);
}

/// Release notes are Markdown; headings and bullets get a light touch, the rest is shown as written.
fn draw_update(frame: &mut Frame, app: &mut App, area: Rect, release: &ReleaseInfo, installed: bool) {
    const MAX_NOTE_LINES: usize = 24;
    let mut lines = vec![if installed {
        Line::styled(
            format!("You are now on sf-cockpit {}.", release.version),
            Style::new().fg(TEXT).bold(),
        )
    } else {
        Line::styled(
            format!(
                "sf-cockpit {} is available, you have {}.",
                release.version,
                update::current_version()
            ),
            Style::new().fg(TEXT).bold(),
        )
    }];
    let notes: Vec<&str> = release.notes.lines().filter(|l| !l.trim().is_empty()).collect();
    if !notes.is_empty() {
        lines.push(Line::default());
        lines.push(Line::styled("What's new", Style::new().fg(OVERLAY0).bold()));
    }
    for note in notes.iter().take(MAX_NOTE_LINES) {
        let trimmed = note.trim_start();
        lines.push(if let Some(heading) = trimmed.strip_prefix('#') {
            Line::styled(
                heading.trim_start_matches('#').trim().to_string(),
                Style::new().fg(PEACH).bold(),
            )
        } else if let Some(item) = trimmed.strip_prefix("* ").or_else(|| trimmed.strip_prefix("- ")) {
            Line::styled(format!("• {item}"), Style::new().fg(TEXT))
        } else {
            Line::styled(note.to_string(), Style::new().fg(TEXT))
        });
    }
    if notes.len() > MAX_NOTE_LINES || notes.is_empty() {
        lines.push(Line::default());
        lines.push(Line::styled(release.url.clone(), Style::new().fg(SUBTEXT)));
    }
    let text = Text::from(lines);
    let width = width_for(area, 100);
    let height = wrapped_height(&text, width.saturating_sub(4)) + 5;
    let title = if installed {
        "What's new"
    } else {
        "Update available"
    };
    let (content, row) = dialog(frame, area, width, height, title, GREEN);
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), content);
    if installed {
        buttons(frame, app, row, &[("Enter", "Close", Some(Action::ModalCancel))]);
    } else {
        buttons(
            frame,
            app,
            row,
            &[
                ("Enter", "Update now", Some(Action::ModalConfirm)),
                ("Esc", "Later", Some(Action::ModalCancel)),
            ],
        );
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!(
            "{}…",
            text.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}
