pub mod input;
pub mod modal;
pub mod selection;
pub mod settings;
pub mod tabs;

use crate::cache::Cache;
use crate::config::Config;
use crate::sf::deploy::{self, DeployRecord, TestRun};
use crate::sf::orgs::{self, OrgInfo, OrgKind};
use crate::sf::push::{self, PackageIds, PushData, PushError, PushJob, PushRequest, Subscriber, Version};
use crate::sf::runner::{self, TaskHandle, TaskId, TaskKind, TaskSpec};
use crate::sf::versions::{self, PackageVersion, install_url};
use crate::sf::{self, Msg};
use crate::update::{self, ReleaseInfo};
use crate::{clipboard, demo};
use chrono::{DateTime, Local};
use modal::{
    Confirm, Input, InputPurpose, Modal, OrgChoice, PendingAction, PickItem, PickPurpose, Picker, Wizard,
    WizardVersion,
};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::widgets::TableState;
pub use selection::Selection;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
pub use tabs::{Hitboxes, TabId, Target};

const MIN_SPLIT: u16 = 20;
const MAX_SPLIT: u16 = 75;
const AUTO_REFRESH: Duration = Duration::from_secs(30);
const TOAST_TTL: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Reload,
    Copy,
    Filter,
    NextPane,
    Quit,
    ShowTab(TabId),
    ShowLog,
    CancelTask,
    Schedule,
    Abort,
    RetryFailed,
    OpenOrg,
    CopyOrgId,
    DeleteScratch,
    Installed,
    InstalledAll,
    CopyInstallUrl,
    CopySandboxUrl,
    Promote,
    Install,
    CreateVersion,
    Deploy,
    RunTests,
    PickDeployOrg,
    EditSetting,
    ClearCache,
    ToggleImportant,
    RenameOrg,
    ShowUpdate,
    ModalConfirm,
    ModalCancel,
    ModalBack,
    ModalRow(usize),
}

pub struct Toast {
    pub text: String,
    pub is_error: bool,
    created: Instant,
}

/// Data that is loaded in the background.
pub struct Loadable<T> {
    pub value: Option<T>,
    pub loading: bool,
    pub error: Option<String>,
    pub loaded_at: Option<DateTime<Local>>,
    /// The value was read from the cache and has not been refreshed yet.
    pub from_cache: bool,
}

impl<T> Default for Loadable<T> {
    fn default() -> Self {
        Self {
            value: None,
            loading: false,
            error: None,
            loaded_at: None,
            from_cache: false,
        }
    }
}

impl<T> Loadable<T> {
    pub fn cached(value: T, saved_at: DateTime<Local>) -> Self {
        Self {
            value: Some(value),
            loaded_at: Some(saved_at),
            from_cache: true,
            ..Self::default()
        }
    }

    fn set(&mut self, result: Result<T, String>) {
        self.loading = false;
        match result {
            Ok(value) => {
                self.value = Some(value);
                self.error = None;
                self.loaded_at = Some(Local::now());
                self.from_cache = false;
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn stale(&self, max_age: Duration) -> bool {
        self.loaded_at
            .is_none_or(|at| (Local::now() - at).to_std().unwrap_or(Duration::MAX) > max_age)
    }
}

pub enum InstalledState {
    Loading,
    Loaded(Option<String>),
    Failed(String),
}

/// A background `sf` command and everything it printed.
pub struct TaskView {
    pub id: TaskId,
    pub title: String,
    pub kind: TaskKind,
    pub argv: Vec<String>,
    pub lines: Vec<(bool, String)>,
    pub started: Instant,
    pub finished: Option<Instant>,
    pub exit: Option<i32>,
}

impl TaskView {
    pub fn running(&self) -> bool {
        self.finished.is_none()
    }

    pub fn succeeded(&self) -> bool {
        !self.running() && self.exit == Some(0)
    }

    pub fn elapsed(&self) -> Duration {
        self.finished
            .unwrap_or_else(Instant::now)
            .duration_since(self.started)
    }

    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|(_, line)| line.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub struct App {
    pub cfg: Config,
    pub demo: bool,
    /// None in demo mode and in tests.
    pub cache: Option<Cache>,
    pub demo_delay: Duration,
    pub push: Loadable<PushData>,
    pub orgs: Loadable<Vec<OrgInfo>>,
    pub versions: Loadable<Vec<PackageVersion>>,
    pub deploys: Loadable<Vec<DeployRecord>>,
    pub deploy_org: String,
    pub test_run: Option<TestRun>,
    pub installed: HashMap<String, InstalledState>,
    pub tab: TabId,
    pub focus: Target,
    pub requests: TableState,
    pub jobs: TableState,
    pub subscribers: TableState,
    pub org_rows: TableState,
    pub version_rows: TableState,
    pub deploy_rows: TableState,
    pub setting_rows: TableState,
    /// Packages of a Dev Hub, for the package picker.
    pub packages: Option<(String, Vec<PackageIds>)>,
    pending_package_picker: bool,
    /// The org picker to open once the org list has loaded.
    pending_org_picker: Option<settings::SettingKey>,
    pub detail_scroll: u16,
    pub detail_max_scroll: u16,
    pub filter: String,
    pub org_filter: String,
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
    pub modal: Option<Modal>,
    pub tasks: Vec<TaskView>,
    /// A newer sf-cockpit release, found in the background at start.
    pub update: Option<ReleaseInfo>,
    pub updating: bool,
    /// Notes of the version this binary was just updated to, shown once.
    pub whats_new: Option<ReleaseInfo>,
    handles: HashMap<TaskId, TaskHandle>,
    next_task: TaskId,
    pending_request: Option<String>,
    pending_version: Option<String>,
    pub tick: usize,
    pub quit: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl App {
    pub fn new(cfg: Config, demo: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            deploy_org: cfg.scratch_org.clone().unwrap_or_default(),
            cfg,
            demo,
            cache: None,
            demo_delay: Duration::from_millis(300),
            push: Loadable::default(),
            orgs: Loadable::default(),
            versions: Loadable::default(),
            deploys: Loadable::default(),
            test_run: None,
            installed: HashMap::new(),
            tab: TabId::Push,
            focus: Target::Requests,
            requests: TableState::default(),
            jobs: TableState::default(),
            subscribers: TableState::default(),
            org_rows: TableState::default(),
            version_rows: TableState::default(),
            deploy_rows: TableState::default(),
            setting_rows: TableState::default().with_selected(Some(0)),
            packages: None,
            pending_package_picker: false,
            pending_org_picker: None,
            detail_scroll: 0,
            detail_max_scroll: 0,
            filter: String::new(),
            org_filter: String::new(),
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
            modal: None,
            tasks: Vec::new(),
            update: None,
            updating: false,
            whats_new: None,
            handles: HashMap::new(),
            next_task: 1,
            pending_request: None,
            pending_version: None,
            tick: 0,
            quit: false,
            tx,
            rx,
        }
    }

    pub fn hub(&self) -> &str {
        &self.cfg.dev_hub
    }

    // ─── Loading ─────────────────────────────────────────────────────────────

    pub fn reload_push(&mut self) {
        if self.push.loading || self.cfg.needs_setup() {
            return;
        }
        self.push.loading = true;
        let key = self.push_key();
        if self.demo {
            let _ = self.tx.send(Msg::PushLoaded {
                key,
                result: Ok(Box::new(demo::push())),
            });
            return;
        }
        let (tx, cache, hub, limit, package) = (
            self.tx.clone(),
            self.cache.clone(),
            self.cfg.dev_hub.clone(),
            self.cfg.limit,
            self.cfg.package.clone(),
        );
        std::thread::spawn(move || {
            let result = push::load(&hub, limit, package.as_deref()).map_err(|e| format!("{e:#}"));
            if let (Ok(data), Some(cache)) = (&result, &cache) {
                let _ = cache.save(&key, data);
            }
            let _ = tx.send(Msg::PushLoaded {
                key,
                result: result.map(Box::new),
            });
        });
    }

    pub fn reload_orgs(&mut self) {
        if self.orgs.loading {
            return;
        }
        self.orgs.loading = true;
        if self.demo {
            let _ = self.tx.send(Msg::OrgsLoaded(Ok(demo::orgs())));
            return;
        }
        let (tx, cache) = (self.tx.clone(), self.cache.clone());
        std::thread::spawn(move || {
            let result = orgs::load_orgs().map_err(|e| format!("{e:#}"));
            if let (Ok(list), Some(cache)) = (&result, &cache) {
                let _ = cache.save(settings::ORGS_KEY, list);
            }
            let _ = tx.send(Msg::OrgsLoaded(result));
        });
    }

    pub fn reload_versions(&mut self) {
        if self.versions.loading || self.cfg.needs_setup() {
            return;
        }
        self.versions.loading = true;
        let key = self.versions_key();
        if self.demo {
            let _ = self.tx.send(Msg::VersionsLoaded {
                key,
                result: Ok(demo::versions()),
            });
            return;
        }
        let (tx, cache, hub, package) = (
            self.tx.clone(),
            self.cache.clone(),
            self.cfg.dev_hub.clone(),
            self.package_id(),
        );
        let configured = self.cfg.package.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let package = match package {
                    Some(id) => id,
                    None => push::resolve_package(&hub, configured.as_deref())?
                        .map(|p| p.id)
                        .ok_or_else(|| {
                            anyhow::anyhow!("no package chosen, pick one on the Settings tab (6)")
                        })?,
                };
                versions::load(&hub, &package)
            })();
            let result = result.map_err(|e| format!("{e:#}"));
            if let (Ok(list), Some(cache)) = (&result, &cache) {
                let _ = cache.save(&key, list);
            }
            let _ = tx.send(Msg::VersionsLoaded { key, result });
        });
    }

    pub fn reload_deploys(&mut self) {
        if self.deploys.loading {
            return;
        }
        if self.deploy_org.is_empty() {
            self.deploys.error =
                Some("No org chosen. Press o to pick one, or set scratch_org in sf-cockpit.toml.".into());
            return;
        }
        self.deploys.loading = true;
        let org = self.deploy_org.clone();
        if self.demo {
            let _ = self.tx.send(Msg::DeploysLoaded {
                org,
                result: Ok(demo::deploys()),
            });
            return;
        }
        let (tx, cache, key) = (self.tx.clone(), self.cache.clone(), settings::deploys_key(&org));
        std::thread::spawn(move || {
            let result = deploy::load_deploys(&org).map_err(|e| format!("{e:#}"));
            if let (Ok(list), Some(cache)) = (&result, &cache) {
                let _ = cache.save(&key, list);
            }
            let _ = tx.send(Msg::DeploysLoaded { org, result });
        });
    }

    pub fn load_installed(&mut self, username: String) {
        if matches!(self.installed.get(&username), Some(InstalledState::Loading)) {
            return;
        }
        self.installed.insert(username.clone(), InstalledState::Loading);
        if self.demo {
            let _ = self.tx.send(Msg::InstalledLoaded {
                result: Ok(demo::installed(&username)),
                username,
            });
            return;
        }
        let (tx, package) = (self.tx.clone(), self.subscriber_package_id());
        std::thread::spawn(move || {
            let result = orgs::load_installed(&username, package.as_deref()).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::InstalledLoaded { username, result });
        });
    }

    fn reload_current(&mut self) {
        match self.tab {
            TabId::Push | TabId::Subscribers => self.reload_push(),
            TabId::Orgs => self.reload_orgs(),
            TabId::Versions => self.reload_versions(),
            TabId::Deploy => self.reload_deploys(),
            TabId::Settings => self.refresh_all(),
        }
    }

    /// Picks up finished background work and expires the toast.
    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::PushLoaded { key, result } => {
                    if key != self.push_key() {
                        continue;
                    }
                    let error = result.as_ref().err().cloned();
                    match result {
                        Ok(data) => self.set_push(*data),
                        Err(_) => self.push.loading = false,
                    }
                    if let Some(error) = error {
                        self.notify(format!("Reload failed: {error}"), true);
                        self.push.error = Some(error);
                    }
                }
                Msg::OrgsLoaded(result) => {
                    self.orgs.set(result);
                    self.clamp_selection(Target::Orgs);
                    if let Some(key) = self.pending_org_picker.take()
                        && self.tab == TabId::Settings
                        && self.modal.is_none()
                    {
                        self.pick_org_setting(key);
                    }
                }
                Msg::InstalledLoaded { username, result } => {
                    let state = match result {
                        Ok(version) => InstalledState::Loaded(version),
                        Err(error) => InstalledState::Failed(error),
                    };
                    self.installed.insert(username, state);
                    self.save_installed();
                }
                Msg::VersionsLoaded { key, result } => {
                    if key != self.versions_key() {
                        continue;
                    }
                    self.versions.set(result);
                    if let Some(id) = self.pending_version.take()
                        && let Some(index) = self
                            .versions
                            .value
                            .as_ref()
                            .and_then(|v| v.iter().position(|version| version.id == id))
                    {
                        self.version_rows.select(Some(index));
                    }
                    self.clamp_selection(Target::Versions);
                }
                Msg::PackagesLoaded { hub, result } => self.packages_loaded(hub, result),
                Msg::DeploysLoaded { org, result } => {
                    if org == self.deploy_org {
                        self.deploys.set(result);
                        self.clamp_selection(Target::Deploys);
                    }
                }
                Msg::UpdateChecked(release) => self.update = release,
                Msg::UpdateInstalled(result) => {
                    self.updating = false;
                    match result {
                        Ok(version) => {
                            self.update = None;
                            self.modal = Some(Modal::Message {
                                title: "sf-cockpit updated".into(),
                                body: vec![
                                    format!("Version {version} is installed."),
                                    "Quit with q and start sf-cockpit again to use it.".into(),
                                ],
                                error: false,
                            });
                        }
                        Err(error) => self.notify(format!("Update failed: {error}"), true),
                    }
                }
                Msg::TaskLine { id, line, stderr } => {
                    // `sf org open` prints a URL with a session id, so its output is never kept.
                    if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id)
                        && task.kind != TaskKind::Open
                    {
                        task.lines.push((stderr, line));
                    }
                }
                Msg::TaskDone {
                    id,
                    exit,
                    json,
                    stdout,
                    stderr,
                } => self.task_done(id, exit, json, &stdout, &stderr),
            }
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|t| t.created.elapsed() > TOAST_TTL)
        {
            self.toast = None;
        }
        if self.auto_refresh_due() {
            self.reload_push();
        }
    }

    /// Push data refreshes itself while a request is in progress.
    pub fn auto_refresh_active(&self) -> bool {
        self.push.value.as_ref().is_some_and(PushData::has_active_request)
    }

    fn auto_refresh_due(&self) -> bool {
        self.auto_refresh_active()
            && !self.push.loading
            && !self.demo
            && self.running_task().is_none()
            && self.push.stale(AUTO_REFRESH)
    }

    pub fn set_push(&mut self, data: PushData) {
        let selected_request = self
            .pending_request
            .take()
            .or_else(|| self.selected_request().map(|r| r.id.clone()));
        let selected_job = self.selected_job().map(|j| j.id.clone());
        self.push.set(Ok(data));

        let data = self.push.value.as_ref().expect("data was just set");
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
        self.clamp_selection(Target::Subscribers);
    }

    pub fn notify(&mut self, text: impl Into<String>, is_error: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            is_error,
            created: Instant::now(),
        });
    }

    pub fn show_tab(&mut self, tab: TabId) {
        self.tab = tab;
        self.focus = tabs::spec(tab).panes[0];
        self.editing_filter = false;
        self.selection = None;
        match tab {
            TabId::Orgs if self.orgs.value.is_none() => self.reload_orgs(),
            TabId::Versions if self.versions.value.is_none() => self.reload_versions(),
            TabId::Deploy if self.deploys.value.is_none() => self.reload_deploys(),
            _ => {}
        }
    }

    // ─── Updates ─────────────────────────────────────────────────────────────

    /// Looks for a newer release in the background. Never in demo mode or tests, which have no cache.
    pub fn check_for_update(&mut self) {
        let Some(cache) = self.cache.clone() else {
            return;
        };
        if self.demo || !update::check_enabled() {
            return;
        }
        self.remember_version(&cache);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::UpdateChecked(update::check(Some(&cache))));
        });
    }

    /// After an update, offers the notes of the new version once.
    fn remember_version(&mut self, cache: &crate::cache::Cache) {
        const KEY: &str = "last-version";
        let current = update::current_version();
        let previous = cache.load::<String>(KEY).map(|(v, _)| v);
        if previous.as_deref() != Some(current) {
            let _ = cache.save(KEY, &current.to_string());
        }
        if previous.is_some_and(|p| update::is_newer(current, &p)) {
            self.whats_new = update::cached_release(cache).filter(|r| r.version == current);
            let hint = if self.whats_new.is_some() {
                " · N shows what's new"
            } else {
                ""
            };
            self.notify(format!("sf-cockpit updated to {current}{hint}"), false);
        }
    }

    fn show_update(&mut self) {
        let (release, installed) = match (&self.update, &self.whats_new) {
            (Some(release), _) => (release.clone(), false),
            (None, Some(release)) => (release.clone(), true),
            (None, None) => {
                let text = format!("sf-cockpit {} is the latest version", update::current_version());
                self.notify(text, false);
                return;
            }
        };
        self.modal = Some(Modal::Update { release, installed });
    }

    fn install_update(&mut self) {
        let Some(release) = self.update.clone() else {
            return;
        };
        if self.updating {
            return;
        }
        self.updating = true;
        self.notify(format!("Downloading sf-cockpit {}…", release.version), false);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = update::install(&release)
                .map(|_| release.version)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::UpdateInstalled(result));
        });
    }

    // ─── Derived state ───────────────────────────────────────────────────────

    pub fn package_id(&self) -> Option<String> {
        self.push
            .value
            .as_ref()
            .and_then(|d| d.package.as_ref())
            .map(|p| p.id.clone())
            .or_else(|| self.cfg.package.clone().filter(|p| p.starts_with("0Ho")))
    }

    fn subscriber_package_id(&self) -> Option<String> {
        self.push
            .value
            .as_ref()
            .and_then(|d| d.package.as_ref())
            .map(|p| p.subscriber_package_id.clone())
    }

    pub fn selected_request(&self) -> Option<&PushRequest> {
        self.request_in(self.push.value.as_ref()?)
    }

    pub fn current_jobs(&self) -> Vec<&PushJob> {
        self.push
            .value
            .as_ref()
            .map(|data| self.jobs_in(data))
            .unwrap_or_default()
    }

    pub fn selected_job(&self) -> Option<&PushJob> {
        self.job_in(self.push.value.as_ref()?)
    }

    pub fn filtered_subscribers(&self) -> Vec<&Subscriber> {
        self.push
            .value
            .as_ref()
            .map(|data| self.subscribers_in(data))
            .unwrap_or_default()
    }

    pub fn filtered_orgs(&self) -> Vec<&OrgInfo> {
        self.orgs
            .value
            .as_deref()
            .map(|orgs| self.orgs_in(orgs))
            .unwrap_or_default()
    }

    pub fn selected_org(&self) -> Option<&OrgInfo> {
        self.org_in(self.orgs.value.as_deref()?)
    }

    pub fn selected_version(&self) -> Option<&PackageVersion> {
        self.version_in(self.versions.value.as_deref()?)
    }

    pub fn selected_deploy(&self) -> Option<&DeployRecord> {
        self.deploy_in(self.deploys.value.as_deref()?)
    }

    // The `*_in` variants take the data explicitly, because rendering moves it out of the app.

    pub fn request_in<'a>(&self, data: &'a PushData) -> Option<&'a PushRequest> {
        data.requests.get(self.requests.selected()?)
    }

    pub fn jobs_in<'a>(&self, data: &'a PushData) -> Vec<&'a PushJob> {
        self.request_in(data)
            .map(|r| data.jobs_for(&r.id))
            .unwrap_or_default()
    }

    pub fn job_in<'a>(&self, data: &'a PushData) -> Option<&'a PushJob> {
        self.jobs_in(data).get(self.jobs.selected()?).copied()
    }

    pub fn subscribers_in<'a>(&self, data: &'a PushData) -> Vec<&'a Subscriber> {
        let needle = self.filter.trim().to_lowercase();
        let mut subscribers: Vec<&Subscriber> = data
            .subscribers
            .iter()
            .filter(|s| {
                if needle.is_empty() {
                    return true;
                }
                if matches!(needle.as_str(), "★" | "important") {
                    return self.cfg.is_important(&s.org_key);
                }
                let alias = data
                    .alias(&s.org_key)
                    .map(|o| o.alias.as_str())
                    .unwrap_or_default();
                let own_name = self.cfg.org_display_name(&s.org_key, "");
                [
                    s.name.as_str(),
                    &own_name,
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
        subscribers.sort_by_key(|s| {
            (
                !self.cfg.is_important(&s.org_key),
                data.is_latest(&s.version_id),
                self.cfg.org_display_name(&s.org_key, &s.name).to_lowercase(),
            )
        });
        subscribers
    }

    /// The user's name for an org from the config, else the subscriber name.
    pub fn org_label(&self, data: &PushData, org_key: &str) -> String {
        self.cfg.org_display_name(org_key, &data.org_name(org_key))
    }

    pub fn orgs_in<'a>(&self, orgs: &'a [OrgInfo]) -> Vec<&'a OrgInfo> {
        let needle = self.org_filter.trim().to_lowercase();
        orgs.iter()
            .filter(|o| {
                needle.is_empty()
                    || [
                        o.aliases.join(" ").as_str(),
                        &o.username,
                        &o.org_id,
                        &o.instance_url,
                        o.kind.label(),
                        &o.org_name,
                        &o.status,
                    ]
                    .iter()
                    .any(|field| field.to_lowercase().contains(&needle))
            })
            .collect()
    }

    pub fn org_in<'a>(&self, orgs: &'a [OrgInfo]) -> Option<&'a OrgInfo> {
        self.orgs_in(orgs).get(self.org_rows.selected()?).copied()
    }

    pub fn version_in<'a>(&self, versions: &'a [PackageVersion]) -> Option<&'a PackageVersion> {
        versions.get(self.version_rows.selected()?)
    }

    pub fn deploy_in<'a>(&self, deploys: &'a [DeployRecord]) -> Option<&'a DeployRecord> {
        deploys.get(self.deploy_rows.selected()?)
    }

    pub fn installed_label(&self, username: &str) -> Option<String> {
        Some(match self.installed.get(username)? {
            InstalledState::Loading => "loading…".into(),
            InstalledState::Loaded(Some(version)) => version.clone(),
            InstalledState::Loaded(None) => "not installed".into(),
            InstalledState::Failed(error) => format!("error: {error}"),
        })
    }

    pub fn latest_task(&self) -> Option<&TaskView> {
        self.tasks.last()
    }

    pub fn running_task(&self) -> Option<&TaskView> {
        self.tasks.iter().rev().find(|t| t.running())
    }

    pub fn task(&self, id: TaskId) -> Option<&TaskView> {
        self.tasks.iter().find(|t| t.id == id)
    }

    fn list_len(&self, target: Target) -> usize {
        match target {
            Target::Requests => self.push.value.as_ref().map_or(0, |d| d.requests.len()),
            Target::Jobs => self.current_jobs().len(),
            Target::Subscribers => self.filtered_subscribers().len(),
            Target::Orgs => self.filtered_orgs().len(),
            Target::Versions => self.versions.value.as_ref().map_or(0, Vec::len),
            Target::Deploys => self.deploys.value.as_ref().map_or(0, Vec::len),
            Target::Settings => settings::ROWS.len(),
            _ => 0,
        }
    }

    pub fn state_mut(&mut self, target: Target) -> Option<&mut TableState> {
        Some(match target {
            Target::Requests => &mut self.requests,
            Target::Jobs => &mut self.jobs,
            Target::Subscribers => &mut self.subscribers,
            Target::Orgs => &mut self.org_rows,
            Target::Versions => &mut self.version_rows,
            Target::Deploys => &mut self.deploy_rows,
            Target::Settings => &mut self.setting_rows,
            _ => return None,
        })
    }

    pub fn state(&self, target: Target) -> Option<&TableState> {
        Some(match target {
            Target::Requests => &self.requests,
            Target::Jobs => &self.jobs,
            Target::Subscribers => &self.subscribers,
            Target::Orgs => &self.org_rows,
            Target::Versions => &self.version_rows,
            Target::Deploys => &self.deploy_rows,
            Target::Settings => &self.setting_rows,
            _ => return None,
        })
    }

    /// Plain-text version of the details pane of the current tab, for the clipboard.
    pub fn details_text(&self) -> Option<String> {
        match self.tab {
            TabId::Push => {
                let data = self.push.value.as_ref()?;
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
            TabId::Subscribers => {
                let data = self.push.value.as_ref()?;
                let s = self
                    .filtered_subscribers()
                    .get(self.subscribers.selected()?)
                    .copied()?;
                Some(format!(
                    "{} [{}] {} · {} · {} · {}\n",
                    s.name,
                    s.org_key,
                    data.version_label(&s.version_id),
                    s.org_type,
                    s.org_status,
                    s.instance
                ))
            }
            TabId::Orgs => {
                let o = self.selected_org()?;
                Some(format!(
                    "{}\n{}\n{}\n{}\n",
                    o.aliases.join(", "),
                    o.username,
                    o.org_id,
                    o.instance_url
                ))
            }
            TabId::Versions => {
                let v = self.selected_version()?;
                Some(format!(
                    "{} {}\n{}\n{}\n",
                    v.version,
                    v.name,
                    v.id,
                    install_url(&v.id, false)
                ))
            }
            TabId::Deploy => {
                let d = self.selected_deploy()?;
                Some(format!(
                    "Deployment {} on {}: {}\nComponents {}/{} ({} errors), tests {}/{} ({} errors)\n{}\n",
                    d.id,
                    self.deploy_org,
                    d.status,
                    d.components_deployed,
                    d.components_total,
                    d.component_errors,
                    d.tests_completed,
                    d.tests_total,
                    d.test_errors,
                    d.error_message
                ))
            }
            TabId::Settings => {
                let key = settings::ROWS[self.setting_rows.selected()?];
                Some(self.setting_value(key))
            }
        }
    }

    // ─── Actions ─────────────────────────────────────────────────────────────

    pub fn run(&mut self, action: Action) {
        match action {
            Action::Reload => {
                if !self.push.loading {
                    self.notify(format!("Reloading from {}", self.hub()), false);
                }
                self.reload_current();
            }
            Action::Copy => self.copy_details(),
            Action::Filter => {
                if self.tab != TabId::Orgs {
                    self.show_tab(TabId::Subscribers);
                }
                self.editing_filter = true;
            }
            Action::NextPane => self.cycle_pane(1),
            Action::Quit => self.quit = true,
            Action::ShowTab(tab) => self.show_tab(tab),
            Action::ShowLog => match self.latest_task() {
                Some(task) => {
                    self.modal = Some(Modal::TaskLog {
                        id: task.id,
                        scroll: 0,
                    })
                }
                None => self.notify("No command has run yet", true),
            },
            Action::CancelTask => {
                if let Some(id) = self.running_task().map(|t| t.id) {
                    self.cancel_task(id);
                }
            }
            Action::Schedule => self.open_wizard(false),
            Action::Abort => self.confirm_abort(),
            Action::RetryFailed => self.open_wizard(true),
            Action::OpenOrg => self.open_org(),
            Action::CopyOrgId => match self.selected_org().map(|o| o.org_id.clone()) {
                Some(id) => self.copy_text(id),
                None => self.notify("Select an org first", true),
            },
            Action::DeleteScratch => self.confirm_delete_scratch(),
            Action::Installed => match self.selected_org().map(|o| o.username.clone()) {
                Some(username) => self.load_installed(username),
                None => self.notify("Select an org first", true),
            },
            Action::InstalledAll => {
                let usernames: Vec<String> = self
                    .filtered_orgs()
                    .iter()
                    .filter(|o| !o.expired)
                    .map(|o| o.username.clone())
                    .collect();
                for username in usernames {
                    self.load_installed(username);
                }
            }
            Action::CopyInstallUrl => self.copy_version_url(false),
            Action::CopySandboxUrl => self.copy_version_url(true),
            Action::Promote => self.confirm_promote(),
            Action::Install => self.pick_install_org(),
            Action::CreateVersion => self.confirm_create_version(),
            Action::Deploy => self.confirm_deploy(),
            Action::RunTests => self.ask_test_classes(),
            Action::PickDeployOrg => self.pick_deploy_org(),
            Action::EditSetting => self.edit_setting(),
            Action::ClearCache => self.clear_cache(),
            Action::ShowUpdate => self.show_update(),
            Action::ToggleImportant => self.toggle_important(),
            Action::RenameOrg => self.rename_org(),
            Action::ModalConfirm | Action::ModalCancel | Action::ModalBack | Action::ModalRow(_) => {
                self.modal_action(action)
            }
        }
    }

    fn open_wizard(&mut self, retry: bool) {
        match self.build_wizard(retry) {
            Ok(wizard) => self.modal = Some(Modal::Wizard(wizard)),
            Err(message) => self.notify(message, true),
        }
    }

    fn build_wizard(&self, retry: bool) -> Result<Wizard, String> {
        let data = self.push.value.as_ref().ok_or("Push data is not loaded yet")?;
        let versions: Vec<WizardVersion> = data
            .released_versions()
            .into_iter()
            .map(|(id, v)| WizardVersion {
                id: id.to_string(),
                label: v.label(),
                name: v.name.clone(),
                key: v.key(),
            })
            .collect();
        if versions.is_empty() {
            return Err("No released version to push".into());
        }
        let mut orgs: Vec<OrgChoice> = data
            .subscribers
            .iter()
            .map(|s| OrgChoice {
                key: s.org_key.clone(),
                name: self.cfg.org_display_name(&s.org_key, &s.name),
                important: self.cfg.is_important(&s.org_key),
                org_type: s.org_type.clone(),
                installed: data.version_label(&s.version_id),
                installed_key: data.versions.get(&s.version_id).map(Version::key),
                checked: false,
                warn: None,
            })
            .collect();
        orgs.sort_by_key(|o| (!o.important, o.name.to_lowercase()));

        let (version, preselected, retry_of) = if retry {
            let request = self.selected_request().ok_or("Select a push request first")?;
            let failed: Vec<String> = data
                .jobs_for(&request.id)
                .into_iter()
                .filter(|j| j.status == "Failed")
                .map(|j| j.org_key.clone())
                .collect();
            if failed.is_empty() {
                return Err("This request has no failed jobs".into());
            }
            let version = versions
                .iter()
                .position(|v| v.id == request.version_id)
                .unwrap_or(0);
            (version, Some(failed), Some(request.id.clone()))
        } else {
            (0, None, None)
        };
        Ok(Wizard::new(
            self.cfg.dev_hub.clone(),
            versions,
            version,
            orgs,
            preselected,
            retry_of,
        ))
    }

    fn confirm_abort(&mut self) {
        let Some(request) = self.selected_request() else {
            self.notify("Select a push request first", true);
            return;
        };
        let (id, status, can_abort) = (request.id.clone(), request.status.clone(), request.can_abort());
        let label = self
            .push
            .value
            .as_ref()
            .map(|d| d.version_label(&request.version_id))
            .unwrap_or_default();
        if !can_abort {
            self.notify(
                format!("Only Created or Pending requests can be aborted, this one is {status}"),
                true,
            );
            return;
        }
        let argv = push::build_abort(self.hub(), &id);
        self.modal = Some(Modal::Confirm(Confirm {
            title: "Abort push upgrade".into(),
            body: vec![format!("Abort the push of {label}?"), format!("Request {id}")],
            argv,
            danger: true,
            action: PendingAction::AbortRequest(id),
        }));
    }

    fn open_org(&mut self) {
        let Some(org) = self.selected_org() else {
            self.notify("Select an org first", true);
            return;
        };
        let (username, alias, expired) = (org.username.clone(), org.alias().to_string(), org.expired);
        if expired {
            self.notify("This scratch org has expired", true);
            return;
        }
        self.start_task(TaskSpec {
            title: format!("Open {alias}"),
            argv: orgs::build_open(&username),
            cwd: None,
            kind: TaskKind::Open,
            parse_json: false,
        });
    }

    fn confirm_delete_scratch(&mut self) {
        let Some(org) = self.selected_org() else {
            self.notify("Select an org first", true);
            return;
        };
        let (username, alias, kind) = (org.username.clone(), org.alias().to_string(), org.kind);
        if kind != OrgKind::Scratch {
            self.notify("Only scratch orgs can be deleted here", true);
            return;
        }
        self.modal = Some(Modal::Confirm(Confirm {
            title: "Delete scratch org".into(),
            body: vec![
                format!("Delete the scratch org {alias}?"),
                username.clone(),
                "The org and its data are gone for good.".into(),
            ],
            argv: orgs::build_delete_scratch(&username),
            danger: true,
            action: PendingAction::DeleteScratch(username),
        }));
    }

    fn copy_version_url(&mut self, sandbox: bool) {
        match self.selected_version().map(|v| install_url(&v.id, sandbox)) {
            Some(url) => self.copy_text(url),
            None => self.notify("Select a version first", true),
        }
    }

    fn confirm_promote(&mut self) {
        let Some(version) = self.selected_version() else {
            self.notify("Select a version first", true);
            return;
        };
        let (id, label, released) = (version.id.clone(), version.label(), version.released);
        if released {
            self.notify("This version is already released", true);
            return;
        }
        self.modal = Some(Modal::Confirm(Confirm {
            title: "Promote package version".into(),
            body: vec![
                format!("Release {label}?"),
                "Promotion cannot be undone. Subscribers can install and push upgrades can target it.".into(),
            ],
            argv: versions::build_promote(self.hub(), &id),
            danger: true,
            action: PendingAction::Promote {
                version_id: id,
                label,
            },
        }));
    }

    fn pick_install_org(&mut self) {
        let Some(version) = self.selected_version() else {
            self.notify("Select a version first", true);
            return;
        };
        let (version_id, label) = (version.id.clone(), version.label());
        let items = self.org_pick_items();
        if items.is_empty() {
            self.notify("No orgs known yet, open the Orgs tab first", true);
            return;
        }
        let cursor = self.default_org_index(&items);
        self.modal = Some(Modal::Picker(Picker {
            title: format!("Install {label} into"),
            items,
            cursor,
            purpose: PickPurpose::InstallOrg { version_id, label },
        }));
    }

    fn pick_deploy_org(&mut self) {
        let items = self.org_pick_items();
        if items.is_empty() {
            self.notify("No orgs known yet, open the Orgs tab first", true);
            return;
        }
        let cursor = items
            .iter()
            .position(|i| i.value == self.deploy_org || i.label == self.deploy_org)
            .unwrap_or_else(|| self.default_org_index(&items));
        self.modal = Some(Modal::Picker(Picker {
            title: "Deployments and tests on".into(),
            items,
            cursor,
            purpose: PickPurpose::DeployOrg,
        }));
    }

    fn org_pick_items(&mut self) -> Vec<PickItem> {
        if self.orgs.value.is_none() {
            self.reload_orgs();
        }
        self.orgs
            .value
            .iter()
            .flatten()
            .filter(|o| !o.expired)
            .map(|o| PickItem {
                label: o.alias().to_string(),
                detail: format!("{} · {}", o.kind.label(), o.username),
                value: o.alias().to_string(),
            })
            .collect()
    }

    fn default_org_index(&self, items: &[PickItem]) -> usize {
        self.cfg
            .scratch_org
            .as_ref()
            .and_then(|scratch| items.iter().position(|i| &i.value == scratch))
            .unwrap_or(0)
    }

    fn is_scratch_org(&self, org: &str) -> bool {
        self.cfg.scratch_org.as_deref() == Some(org)
    }

    fn confirm_install(&mut self, version_id: String, label: String, org: String) {
        let danger = !self.is_scratch_org(&org);
        let mut body = vec![format!("Install {label} into {org}?")];
        if danger {
            body.push("This is not the scratch org. Installing changes that org for its users.".into());
        }
        self.modal = Some(Modal::Confirm(Confirm {
            title: "Install package version".into(),
            body,
            argv: versions::build_install(&version_id, &org),
            danger,
            action: PendingAction::Install {
                version_id,
                label,
                org,
            },
        }));
    }

    fn confirm_create_version(&mut self) {
        let Some(package) = self.package_id() else {
            self.notify("No package configured, set package in sf-cockpit.toml", true);
            return;
        };
        if self.cfg.project_dir.is_none() {
            self.notify(
                "No project directory found, set project_dir in sf-cockpit.toml",
                true,
            );
            return;
        }
        self.modal = Some(Modal::Confirm(Confirm {
            title: "Create package version".into(),
            body: vec![
                "Create a new beta version with code coverage?".into(),
                format!(
                    "Runs in {}. This takes a while and is not free: every version counts against your daily limit.",
                    self.cfg
                        .project_dir
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                ),
            ],
            argv: versions::build_create(
                self.hub(),
                &package,
                &self.cfg.definition_file,
                self.cfg.skip_ancestor_check,
            ),
            danger: false,
            action: PendingAction::CreateVersion,
        }));
    }

    fn confirm_deploy(&mut self) {
        if self.deploy_org.is_empty() {
            self.pick_deploy_org();
            return;
        }
        if self.cfg.project_dir.is_none() {
            self.notify(
                "No project directory found, set project_dir in sf-cockpit.toml",
                true,
            );
            return;
        }
        let org = self.deploy_org.clone();
        let danger = !self.is_scratch_org(&org);
        let mut body = vec![format!("Deploy {} to {org}?", self.cfg.source_dir)];
        if danger {
            body.push("This is not the scratch org. Deploying changes that org for its users.".into());
        }
        self.modal = Some(Modal::Confirm(Confirm {
            title: "Deploy".into(),
            body,
            argv: deploy::build_deploy(&self.cfg.source_dir, &org),
            danger,
            action: PendingAction::Deploy(org),
        }));
    }

    fn ask_test_classes(&mut self) {
        if self.deploy_org.is_empty() {
            self.pick_deploy_org();
            return;
        }
        self.modal = Some(Modal::Input(Input::new(
            format!("Run Apex tests on {}", self.deploy_org),
            "Test classes, comma separated. Empty runs all local tests.".into(),
            String::new(),
            InputPurpose::TestClasses {
                org: self.deploy_org.clone(),
            },
        )));
    }

    /// Runs the command behind a confirmed dialog.
    pub fn execute(&mut self, action: PendingAction) {
        let hub = self.cfg.dev_hub.clone();
        let project = self.cfg.project_dir.clone();
        let spec = match action {
            PendingAction::SchedulePush(spec) => TaskSpec {
                title: format!(
                    "Schedule push of {} to {} orgs",
                    spec.version_label,
                    spec.org_keys.len()
                ),
                argv: push::build_schedule(
                    &hub,
                    &spec.version_id,
                    &spec.org_keys,
                    spec.start_time.as_deref(),
                ),
                cwd: project,
                kind: TaskKind::Schedule,
                parse_json: true,
            },
            PendingAction::AbortRequest(id) => TaskSpec {
                title: format!("Abort push request {id}"),
                argv: push::build_abort(&hub, &id),
                cwd: project,
                kind: TaskKind::Abort,
                parse_json: true,
            },
            PendingAction::Promote { version_id, label } => TaskSpec {
                title: format!("Promote {label}"),
                argv: versions::build_promote(&hub, &version_id),
                cwd: project,
                kind: TaskKind::Promote,
                parse_json: true,
            },
            PendingAction::Install {
                version_id,
                label,
                org,
            } => TaskSpec {
                title: format!("Install {label} into {org}"),
                argv: versions::build_install(&version_id, &org),
                cwd: project,
                kind: TaskKind::Install,
                parse_json: true,
            },
            PendingAction::CreateVersion => {
                let Some(package) = self.package_id() else {
                    return;
                };
                TaskSpec {
                    title: "Create package version".into(),
                    argv: versions::build_create(
                        &hub,
                        &package,
                        &self.cfg.definition_file,
                        self.cfg.skip_ancestor_check,
                    ),
                    cwd: project,
                    kind: TaskKind::CreateVersion,
                    parse_json: true,
                }
            }
            PendingAction::DeleteScratch(org) => TaskSpec {
                title: format!("Delete scratch org {org}"),
                argv: orgs::build_delete_scratch(&org),
                cwd: None,
                kind: TaskKind::DeleteScratch,
                parse_json: true,
            },
            PendingAction::Deploy(org) => TaskSpec {
                title: format!("Deploy {} to {org}", self.cfg.source_dir),
                argv: deploy::build_deploy(&self.cfg.source_dir, &org),
                cwd: project,
                kind: TaskKind::Deploy,
                parse_json: false,
            },
            PendingAction::RunTests { org, classes } => TaskSpec {
                title: if classes.is_empty() {
                    format!("Run all tests on {org}")
                } else {
                    format!("Run {} on {org}", classes.join(", "))
                },
                argv: deploy::build_tests(&org, &classes),
                cwd: project,
                kind: TaskKind::Tests,
                parse_json: true,
            },
        };
        self.start_task(spec);
    }

    #[cfg(test)]
    pub fn pending_argv(&self) -> Option<Vec<String>> {
        match self.modal.as_ref()? {
            Modal::Confirm(c) => Some(c.argv.clone()),
            Modal::Wizard(w) => Some(w.argv()),
            _ => None,
        }
    }

    // ─── Tasks ───────────────────────────────────────────────────────────────

    fn start_task(&mut self, spec: TaskSpec) {
        let id = self.next_task;
        self.next_task += 1;
        let handle = if self.demo {
            Ok(runner::spawn_demo(
                id,
                demo::task_lines(spec.kind),
                demo::task_result(spec.kind),
                self.demo_delay,
                self.tx.clone(),
            ))
        } else {
            runner::spawn(id, &spec, self.tx.clone())
        };
        match handle {
            Ok(handle) => {
                self.handles.insert(id, handle);
                self.tasks.push(TaskView {
                    id,
                    title: spec.title.clone(),
                    kind: spec.kind,
                    argv: spec.argv,
                    lines: Vec::new(),
                    started: Instant::now(),
                    finished: None,
                    exit: None,
                });
                self.modal = spec.kind.shows_log().then_some(Modal::TaskLog { id, scroll: 0 });
                self.notify(format!("Started: {}", spec.title), false);
            }
            Err(error) => self.notify(format!("Could not start sf: {error:#}"), true),
        }
    }

    pub fn cancel_task(&mut self, id: TaskId) {
        match self.handles.get(&id) {
            Some(handle) => {
                handle.cancel();
                self.notify("Cancelling…", false);
            }
            None => self.notify("This command has already finished", true),
        }
    }

    fn task_done(&mut self, id: TaskId, exit: Option<i32>, json: Option<Value>, stdout: &str, stderr: &str) {
        self.handles.remove(&id);
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else {
            return;
        };
        task.finished = Some(Instant::now());
        task.exit = exit;
        if task.kind != TaskKind::Open
            && let Some(result) = json.as_ref().map(|j| &j["result"]).filter(|r| !r.is_null())
            && let Ok(pretty) = serde_json::to_string_pretty(result)
        {
            task.lines
                .extend(pretty.lines().take(300).map(|l| (false, l.to_string())));
        }
        let (kind, title) = (task.kind, task.title.clone());

        if exit != Some(0) {
            let message = json
                .as_ref()
                .map(sf::error_message)
                .or_else(|| last_line(stderr))
                .or_else(|| last_line(stdout))
                .unwrap_or_else(|| match exit {
                    Some(code) => format!("exit code {code}"),
                    None => "cancelled".into(),
                });
            self.notify(format!("{title} failed: {message}"), true);
            if kind == TaskKind::Schedule {
                self.reload_push();
                self.modal = Some(Modal::Message {
                    title: "Push upgrade not scheduled".into(),
                    body: vec![
                        message,
                        String::new(),
                        "If Salesforce rejected some orgs, the request may still exist with status Created. \
                         Select it, abort it with a, and schedule again without those orgs. The rejected orgs \
                         are listed in job_errors/ in the project directory."
                            .into(),
                    ],
                    error: true,
                });
            }
            return;
        }

        match kind {
            TaskKind::Schedule => {
                let request = json.as_ref().and_then(push::scheduled_request_id);
                self.notify(
                    format!("Scheduled push request {}", request.clone().unwrap_or_default()),
                    false,
                );
                self.pending_request = request;
                self.modal = None;
                self.show_tab(TabId::Push);
                self.reload_push();
            }
            TaskKind::Abort => {
                self.notify("Push request aborted", false);
                self.modal = None;
                self.reload_push();
            }
            TaskKind::Promote => {
                self.notify(format!("{title} done"), false);
                self.modal = None;
                self.reload_versions();
                self.reload_push();
            }
            TaskKind::Install => self.notify(format!("{title} done"), false),
            TaskKind::CreateVersion => {
                let version = json.as_ref().and_then(versions::created_version_id);
                self.notify(
                    format!("Created version {}", version.clone().unwrap_or_default()),
                    false,
                );
                self.pending_version = version;
                self.reload_versions();
            }
            TaskKind::DeleteScratch => {
                self.notify("Scratch org deleted", false);
                self.modal = None;
                self.reload_orgs();
            }
            TaskKind::Deploy => {
                self.notify(format!("{title} done"), false);
                self.reload_deploys();
            }
            TaskKind::Tests => {
                if let Some(json) = &json {
                    let run = deploy::parse_test_result(json, &self.deploy_org);
                    self.notify(
                        format!(
                            "Tests {}: {} passed, {} failed, coverage {}",
                            run.outcome, run.passing, run.failing, run.run_coverage
                        ),
                        run.failing > 0,
                    );
                    self.test_run = Some(run);
                    self.modal = None;
                    self.focus = Target::DeployDetails;
                    self.detail_scroll = 0;
                }
            }
            TaskKind::Open => {}
        }
    }

    // ─── Selection and clipboard ─────────────────────────────────────────────

    pub fn select(&mut self, target: Target, index: usize) {
        let len = self.list_len(target);
        if index >= len {
            return;
        }
        let Some(state) = self.state_mut(target) else {
            return;
        };
        if state.selected() != Some(index) {
            state.select(Some(index));
            self.after_select(target);
        }
    }

    fn after_select(&mut self, target: Target) {
        self.detail_scroll = 0;
        if target == Target::Requests {
            let has_jobs = !self.current_jobs().is_empty();
            self.jobs = TableState::default().with_selected(has_jobs.then_some(0));
        }
    }

    pub fn clamp_selection(&mut self, target: Target) {
        let len = self.list_len(target);
        if let Some(state) = self.state_mut(target) {
            let selected = state.selected().unwrap_or(0).min(len.saturating_sub(1));
            state.select((len > 0).then_some(selected));
        }
    }

    pub fn scroll(&mut self, target: Target, delta: i32) {
        if target.is_details() {
            let next = (self.detail_scroll as i32 + delta).clamp(0, self.detail_max_scroll as i32);
            self.detail_scroll = next as u16;
            return;
        }
        let len = self.list_len(target);
        let selected = self.state(target).and_then(TableState::selected);
        if let Some(index) = step(selected, len, delta) {
            self.select(target, index);
        }
    }

    pub fn cycle_pane(&mut self, step: i32) {
        let panes = tabs::spec(self.tab).panes;
        let index = panes.iter().position(|p| *p == self.focus).unwrap_or(0) as i32;
        self.focus = panes[(index + step).rem_euclid(panes.len() as i32) as usize];
    }

    pub fn filter_mut(&mut self) -> &mut String {
        if self.tab == TabId::Orgs {
            &mut self.org_filter
        } else {
            &mut self.filter
        }
    }

    pub fn current_filter(&self) -> &str {
        if self.tab == TabId::Orgs {
            &self.org_filter
        } else {
            &self.filter
        }
    }

    pub fn resize_split(&mut self, delta: i32) {
        self.split = (self.split as i32 + delta).clamp(MIN_SPLIT as i32, MAX_SPLIT as i32) as u16;
    }

    pub fn set_split_from_x(&mut self, x: u16) {
        let body = self.hits.body;
        if body.width > 0 {
            let offset = x.saturating_sub(body.x) as u32 * 100 / body.width as u32;
            self.split = (offset as u16).clamp(MIN_SPLIT, MAX_SPLIT);
        }
    }

    pub fn on_divider(&self, pos: Position) -> bool {
        self.tab == TabId::Push
            && self.hits.body.contains(pos)
            && (pos.x == self.hits.divider_x || pos.x + 1 == self.hits.divider_x)
    }

    pub fn press(&self) -> Option<(Position, Rect)> {
        self.press
    }

    pub fn set_press(&mut self, press: Option<(Position, Rect)>) {
        self.press = press;
    }

    fn copy_details(&mut self) {
        match self.details_text() {
            Some(text) => self.copy_text(text),
            None => self.notify("Select a row first", true),
        }
    }

    pub fn copy_text(&mut self, text: String) {
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

    // ─── Modal outcomes ──────────────────────────────────────────────────────

    pub(super) fn submit_input(&mut self, purpose: InputPurpose, value: String) {
        match purpose {
            InputPurpose::TestClasses { org } => {
                let classes: Vec<String> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
                    .collect();
                let what = if classes.is_empty() {
                    "all local tests".to_string()
                } else {
                    classes.join(", ")
                };
                self.modal = Some(Modal::Confirm(Confirm {
                    title: "Run Apex tests".into(),
                    body: vec![format!("Run {what} on {org}?")],
                    argv: deploy::build_tests(&org, &classes),
                    danger: false,
                    action: PendingAction::RunTests { org, classes },
                }));
            }
            InputPurpose::OrgName { org_key } => {
                let name = Some(value.trim().to_string()).filter(|n| !n.is_empty());
                let mut note = self.cfg.org_note(&org_key).cloned().unwrap_or_default();
                note.name = name.clone();
                let what = match &name {
                    Some(name) => format!("Named the org {name}"),
                    None => "Removed the org name".to_string(),
                };
                self.save_org_note(&org_key, note, what);
            }
            InputPurpose::Limit => match value.trim().parse::<usize>() {
                Ok(limit) if (1..=500).contains(&limit) => {
                    self.apply_setting(settings::SettingKey::Limit, limit.to_string())
                }
                _ => self.notify("Enter a number between 1 and 500", true),
            },
        }
    }

    pub(super) fn picked(&mut self, purpose: PickPurpose, item: PickItem) {
        match purpose {
            PickPurpose::InstallOrg { version_id, label } => {
                self.confirm_install(version_id, label, item.value)
            }
            PickPurpose::DeployOrg => {
                self.modal = None;
                if item.value != self.deploy_org {
                    self.switch_deploy_org(item.value);
                }
                self.reload_deploys();
            }
            PickPurpose::Setting(key) => self.apply_setting(key, item.value),
        }
    }
}

fn step(selected: Option<usize>, len: usize, delta: i32) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let current = selected.unwrap_or(0) as i64;
    Some((current + delta as i64).clamp(0, len as i64 - 1) as usize)
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(|l| sf::strip_control(l).trim().to_string())
        .rfind(|l| !l.is_empty())
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
            "Salesforce hides the real cause. Install the version into this org by hand (Versions tab, i) \
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
