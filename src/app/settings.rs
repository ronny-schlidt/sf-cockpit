//! The Settings tab, the cache keys, and loading cached data at start.

use super::modal::{Input, InputPurpose, Modal, PickItem, PickPurpose, Picker};
use super::{App, InstalledState, Loadable, TabId};
use crate::cache::Cache;
use crate::config::{self, Origin, tilde};
use crate::demo;
use crate::sf::Msg;
use crate::sf::orgs::OrgKind;
use crate::sf::push::{self, PackageIds};
use crate::update;
use ratatui::widgets::TableState;
use std::collections::HashMap;

pub const ORGS_KEY: &str = "orgs";
const INSTALLED_KEY: &str = "installed";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingKey {
    DevHub,
    Package,
    ScratchOrg,
    Limit,
    ProjectDir,
    SavePath,
    Cache,
    Version,
}

pub const ROWS: [SettingKey; 8] = [
    SettingKey::DevHub,
    SettingKey::Package,
    SettingKey::ScratchOrg,
    SettingKey::Limit,
    SettingKey::ProjectDir,
    SettingKey::SavePath,
    SettingKey::Cache,
    SettingKey::Version,
];

impl SettingKey {
    pub fn label(self) -> &'static str {
        match self {
            SettingKey::DevHub => "Dev Hub",
            SettingKey::Package => "Package",
            SettingKey::ScratchOrg => "Scratch org",
            SettingKey::Limit => "Push requests to load",
            SettingKey::ProjectDir => "Project directory",
            SettingKey::SavePath => "Changes are saved to",
            SettingKey::Cache => "Cache",
            SettingKey::Version => "sf-cockpit version",
        }
    }

    /// The key in the config file; None for rows that cannot be changed here.
    pub fn file_key(self) -> Option<&'static str> {
        match self {
            SettingKey::DevHub => Some("dev_hub"),
            SettingKey::Package => Some("package"),
            SettingKey::ScratchOrg => Some("scratch_org"),
            SettingKey::Limit => Some("limit"),
            _ => None,
        }
    }
}

pub fn deploys_key(org: &str) -> String {
    Cache::key(&["deploys", org])
}

impl App {
    pub fn push_key(&self) -> String {
        Cache::key(&[
            "push",
            &self.cfg.dev_hub,
            self.cfg.package.as_deref().unwrap_or_default(),
        ])
    }

    pub fn versions_key(&self) -> String {
        Cache::key(&[
            "versions",
            &self.cfg.dev_hub,
            self.cfg.package.as_deref().unwrap_or_default(),
        ])
    }

    /// The requested tab, or Settings when something the app needs is missing.
    pub fn open_first_tab(&mut self, requested: TabId) {
        let missing = self.missing_settings();
        let Some(first) = missing.first().copied() else {
            self.show_tab(requested);
            return;
        };
        self.show_tab(TabId::Settings);
        self.setting_rows
            .select(ROWS.iter().position(|row| *row == first));
        self.modal = Some(Modal::Message {
            title: "Setup incomplete".into(),
            body: self.setup_message(&missing),
            error: true,
        });
    }

    /// Settings the app cannot work well without, most important first.
    pub fn missing_settings(&self) -> Vec<SettingKey> {
        let mut missing = Vec::new();
        if self.cfg.needs_setup() {
            missing.push(SettingKey::DevHub);
        }
        if self.cfg.package.is_none() {
            missing.push(SettingKey::Package);
        }
        if self.cfg.scratch_org.is_none() {
            missing.push(SettingKey::ScratchOrg);
        }
        missing
    }

    /// Lines starting with `# ` are headings in the message dialog.
    fn setup_message(&self, missing: &[SettingKey]) -> Vec<String> {
        let mut body = Vec::new();
        if self.cfg.project_dir.is_none() {
            let cwd = std::env::current_dir()
                .map(|dir| tilde(&dir))
                .unwrap_or_else(|_| "this folder".into());
            body.push("# You are not inside a Salesforce project".into());
            body.push(format!(
                "sf-cockpit was started in {cwd}. There is no sf-cockpit.toml or sfdx-project.json in this \
                 folder or above it."
            ));
            body.push(
                "If you meant to work on a project, quit with q and start sf-cockpit in its folder. The \
                 project's settings are then used automatically."
                    .into(),
            );
            if let Some(path) = &self.cfg.save_path {
                body.push(format!(
                    "Otherwise the settings you choose now are saved globally to {}.",
                    tilde(path)
                ));
            }
            body.push(String::new());
        }
        body.push("# Missing settings".into());
        for key in missing {
            body.push(format!(
                "  • {}: {}",
                key.label(),
                match key {
                    SettingKey::DevHub => "the org that owns your package. Nothing can be loaded without it.",
                    SettingKey::Package => "which package to show, if the Dev Hub owns more than one.",
                    SettingKey::ScratchOrg => "the default org for deploys, Apex tests and installs.",
                    _ => "",
                }
            ));
        }
        body.push(String::new());
        body.push("Close this with Enter, then choose them on the Settings tab.".into());
        body
    }

    /// Problems with the current settings, most important first, for the Settings tab.
    pub fn setup_problems(&self) -> Vec<(bool, String)> {
        let mut problems = Vec::new();
        let orgs = self.orgs.value.as_deref();
        if self.cfg.needs_setup() {
            problems.push((
                true,
                "No Dev Hub chosen. Select Dev Hub and press Enter.".to_string(),
            ));
            match orgs {
                Some(list) if list.iter().all(|o| o.expired) => problems.push((
                    true,
                    "The sf CLI knows no orgs yet. Quit, log in with `sf org login web --alias DevHub \
                     --set-default-dev-hub`, and start sf-cockpit again."
                        .into(),
                )),
                Some(list) if !list.iter().any(|o| o.kind == OrgKind::DevHub) => problems.push((
                    true,
                    "None of your orgs is marked as a Dev Hub. Log in to the org that owns the package with \
                     `sf org login web --alias DevHub --set-default-dev-hub`."
                        .into(),
                )),
                _ => {}
            }
        } else if self.cfg.package.is_none() {
            problems.push((
                false,
                "No package chosen. If the Dev Hub owns more than one package, choose yours.".into(),
            ));
        }
        if self.cfg.project_dir.is_none() {
            problems.push((
                false,
                "Not started inside a Salesforce project, so deploy, tests and new versions are off and \
                 settings go to the global file. Quit and start sf-cockpit in the project folder \
                 (where sfdx-project.json is)."
                    .into(),
            ));
        }
        if !self.cfg.needs_setup() && self.cfg.scratch_org.is_none() {
            problems.push((
                false,
                "No scratch org chosen. Deploy & Test and installs need a default org.".into(),
            ));
        }
        problems
    }

    /// Shows cached data for every tab at once, then refreshes all of it in the background.
    pub fn start(&mut self) {
        if let Some(cache) = self.cache.clone() {
            if let Some((data, saved_at)) = cache.load(&self.push_key()) {
                self.set_push(data);
                self.mark_cached_push(saved_at);
            }
            if let Some((list, saved_at)) = cache.load(ORGS_KEY) {
                self.orgs = Loadable::cached(list, saved_at);
            }
            if let Some((list, saved_at)) = cache.load(&self.versions_key()) {
                self.versions = Loadable::cached(list, saved_at);
            }
            if let Some((list, saved_at)) = cache.load(&deploys_key(&self.deploy_org)) {
                self.deploys = Loadable::cached(list, saved_at);
            }
            if let Some((installed, _)) = cache.load::<HashMap<String, Option<String>>>(INSTALLED_KEY) {
                self.installed = installed
                    .into_iter()
                    .map(|(user, version)| (user, InstalledState::Loaded(version)))
                    .collect();
            }
            for target in [
                super::Target::Orgs,
                super::Target::Versions,
                super::Target::Deploys,
            ] {
                self.clamp_selection(target);
            }
        }
        self.refresh_all();
    }

    fn mark_cached_push(&mut self, saved_at: chrono::DateTime<chrono::Local>) {
        self.push.from_cache = true;
        self.push.loaded_at = Some(saved_at);
    }

    pub fn refresh_all(&mut self) {
        self.reload_push();
        self.reload_orgs();
        self.reload_versions();
        if !self.deploy_org.is_empty() {
            self.reload_deploys();
        }
    }

    pub(super) fn save_installed(&self) {
        let Some(cache) = &self.cache else {
            return;
        };
        let known: HashMap<&str, &Option<String>> = self
            .installed
            .iter()
            .filter_map(|(user, state)| match state {
                InstalledState::Loaded(version) => Some((user.as_str(), version)),
                _ => None,
            })
            .collect();
        let _ = cache.save(INSTALLED_KEY, &known);
    }

    pub(super) fn switch_deploy_org(&mut self, org: String) {
        self.deploys = self
            .cache
            .as_ref()
            .and_then(|cache| cache.load(&deploys_key(&org)))
            .map(|(list, saved_at)| Loadable::cached(list, saved_at))
            .unwrap_or_default();
        self.deploy_org = org;
        self.test_run = None;
        self.deploy_rows = TableState::default();
        self.clamp_selection(super::Target::Deploys);
    }

    pub(super) fn clear_cache(&mut self) {
        match self.cache.as_ref().map(Cache::clear) {
            Some(Ok(())) => self.notify("Cache cleared. It fills again with the next refresh.", false),
            Some(Err(error)) => self.notify(format!("Could not clear the cache: {error:#}"), true),
            None => self.notify("No cache in demo mode", true),
        }
    }

    // ─── Values shown in the tab ─────────────────────────────────────────────

    pub fn setting_value(&self, key: SettingKey) -> String {
        match key {
            SettingKey::DevHub if self.cfg.needs_setup() => "not set".into(),
            SettingKey::DevHub => self.cfg.dev_hub.clone(),
            SettingKey::Package => self
                .cfg
                .package
                .clone()
                .unwrap_or_else(|| "not set, the Dev Hub's only package is used".into()),
            SettingKey::ScratchOrg => self.cfg.scratch_org.clone().unwrap_or_else(|| "not set".into()),
            SettingKey::Limit => self.cfg.limit.to_string(),
            SettingKey::ProjectDir => self
                .cfg
                .project_dir
                .as_deref()
                .map(tilde)
                .unwrap_or_else(|| "not found, deploy and new versions need one".into()),
            SettingKey::SavePath => self
                .cfg
                .save_path
                .as_deref()
                .map(tilde)
                .unwrap_or_else(|| "nowhere, demo mode".into()),
            SettingKey::Cache => match &self.cache {
                Some(cache) => {
                    let (count, bytes) = cache.summary();
                    format!(
                        "{} · {count} entries, {} KB",
                        tilde(cache.dir()),
                        bytes.div_ceil(1024)
                    )
                }
                None => "off in demo mode".into(),
            },
            SettingKey::Version => match &self.update {
                Some(release) => format!("{} · {} available", update::current_version(), release.version),
                None => update::current_version().into(),
            },
        }
    }

    pub fn setting_source(&self, key: SettingKey) -> String {
        match key {
            SettingKey::ProjectDir => self.cfg.origin("project_dir").label(),
            SettingKey::SavePath => match self.cfg.save_origin() {
                Some(Origin::Project(_)) => "project file, found next to sfdx-project.json".into(),
                Some(_) => "global file, no sf-cockpit.toml in this directory or above".into(),
                None => String::new(),
            },
            SettingKey::Cache => "C clears it".into(),
            SettingKey::Version if self.update.is_some() => "N or Enter updates".into(),
            SettingKey::Version => "N shows what's new".into(),
            _ => key
                .file_key()
                .map(|file_key| self.cfg.origin(file_key).label())
                .unwrap_or_default(),
        }
    }

    // ─── Editing ─────────────────────────────────────────────────────────────

    pub(super) fn edit_setting(&mut self) {
        let Some(key) = self.setting_rows.selected().and_then(|i| ROWS.get(i)).copied() else {
            return;
        };
        match key {
            SettingKey::DevHub | SettingKey::ScratchOrg => self.pick_org_setting(key),
            SettingKey::Package => self.pick_package(),
            SettingKey::Limit => {
                self.modal = Some(Modal::Input(Input::new(
                    "Push requests to load".into(),
                    "How many recent push requests should the Push tab show? (1 to 500)".into(),
                    self.cfg.limit.to_string(),
                    InputPurpose::Limit,
                )))
            }
            SettingKey::Cache => self.clear_cache(),
            SettingKey::Version => self.show_update(),
            SettingKey::ProjectDir | SettingKey::SavePath => self.notify(
                "This follows from where sf-cockpit is started. Start it inside the Salesforce project.",
                true,
            ),
        }
    }

    pub(super) fn pick_org_setting(&mut self, key: SettingKey) {
        if self.orgs.value.is_none() {
            self.pending_org_picker = Some(key);
            self.reload_orgs();
            self.notify("Loading your orgs from the sf CLI…", false);
            return;
        }
        if self.orgs.value.iter().flatten().all(|o| o.expired) {
            self.notify(
                "The sf CLI knows no orgs. Log in first: sf org login web --alias DevHub --set-default-dev-hub",
                true,
            );
            return;
        }
        let preferred = if key == SettingKey::DevHub {
            OrgKind::DevHub
        } else {
            OrgKind::Scratch
        };
        let mut orgs: Vec<_> = self.orgs.value.iter().flatten().filter(|o| !o.expired).collect();
        orgs.sort_by_key(|o| (o.kind != preferred, o.alias().to_lowercase()));
        let items: Vec<PickItem> = orgs
            .iter()
            .map(|o| PickItem {
                label: o.alias().to_string(),
                detail: format!("{} · {} · {}", o.kind.label(), o.username, o.status),
                value: o.alias().to_string(),
            })
            .collect();
        let current = self.setting_value(key);
        let cursor = items.iter().position(|i| i.value == current).unwrap_or(0);
        self.modal = Some(Modal::Picker(Picker {
            title: format!("Choose the {}", key.label().to_lowercase()),
            items,
            cursor,
            purpose: PickPurpose::Setting(key),
        }));
    }

    fn pick_package(&mut self) {
        let hub = self.cfg.dev_hub.clone();
        let Some(packages) = self
            .packages
            .as_ref()
            .filter(|(owner, _)| *owner == hub)
            .map(|(_, list)| list.clone())
        else {
            self.pending_package_picker = true;
            self.notify(format!("Loading packages from {hub}"), false);
            if self.demo {
                let _ = self.tx.send(Msg::PackagesLoaded {
                    hub,
                    result: Ok(demo::packages()),
                });
            } else {
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let result = push::load_packages(&hub).map_err(|e| format!("{e:#}"));
                    let _ = tx.send(Msg::PackagesLoaded { hub, result });
                });
            }
            return;
        };
        if packages.is_empty() {
            self.notify(format!("{hub} owns no packages. Is it the right Dev Hub?"), true);
            return;
        }
        let current = self.cfg.package.clone().unwrap_or_default();
        let cursor = packages
            .iter()
            .position(|p| p.name == current || p.id == current)
            .unwrap_or(0);
        self.modal = Some(Modal::Picker(Picker {
            title: format!("Choose the package of {hub}"),
            items: packages
                .iter()
                .map(|p| PickItem {
                    label: p.name.clone(),
                    detail: p.id.clone(),
                    value: p.name.clone(),
                })
                .collect(),
            cursor,
            purpose: PickPurpose::Setting(SettingKey::Package),
        }));
    }

    pub(super) fn packages_loaded(&mut self, hub: String, result: Result<Vec<PackageIds>, String>) {
        match result {
            Ok(list) => {
                let open = self.pending_package_picker && hub == self.cfg.dev_hub && self.modal.is_none();
                self.packages = Some((hub, list));
                self.pending_package_picker = false;
                if open && self.tab == TabId::Settings {
                    self.pick_package();
                }
            }
            Err(error) => {
                self.pending_package_picker = false;
                self.notify(format!("Could not load packages: {error}"), true);
            }
        }
    }

    /// Saves a changed setting to the active config file and reloads what depends on it.
    pub(super) fn apply_setting(&mut self, key: SettingKey, value: String) {
        let Some(file_key) = key.file_key() else {
            return;
        };
        if value == self.setting_value(key) {
            return;
        }
        let saved_to = match self.cfg.save_path.clone() {
            Some(path) => {
                let result = match key {
                    SettingKey::Limit => {
                        config::save_value(&path, file_key, value.parse::<i64>().unwrap_or(30))
                    }
                    _ => config::save_value(&path, file_key, value.as_str()),
                };
                if let Err(error) = result {
                    self.notify(format!("Could not save: {error:#}"), true);
                    return;
                }
                Some(path)
            }
            None => None,
        };
        if self.cfg.origin(file_key) == Origin::Flag {
            self.notify(
                format!("Saved {file_key}, but the command-line flag stays in effect until you restart"),
                true,
            );
            return;
        }

        let old_scratch = self.cfg.scratch_org.clone();
        match key {
            SettingKey::DevHub => self.cfg.dev_hub = value.clone(),
            SettingKey::Package => self.cfg.package = Some(value.clone()),
            SettingKey::ScratchOrg => self.cfg.scratch_org = Some(value.clone()),
            SettingKey::Limit => self.cfg.limit = value.parse().unwrap_or(self.cfg.limit),
            _ => {}
        }
        if let Some(origin) = self.cfg.save_origin() {
            self.cfg.origins.insert(file_key, origin);
        }
        self.notify(
            match &saved_to {
                Some(path) => format!("Saved {file_key} = {value} to {}", tilde(path)),
                None => format!("Changed {file_key} to {value} (demo mode, not saved)"),
            },
            false,
        );

        match key {
            SettingKey::DevHub | SettingKey::Package => self.switch_package(),
            SettingKey::ScratchOrg => {
                if old_scratch.as_deref().is_none_or(|old| old == self.deploy_org) {
                    self.switch_deploy_org(value);
                    self.reload_deploys();
                }
            }
            SettingKey::Limit => {
                self.push.loading = false;
                self.reload_push();
            }
            _ => {}
        }
    }

    fn selected_subscriber_key(&mut self) -> Option<(String, String)> {
        let found = self
            .filtered_subscribers()
            .get(self.subscribers.selected()?)
            .map(|s| (s.org_key.clone(), s.name.clone()));
        if found.is_none() {
            self.notify("Select an org first", true);
        }
        found
    }

    pub(super) fn toggle_important(&mut self) {
        let Some((key, name)) = self.selected_subscriber_key() else {
            return;
        };
        let mut note = self.cfg.org_note(&key).cloned().unwrap_or_default();
        let important = !self.cfg.is_important(&key);
        note.important = important.then_some(true);
        let name = self.cfg.org_display_name(&key, &name);
        let what = if important {
            format!("Marked {name} as important — preselected for push upgrades")
        } else {
            format!("Unmarked {name} as important")
        };
        self.save_org_note(&key, note, what);
    }

    pub(super) fn rename_org(&mut self) {
        let Some((key, name)) = self.selected_subscriber_key() else {
            return;
        };
        let value = self
            .cfg
            .org_note(&key)
            .and_then(|n| n.name.clone())
            .unwrap_or_default();
        self.modal = Some(Modal::Input(Input::new(
            format!("Name for {name}"),
            format!("Your own name for org {key}, saved in the config file. Empty removes it."),
            value,
            InputPurpose::OrgName { org_key: key },
        )));
    }

    /// Writes an org note to the active config file and applies it at once.
    pub(super) fn save_org_note(&mut self, key: &str, note: config::OrgNote, what: String) {
        let saved_to = match self.cfg.save_path.clone() {
            Some(path) => match config::save_org_note(&path, key, &note) {
                Ok(()) => Some(path),
                Err(error) => {
                    self.notify(format!("Could not save: {error:#}"), true);
                    return;
                }
            },
            None => None,
        };
        let key = crate::sf::query::org_key(key);
        if note.is_empty() {
            self.cfg.orgs.remove(&key);
        } else {
            self.cfg.orgs.insert(key.clone(), note);
        }
        // The list is sorted by marking and name: keep the cursor on the same org.
        if let Some(row) = self.filtered_subscribers().iter().position(|s| s.org_key == key) {
            self.subscribers.select(Some(row));
        }
        self.notify(
            match saved_to {
                Some(path) => format!("{what} in {}", tilde(&path)),
                None => format!("{what} (demo mode, not saved)"),
            },
            false,
        );
    }

    /// After the Dev Hub or package changed: show that package's cached data and refresh it.
    fn switch_package(&mut self) {
        let cache = self.cache.clone();
        self.push = Loadable::default();
        self.versions = Loadable::default();
        self.requests = TableState::default();
        self.jobs = TableState::default();
        self.subscribers = TableState::default();
        self.version_rows = TableState::default();
        if let Some(cache) = cache {
            if let Some((data, saved_at)) = cache.load(&self.push_key()) {
                self.set_push(data);
                self.mark_cached_push(saved_at);
            }
            if let Some((list, saved_at)) = cache.load(&self.versions_key()) {
                self.versions = Loadable::cached(list, saved_at);
                self.clamp_selection(super::Target::Versions);
            }
        }
        self.reload_push();
        self.reload_versions();
    }
}
