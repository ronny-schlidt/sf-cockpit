use crate::sf::{self, Data, PushError, PushJob, PushRequest, Subscriber};
use crate::{clipboard, demo};
use chrono::{DateTime, Local};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Margin, Position, Rect};
use ratatui::widgets::TableState;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

const MIN_SPLIT: u16 = 20;
const MAX_SPLIT: u16 = 75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Push,
    Subscribers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Requests,
    Jobs,
    Details,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Reload,
    Copy,
    Filter,
    NextPane,
    Quit,
    ShowTab(Tab),
}

/// Screen regions from the last rendered frame, used to route mouse events.
#[derive(Default)]
pub struct Hitboxes {
    pub body: Rect,
    pub divider_x: u16,
    pub requests: Rect,
    pub jobs: Rect,
    pub details: Rect,
    pub filter: Rect,
    pub subscribers: Rect,
    pub buttons: Vec<(Rect, Action)>,
}

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

    fn clamp(&self, pos: Position) -> Position {
        let region = self.region;
        Position::new(
            pos.x.clamp(region.x, region.right().saturating_sub(1)),
            pos.y.clamp(region.y, region.bottom().saturating_sub(1)),
        )
    }
}

pub struct Toast {
    pub text: String,
    pub is_error: bool,
    created: Instant,
}

type LoadResult = Result<Data, String>;

pub struct App {
    pub org: String,
    pub demo: bool,
    limit: usize,
    pub data: Option<Data>,
    pub loading: bool,
    pub load_error: Option<String>,
    pub loaded_at: Option<DateTime<Local>>,
    pub tab: Tab,
    pub pane: Pane,
    pub requests: TableState,
    pub jobs: TableState,
    pub subscribers: TableState,
    pub detail_scroll: u16,
    pub detail_max_scroll: u16,
    pub filter: String,
    pub editing_filter: bool,
    pub split: u16,
    pub dragging: bool,
    pub hover: Option<Position>,
    pub hits: Hitboxes,
    /// Last rendered screen, the source for copying selected text.
    pub screen: Buffer,
    pub selection: Option<Selection>,
    press: Option<(Position, Rect)>,
    pub last_copied: Option<String>,
    pub toast: Option<Toast>,
    pub tick: usize,
    pub quit: bool,
    tx: Sender<LoadResult>,
    rx: Receiver<LoadResult>,
}

impl App {
    pub fn new(org: String, demo: bool, limit: usize) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            org,
            demo,
            limit,
            data: None,
            loading: false,
            load_error: None,
            loaded_at: None,
            tab: Tab::Push,
            pane: Pane::Requests,
            requests: TableState::default(),
            jobs: TableState::default(),
            subscribers: TableState::default(),
            detail_scroll: 0,
            detail_max_scroll: 0,
            filter: String::new(),
            editing_filter: false,
            split: 38,
            dragging: false,
            hover: None,
            hits: Hitboxes::default(),
            screen: Buffer::empty(Rect::default()),
            selection: None,
            press: None,
            last_copied: None,
            toast: None,
            tick: 0,
            quit: false,
            tx,
            rx,
        }
    }

    pub fn reload(&mut self) {
        if self.loading {
            return;
        }
        self.loading = true;
        if self.demo {
            let _ = self.tx.send(Ok(demo::data()));
            return;
        }
        let (tx, org, limit) = (self.tx.clone(), self.org.clone(), self.limit);
        std::thread::spawn(move || {
            let _ = tx.send(sf::load(&org, limit).map_err(|e| format!("{e:#}")));
        });
    }

    /// Picks up finished background loads.
    pub fn poll(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            self.loading = false;
            match result {
                Ok(data) => self.set_data(data),
                Err(message) => {
                    self.notify(format!("Reload failed: {message}"), true);
                    self.load_error = Some(message);
                }
            }
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|t| t.created.elapsed() > Duration::from_secs(4))
        {
            self.toast = None;
        }
    }

    pub fn set_data(&mut self, data: Data) {
        let selected_request = self.selected_request().map(|r| r.id.clone());
        let selected_job = self.selected_job().map(|j| j.id.clone());
        self.data = Some(data);
        self.load_error = None;
        self.loaded_at = Some(Local::now());

        let data = self.data.as_ref().expect("data was just set");
        let request_index = selected_request
            .and_then(|id| data.requests.iter().position(|r| r.id == id))
            .unwrap_or(0);
        self.requests
            .select((!data.requests.is_empty()).then_some(request_index));

        let jobs = self.current_jobs();
        let job_index = selected_job
            .and_then(|id| jobs.iter().position(|j| j.id == id))
            .unwrap_or(0);
        let has_jobs = !jobs.is_empty();
        self.jobs.select(has_jobs.then_some(job_index));
        self.clamp_subscriber_selection();
    }

    pub fn notify(&mut self, text: impl Into<String>, is_error: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            is_error,
            created: Instant::now(),
        });
    }

    // ─── Derived state ───────────────────────────────────────────────────────

    pub fn selected_request(&self) -> Option<&PushRequest> {
        self.request_in(self.data.as_ref()?)
    }

    pub fn current_jobs(&self) -> Vec<&PushJob> {
        self.data
            .as_ref()
            .map(|data| self.jobs_in(data))
            .unwrap_or_default()
    }

    pub fn selected_job(&self) -> Option<&PushJob> {
        self.job_in(self.data.as_ref()?)
    }

    pub fn filtered_subscribers(&self) -> Vec<&Subscriber> {
        self.data
            .as_ref()
            .map(|data| self.subscribers_in(data))
            .unwrap_or_default()
    }

    // The `*_in` variants take the data explicitly, because rendering moves it out of the app.

    pub fn request_in<'a>(&self, data: &'a Data) -> Option<&'a PushRequest> {
        data.requests.get(self.requests.selected()?)
    }

    pub fn jobs_in<'a>(&self, data: &'a Data) -> Vec<&'a PushJob> {
        self.request_in(data)
            .map(|r| data.jobs_for(&r.id))
            .unwrap_or_default()
    }

    pub fn job_in<'a>(&self, data: &'a Data) -> Option<&'a PushJob> {
        self.jobs_in(data).get(self.jobs.selected()?).copied()
    }

    pub fn subscribers_in<'a>(&self, data: &'a Data) -> Vec<&'a Subscriber> {
        let needle = self.filter.trim().to_lowercase();
        let mut subscribers: Vec<&Subscriber> = data
            .subscribers
            .iter()
            .filter(|s| {
                if needle.is_empty() {
                    return true;
                }
                let alias = data
                    .alias(&s.org_key)
                    .map(|o| o.alias.as_str())
                    .unwrap_or_default();
                [
                    s.name.as_str(),
                    alias,
                    &s.org_key,
                    &s.instance,
                    &s.org_type,
                    &data.version_label(&s.version_id),
                ]
                .iter()
                .any(|field| field.to_lowercase().contains(&needle))
            })
            .collect();
        subscribers.sort_by_key(|s| (data.is_latest(&s.version_id), s.name.to_lowercase()));
        subscribers
    }

    /// Plain-text version of the details pane, for the clipboard.
    pub fn details_text(&self) -> Option<String> {
        let data = self.data.as_ref()?;
        let job = self.selected_job()?;
        let request = self.selected_request()?;
        let mut out = format!(
            "Push upgrade {} ({})\nOrg: {} [{}]\nJob status: {}\n",
            data.version_label(&request.version_id),
            request.id,
            data.org_name(&job.org_key),
            job.org_key,
            job.status
        );
        for error in data.errors_for(&job.id) {
            out.push_str(&format!(
                "\n{} ({}, {})\n{}\n",
                error.title, error.kind, error.severity, error.message
            ));
            if !error.details.is_empty() {
                out.push_str(&format!("{}\n", error.details));
            }
            out.push_str(&format!("Hint: {}\n", hint(error)));
        }
        Some(out)
    }

    // ─── Input ───────────────────────────────────────────────────────────────

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        if self.selection.take().is_some() && key.code == KeyCode::Esc {
            return;
        }
        if self.editing_filter {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Down | KeyCode::Up => self.editing_filter = false,
                KeyCode::Backspace => {
                    self.filter.pop();
                }
                KeyCode::Char(c) => self.filter.push(c),
                _ => {}
            }
            self.subscribers.select(Some(0));
            self.clamp_subscriber_selection();
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc if self.tab == Tab::Subscribers && !self.filter.is_empty() => {
                self.filter.clear();
                self.clamp_subscriber_selection();
            }
            KeyCode::Esc => self.quit = true,
            KeyCode::Char('r') => self.run(Action::Reload),
            KeyCode::Char('c') => self.run(Action::Copy),
            KeyCode::Char('/') => self.run(Action::Filter),
            KeyCode::Char('1') => self.run(Action::ShowTab(Tab::Push)),
            KeyCode::Char('2') => self.run(Action::ShowTab(Tab::Subscribers)),
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.run(Action::NextPane),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => self.cycle_pane(-1),
            KeyCode::Enter if self.pane == Pane::Requests => self.pane = Pane::Jobs,
            KeyCode::Char('[') => self.split = self.split.saturating_sub(4).max(MIN_SPLIT),
            KeyCode::Char(']') => self.split = (self.split + 4).min(MAX_SPLIT),
            KeyCode::Down | KeyCode::Char('j') => self.scroll(self.focused_target(), 1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll(self.focused_target(), -1),
            KeyCode::PageDown => self.scroll(self.focused_target(), 10),
            KeyCode::PageUp => self.scroll(self.focused_target(), -10),
            KeyCode::Home | KeyCode::Char('g') => self.scroll(self.focused_target(), -10_000),
            KeyCode::End | KeyCode::Char('G') => self.scroll(self.focused_target(), 10_000),
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        let pos = Position::new(mouse.column, mouse.row);
        self.hover = Some(pos);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.selection = None;
                self.press = None;
                if let Some(action) = self
                    .hits
                    .buttons
                    .iter()
                    .find(|(r, _)| r.contains(pos))
                    .map(|(_, a)| *a)
                {
                    self.run(action);
                } else if self.on_divider(pos) {
                    self.dragging = true;
                } else {
                    let target = self.target_at(pos);
                    let region = match target {
                        Some(target) => self.target_rect(target).inner(Margin::new(1, 1)),
                        None => self.screen.area,
                    };
                    self.press = Some((pos, region));
                    if let Some(target) = target {
                        self.click(target, pos);
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                let body = self.hits.body;
                if body.width > 0 {
                    let offset = pos.x.saturating_sub(body.x) as u32 * 100 / body.width as u32;
                    self.split = (offset as u16).clamp(MIN_SPLIT, MAX_SPLIT);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some((anchor, region)) = self.press
                    && region.contains(anchor)
                {
                    let selection = Selection {
                        anchor,
                        cursor: anchor,
                        region,
                    };
                    self.selection = Some(Selection {
                        cursor: selection.clamp(pos),
                        ..selection
                    });
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.dragging = false;
                self.press = None;
                if let Some(selection) = self.selection.filter(|s| s.anchor != s.cursor) {
                    self.copy_text(selection.text(&self.screen));
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(target) = self.target_at(pos) {
                    self.selection = None;
                    self.scroll(target, if target == Target::Details { 3 } else { 1 });
                }
            }
            MouseEventKind::ScrollUp => {
                if let Some(target) = self.target_at(pos) {
                    self.selection = None;
                    self.scroll(target, if target == Target::Details { -3 } else { -1 });
                }
            }
            _ => {}
        }
    }

    pub fn on_divider(&self, pos: Position) -> bool {
        self.tab == Tab::Push
            && self.hits.body.contains(pos)
            && (pos.x == self.hits.divider_x || pos.x + 1 == self.hits.divider_x)
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Reload => {
                if !self.loading {
                    self.notify(format!("Reloading from {}", self.org), false);
                }
                self.reload();
            }
            Action::Copy => self.copy_details(),
            Action::Filter => {
                self.tab = Tab::Subscribers;
                self.editing_filter = true;
            }
            Action::NextPane => self.cycle_pane(1),
            Action::Quit => self.quit = true,
            Action::ShowTab(tab) => {
                self.tab = tab;
                self.editing_filter = false;
            }
        }
    }

    fn cycle_pane(&mut self, step: i32) {
        if self.tab != Tab::Push {
            return;
        }
        let panes = [Pane::Requests, Pane::Jobs, Pane::Details];
        let index = panes.iter().position(|p| *p == self.pane).unwrap_or(0) as i32;
        self.pane = panes[(index + step).rem_euclid(panes.len() as i32) as usize];
    }

    fn focused_target(&self) -> Target {
        match (self.tab, self.pane) {
            (Tab::Subscribers, _) => Target::Subscribers,
            (Tab::Push, Pane::Requests) => Target::Requests,
            (Tab::Push, Pane::Jobs) => Target::Jobs,
            (Tab::Push, Pane::Details) => Target::Details,
        }
    }

    fn target_rect(&self, target: Target) -> Rect {
        match target {
            Target::Requests => self.hits.requests,
            Target::Jobs => self.hits.jobs,
            Target::Details => self.hits.details,
            Target::Filter => self.hits.filter,
            Target::Subscribers => self.hits.subscribers,
        }
    }

    fn target_at(&self, pos: Position) -> Option<Target> {
        let hits = &self.hits;
        [
            (hits.requests, Target::Requests),
            (hits.jobs, Target::Jobs),
            (hits.details, Target::Details),
            (hits.filter, Target::Filter),
            (hits.subscribers, Target::Subscribers),
        ]
        .into_iter()
        .find(|(rect, _)| rect.contains(pos))
        .map(|(_, target)| target)
    }

    fn click(&mut self, target: Target, pos: Position) {
        self.editing_filter = target == Target::Filter;
        match target {
            Target::Requests => {
                self.pane = Pane::Requests;
                if let Some(row) = row_at(self.hits.requests, self.requests.offset(), pos.y) {
                    self.select_request(row);
                }
            }
            Target::Jobs => {
                self.pane = Pane::Jobs;
                if let Some(row) = row_at(self.hits.jobs, self.jobs.offset(), pos.y) {
                    self.select_job(row);
                }
            }
            Target::Details => self.pane = Pane::Details,
            Target::Subscribers => {
                if let Some(row) = row_at(self.hits.subscribers, self.subscribers.offset(), pos.y)
                    && row < self.filtered_subscribers().len()
                {
                    self.subscribers.select(Some(row));
                }
            }
            Target::Filter => {}
        }
    }

    fn scroll(&mut self, target: Target, delta: i32) {
        match target {
            Target::Requests => {
                let len = self.data.as_ref().map_or(0, |d| d.requests.len());
                if let Some(index) = step(self.requests.selected(), len, delta) {
                    self.select_request(index);
                }
            }
            Target::Jobs => {
                if let Some(index) = step(self.jobs.selected(), self.current_jobs().len(), delta) {
                    self.select_job(index);
                }
            }
            Target::Details => {
                let next = (self.detail_scroll as i32 + delta).clamp(0, self.detail_max_scroll as i32);
                self.detail_scroll = next as u16;
            }
            Target::Subscribers => {
                let len = self.filtered_subscribers().len();
                self.subscribers
                    .select(step(self.subscribers.selected(), len, delta));
            }
            Target::Filter => {}
        }
    }

    fn select_request(&mut self, index: usize) {
        let len = self.data.as_ref().map_or(0, |d| d.requests.len());
        if index >= len {
            return;
        }
        if self.requests.selected() != Some(index) {
            self.requests.select(Some(index));
            let has_jobs = !self.current_jobs().is_empty();
            self.jobs = TableState::default().with_selected(has_jobs.then_some(0));
            self.detail_scroll = 0;
        }
    }

    fn select_job(&mut self, index: usize) {
        if index < self.current_jobs().len() && self.jobs.selected() != Some(index) {
            self.jobs.select(Some(index));
            self.detail_scroll = 0;
        }
    }

    fn clamp_subscriber_selection(&mut self) {
        let len = self.filtered_subscribers().len();
        let selected = self
            .subscribers
            .selected()
            .unwrap_or(0)
            .min(len.saturating_sub(1));
        self.subscribers.select((len > 0).then_some(selected));
    }

    fn copy_details(&mut self) {
        match self.details_text() {
            Some(text) => self.copy_text(text),
            None => self.notify("Select a job first", true),
        }
    }

    fn copy_text(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        if clipboard::copy(&text) {
            let lines = text.lines().count();
            let what = if lines > 1 {
                format!("{lines} lines")
            } else {
                format!("{} characters", text.chars().count())
            };
            self.notify(format!("Copied {what} to clipboard"), false);
            self.last_copied = Some(text);
        } else {
            self.notify("Could not copy to clipboard", true);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Requests,
    Jobs,
    Details,
    Filter,
    Subscribers,
}

/// Maps a screen row inside a bordered table with a one-line header to a row index.
fn row_at(rect: Rect, offset: usize, y: u16) -> Option<usize> {
    let first = rect.y + 2;
    (y >= first && y + 1 < rect.bottom()).then(|| offset + (y - first) as usize)
}

fn step(selected: Option<usize>, len: usize, delta: i32) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let current = selected.unwrap_or(0) as i64;
    Some((current + delta as i64).clamp(0, len as i64 - 1) as usize)
}

/// Plain-language next step for a push upgrade error.
pub fn hint(error: &PushError) -> &'static str {
    let message = error.message.to_lowercase();
    match error.kind.as_str() {
        "IneligibleUpgrade" if message.contains("not yet available") => {
            "The version has not reached this org's Salesforce instance yet. This is normal right after \
             promoting a version. Schedule the push again in a few hours."
        }
        "IneligibleUpgrade" => {
            "This org cannot receive the upgrade: the package is not installed, a beta version is installed, \
             or the org already has this or a newer version. Check the Subscribers tab."
        }
        "UnclassifiedError" => {
            "Salesforce hides the real cause. Install the version into this org by hand (sf package install) \
             to see the actual error. If that works, push again. Otherwise open a Salesforce support case \
             with the error number."
        }
        "ApexTestFailure" => {
            "An Apex test failed in the subscriber org during the upgrade. Fix the test or the code and \
             create a new package version."
        }
        _ => {
            "Read the message above. A problem in the package needs a new version. A problem in the org \
             (settings, permissions, features) can be fixed there before pushing again."
        }
    }
}
