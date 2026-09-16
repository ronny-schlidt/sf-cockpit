mod deploy;
pub mod modal;
mod orgs;
mod push;
mod settings;
mod subscribers;
pub mod table;
mod versions;

use crate::app::{Action, App, Loadable, TabId, Target, tabs};
use crate::sf::query::Timestamp;
use crate::theme::*;
use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};
use std::time::Duration;

pub(super) const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(BASE).fg(TEXT)), area);
    app.hits = Default::default();

    let [top, body, bottom] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    let body = body.inner(Margin::new(1, 0));

    draw_top_bar(frame, app, top);
    match app.tab {
        TabId::Push => push::draw(frame, app, body),
        TabId::Subscribers => subscribers::draw(frame, app, body),
        TabId::Orgs => orgs::draw(frame, app, body),
        TabId::Versions => versions::draw(frame, app, body),
        TabId::Deploy => deploy::draw(frame, app, body),
        TabId::Settings => settings::draw(frame, app, body),
    }
    draw_bottom_bar(frame, app, bottom);
    modal::draw(frame, app);
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

    let spinner = SPINNER[app.tick / 2 % SPINNER.len()];
    let title = vec![
        Span::styled(" ◆ ", Style::new().fg(MAUVE).bold()),
        Span::styled("sf-cockpit", Style::new().fg(TEXT).bold()),
        Span::raw("   "),
    ];
    let tab_texts = |short: bool| -> Vec<String> {
        tabs::TABS
            .iter()
            .map(|tab| {
                let label = if short { tab.short } else { tab.label };
                if tab_loading(app, tab.id) {
                    format!(" {} {label} {spinner} ", tab.key)
                } else {
                    format!(" {} {label} ", tab.key)
                }
            })
            .collect()
    };
    let (full, short) = (tab_texts(false), tab_texts(true));
    let tabs_width = |texts: &[String]| -> usize { texts.iter().map(|t| t.chars().count() + 1).sum() };
    let title_width: usize = title.iter().map(Span::width).sum();

    // Tabs always win: drop the title first, then shorten the status step by step, then the tab labels.
    // Nothing is ever drawn over a tab, so every tab stays visible and clickable.
    let width = area.width as usize;
    let statuses = [Detail::Full, Detail::NoBadges, Detail::Compact].map(|d| status_line(app, spinner, d));
    let layouts = [
        (true, false, Some(0)),
        (false, false, Some(0)),
        (false, false, Some(1)),
        (false, false, Some(2)),
        (false, true, Some(2)),
        (false, true, None),
    ];
    let (show_title, short_tabs, status) = layouts
        .into_iter()
        .find(|&(show_title, short_tabs, status)| {
            let left = if show_title { title_width } else { 1 };
            let tabs = tabs_width(if short_tabs { &short } else { &full });
            let right = status.map_or(0, |i| statuses[i].width() + 1);
            left + tabs + right <= width
        })
        .unwrap_or((false, true, None));
    let texts = if short_tabs { short } else { full };

    let mut spans = if show_title { title } else { vec![Span::raw(" ")] };
    let mut x = area.x + spans.iter().map(|s| s.width() as u16).sum::<u16>();
    for (tab, text) in tabs::TABS.iter().zip(texts) {
        let tab_width = text.chars().count() as u16;
        let rect = Rect::new(x, area.y, tab_width, 1).intersection(area);
        let hovered = app.hover.is_some_and(|p| rect.contains(p));
        let style = if app.tab == tab.id {
            Style::new().bg(MAUVE).fg(CRUST).bold()
        } else if hovered {
            Style::new().bg(SURFACE0).fg(TEXT)
        } else {
            Style::new().fg(SUBTEXT)
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::raw(" "));
        app.hits.buttons.push((rect, Action::ShowTab(tab.id)));
        x = x.saturating_add(tab_width + 1);
    }
    frame.render_widget(Line::from(spans), area);

    if let Some(line) = status.and_then(|i| statuses.into_iter().nth(i)) {
        let free = Rect::new(x, area.y, area.right().saturating_sub(x), 1);
        frame.render_widget(line.alignment(Alignment::Right), free);
    }
}

/// How much the status in the top bar shows, from most to least.
#[derive(Clone, Copy, PartialEq)]
enum Detail {
    Full,
    /// Without the "DEMO DATA" and "auto-refresh" badges.
    NoBadges,
    /// Only the spinner or dot and the org.
    Compact,
}

/// The right side of the top bar.
fn status_line(app: &App, spinner: &str, detail: Detail) -> Line<'static> {
    if let Some(task) = app.running_task() {
        let mut spans = vec![
            Span::styled(format!("{spinner} "), Style::new().fg(PEACH)),
            Span::styled(format!("{} ", task.title), Style::new().fg(TEXT)),
        ];
        if detail != Detail::Compact {
            spans.push(Span::styled(
                format!("{} ", fmt_elapsed(task.elapsed())),
                Style::new().fg(OVERLAY0),
            ));
        }
        Line::from(spans)
    } else {
        let state = match app.tab {
            TabId::Push | TabId::Subscribers | TabId::Settings => State::of(&app.push),
            TabId::Orgs => State::of(&app.orgs),
            TabId::Versions => State::of(&app.versions),
            TabId::Deploy => State::of(&app.deploys),
        };
        let org = if app.tab == TabId::Deploy && !app.deploy_org.is_empty() {
            app.deploy_org.clone()
        } else if app.tab == TabId::Orgs {
            "local sf orgs".into()
        } else if app.cfg.needs_setup() {
            "no Dev Hub yet".into()
        } else {
            app.cfg.dev_hub.clone()
        };
        if detail == Detail::Compact {
            let (symbol, color) = match (state.loading, state.error) {
                (true, _) => (spinner, MAUVE),
                (false, true) => ("✗", RED),
                (false, false) => ("●", GREEN),
            };
            return Line::from(vec![
                Span::styled(format!("{symbol} "), Style::new().fg(color)),
                Span::styled(format!("{org} "), Style::new().fg(TEXT).bold()),
            ]);
        }
        let mut spans = Vec::new();
        if app.demo && detail == Detail::Full {
            spans.push(Span::styled(
                " DEMO DATA ",
                Style::new().bg(PEACH).fg(CRUST).bold(),
            ));
            spans.push(Span::raw("  "));
        }
        if detail == Detail::Full
            && app.auto_refresh_active()
            && matches!(app.tab, TabId::Push | TabId::Subscribers)
        {
            spans.push(Span::styled("auto-refresh ", Style::new().fg(BLUE)));
        }
        let age = state.loaded_at.map(fmt_age).unwrap_or_default();
        match (state.loading, state.has_value, state.error) {
            (true, true, _) => {
                spans.push(Span::styled(
                    format!("{spinner} refreshing "),
                    Style::new().fg(MAUVE),
                ));
                spans.push(Span::styled(format!("{org} "), Style::new().fg(TEXT).bold()));
                spans.push(Span::styled(
                    format!("· showing data from {age} "),
                    Style::new().fg(OVERLAY0),
                ));
            }
            (true, false, _) => {
                spans.push(Span::styled(
                    format!("{spinner} loading "),
                    Style::new().fg(MAUVE),
                ));
                spans.push(Span::styled(format!("{org} "), Style::new().fg(TEXT).bold()));
            }
            (false, true, true) => {
                spans.push(Span::styled("✗ refresh failed ", Style::new().fg(RED).bold()));
                spans.push(Span::styled(format!("{org} "), Style::new().fg(TEXT).bold()));
                spans.push(Span::styled(
                    format!("· showing data from {age} "),
                    Style::new().fg(OVERLAY0),
                ));
            }
            _ => {
                let color = if state.error { RED } else { GREEN };
                spans.push(Span::styled("● ", Style::new().fg(color)));
                spans.push(Span::styled(format!("{org} "), Style::new().fg(TEXT).bold()));
                if let Some(at) = state.loaded_at {
                    let text = if state.from_cache {
                        format!("cached {age} ")
                    } else {
                        format!("updated {} ", at.format("%H:%M:%S"))
                    };
                    spans.push(Span::styled(text, Style::new().fg(OVERLAY0)));
                }
            }
        }
        Line::from(spans)
    }
}

struct State {
    loading: bool,
    has_value: bool,
    error: bool,
    from_cache: bool,
    loaded_at: Option<DateTime<Local>>,
}

impl State {
    fn of<T>(loadable: &Loadable<T>) -> Self {
        Self {
            loading: loadable.loading,
            has_value: loadable.value.is_some(),
            error: loadable.error.is_some(),
            from_cache: loadable.from_cache,
            loaded_at: loadable.loaded_at,
        }
    }
}

fn tab_loading(app: &App, tab: TabId) -> bool {
    match tab {
        TabId::Push | TabId::Subscribers => app.push.loading,
        TabId::Orgs => app.orgs.loading,
        TabId::Versions => app.versions.loading,
        TabId::Deploy => app.deploys.loading,
        TabId::Settings => false,
    }
}

/// "just now", "5 min ago", "2 h ago", "3 d ago".
pub fn fmt_age(at: DateTime<Local>) -> String {
    let seconds = (Local::now() - at).num_seconds().max(0);
    match seconds {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    }
}

fn draw_bottom_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    frame.render_widget(Block::new().style(Style::new().bg(MANTLE)), area);

    let mut buttons: Vec<(&str, &str, Option<Action>)> = vec![("r", "Reload", Some(Action::Reload))];
    match app.tab {
        TabId::Push => buttons.extend([
            ("s", "Schedule", Some(Action::Schedule)),
            ("a", "Abort", Some(Action::Abort)),
            ("f", "Retry failed", Some(Action::RetryFailed)),
            ("c", "Copy", Some(Action::Copy)),
            ("tab", "Pane", Some(Action::NextPane)),
            ("[ ]", "Resize", None),
        ]),
        TabId::Subscribers => buttons.extend([
            ("/", "Filter", Some(Action::Filter)),
            ("s", "Schedule", Some(Action::Schedule)),
        ]),
        TabId::Orgs => buttons.extend([
            ("/", "Filter", Some(Action::Filter)),
            ("o", "Open", Some(Action::OpenOrg)),
            ("i", "Installed", Some(Action::Installed)),
            ("I", "All", Some(Action::InstalledAll)),
            ("c", "Copy user", Some(Action::Copy)),
            ("C", "Copy ID", Some(Action::CopyOrgId)),
            ("d", "Delete scratch", Some(Action::DeleteScratch)),
        ]),
        TabId::Versions => buttons.extend([
            ("c", "Copy 04t", Some(Action::Copy)),
            ("u", "Install URL", Some(Action::CopyInstallUrl)),
            ("U", "Sandbox URL", Some(Action::CopySandboxUrl)),
            ("p", "Promote", Some(Action::Promote)),
            ("i", "Install", Some(Action::Install)),
            ("n", "New version", Some(Action::CreateVersion)),
        ]),
        TabId::Deploy => buttons.extend([
            ("o", "Org", Some(Action::PickDeployOrg)),
            ("D", "Deploy", Some(Action::Deploy)),
            ("t", "Tests", Some(Action::RunTests)),
            ("c", "Copy", Some(Action::Copy)),
        ]),
        TabId::Settings => buttons.extend([
            ("Enter", "Change", Some(Action::EditSetting)),
            ("C", "Clear cache", Some(Action::ClearCache)),
        ]),
    }
    if !app.tasks.is_empty() {
        buttons.push(("L", "Log", Some(Action::ShowLog)));
    }
    if app.running_task().is_some() {
        buttons.push(("x", "Cancel", Some(Action::CancelTask)));
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

// ─── Shared pieces ───────────────────────────────────────────────────────────

/// Draws the loading or error state of a tab and returns whether the tab should draw its data.
pub(super) fn ready<T>(frame: &mut Frame, app: &App, area: Rect, loadable: &Loadable<T>, what: &str) -> bool {
    if loadable.value.is_some() {
        return true;
    }
    if loadable.loading {
        draw_loading(frame, app, area, what);
    } else {
        draw_load_error(
            frame,
            app,
            area,
            loadable.error.as_deref().unwrap_or("Nothing loaded yet"),
        );
    }
    false
}

/// Tabs that need a Dev Hub show where to set it instead of an error. Returns true when it drew the hint.
pub(super) fn setup_hint(frame: &mut Frame, app: &mut App, area: Rect) -> bool {
    if !app.cfg.needs_setup() {
        return false;
    }
    let rect = centered(area, 76, 9);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(PEACH))
        .title(Line::styled(
            " No Dev Hub chosen yet ",
            Style::new().fg(PEACH).bold(),
        ))
        .style(Style::new().bg(MANTLE));
    let lines = vec![
        Line::styled(
            "This tab reads your package from the Dev Hub that owns it.",
            Style::new().fg(TEXT),
        ),
        Line::default(),
        Line::from(vec![
            Span::styled("Press ", Style::new().fg(SUBTEXT)),
            Span::styled(" 6 ", Style::new().bg(SURFACE0).fg(TEXT).bold()),
            Span::styled(
                " to open Settings and choose the Dev Hub.",
                Style::new().fg(SUBTEXT),
            ),
        ]),
        Line::default(),
        Line::styled(
            "Not logged in to it? Quit and run: sf org login web --alias DevHub --set-default-dev-hub",
            Style::new().fg(OVERLAY0),
        ),
    ];
    let button = Rect::new(rect.x + 7, rect.y + 3, 3, 1);
    app.hits.buttons.push((button, Action::ShowTab(TabId::Settings)));
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(block.padding(ratatui::widgets::Padding::horizontal(1))),
        rect,
    );
    true
}

fn draw_loading(frame: &mut Frame, app: &App, area: Rect, what: &str) {
    let rect = centered(area, 56, 5);
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
            Span::styled(what.to_string(), Style::new().fg(TEXT)),
        ]),
        Line::default(),
        Line::styled("Runs the sf CLI, takes a few seconds", Style::new().fg(OVERLAY0)),
    ];
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center).block(block),
        rect,
    );
}

fn draw_load_error(frame: &mut Frame, app: &App, area: Rect, error: &str) {
    let rect = centered(area, area.width.saturating_mul(7) / 10, 9);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(RED))
        .title(Line::styled(" Could not load data ", Style::new().fg(RED).bold()))
        .style(Style::new().bg(MANTLE));
    let lines = vec![
        Line::styled(error.to_string(), Style::new().fg(TEXT)),
        Line::default(),
        Line::from(vec![
            Span::styled("Check that ", Style::new().fg(OVERLAY0)),
            Span::styled(app.cfg.dev_hub.clone(), Style::new().fg(TEXT).bold()),
            Span::styled(
                " is logged in (sf org list), then press ",
                Style::new().fg(OVERLAY0),
            ),
            Span::styled(" r ", Style::new().bg(SURFACE0).fg(TEXT).bold()),
        ]),
    ];
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }).block(block), rect);
}

/// A filter box that edits the current tab's filter.
pub(super) fn draw_filter(frame: &mut Frame, app: &mut App, area: Rect, placeholder: &'static str) {
    app.hits.register(Target::Filter, area);
    let filter = app.current_filter().to_string();
    let line = if filter.is_empty() && !app.editing_filter {
        Line::styled(placeholder, Style::new().fg(OVERLAY0))
    } else {
        let cursor = if app.editing_filter && (app.tick / 6).is_multiple_of(2) {
            "▏"
        } else {
            " "
        };
        Line::from(vec![
            Span::styled("› ", Style::new().fg(MAUVE).bold()),
            Span::styled(filter, Style::new().fg(TEXT)),
            Span::styled(cursor, Style::new().fg(MAUVE)),
        ])
    };
    let block = pane("Filter".into(), app.editing_filter);
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);
    frame.render_widget(line, inner);
}

/// A scrollable text pane whose scroll position lives in the app.
pub(super) fn draw_scrollable(
    frame: &mut Frame,
    app: &mut App,
    target: Target,
    area: Rect,
    title: &str,
    text: Text<'static>,
) {
    app.hits.register(target, area);
    let block = pane(title.to_string(), app.focus == target);
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);

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

pub(super) fn pane(title: String, focused: bool) -> Block<'static> {
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

pub(super) fn header(titles: &[&'static str]) -> Row<'static> {
    Row::new(titles.to_vec()).style(Style::new().fg(OVERLAY0).add_modifier(Modifier::BOLD))
}

pub(super) fn selected_style(focused: bool) -> Style {
    Style::new().bg(if focused { SURFACE1 } else { SURFACE0 })
}

pub(super) fn row_style(hovered: bool) -> Style {
    if hovered {
        Style::new().bg(Color::Rgb(40, 40, 58))
    } else {
        Style::new()
    }
}

pub(super) fn label(text: &str) -> Span<'static> {
    Span::styled(text.to_string(), Style::new().fg(OVERLAY0))
}

pub(super) fn value(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(TEXT))
}

pub(super) fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(SUBTEXT))
}

pub(super) fn colored(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(color))
}

pub(super) fn empty_message(frame: &mut Frame, area: Rect, message: &'static str) {
    let rect = centered(area, area.width.saturating_sub(4), 1);
    frame.render_widget(
        Line::styled(message, Style::new().fg(OVERLAY0)).alignment(Alignment::Center),
        rect,
    );
}

pub(super) fn centered(area: Rect, width: u16, height: u16) -> Rect {
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
pub(super) fn wrapped_height(text: &Text, width: u16) -> u16 {
    let width = width.max(1) as usize;
    text.lines
        .iter()
        .map(|line| (line.width() * 11 / 10).div_ceil(width).max(1))
        .sum::<usize>()
        .min(u16::MAX as usize) as u16
}

pub fn fmt_date(time: Option<Timestamp>) -> String {
    time.map(|t| t.with_timezone(&Local).format("%d.%m. %H:%M").to_string())
        .unwrap_or_else(|| "—".into())
}

pub(super) fn fmt_time(time: Option<Timestamp>) -> String {
    time.map(|t| t.with_timezone(&Local).format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "—".into())
}

pub fn fmt_duration(start: Option<Timestamp>, end: Option<Timestamp>) -> String {
    let Some(start) = start else {
        return "—".into();
    };
    let end = end.map(|e| e.with_timezone(&Utc)).unwrap_or_else(Utc::now);
    fmt_seconds((end - start.with_timezone(&Utc)).num_seconds().max(0))
}

pub(super) fn fmt_elapsed(elapsed: Duration) -> String {
    fmt_seconds(elapsed.as_secs() as i64)
}

fn fmt_seconds(seconds: i64) -> String {
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, s % 3600 / 60),
    }
}
