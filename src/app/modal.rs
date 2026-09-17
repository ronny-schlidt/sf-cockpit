//! Dialogs: confirmations, text input, pickers, the schedule wizard and the task log.

use super::settings::SettingKey;
use crate::sf::push::{ScheduleSpec, VersionKey, build_schedule};
use crate::sf::runner::TaskId;
use chrono::NaiveDateTime;

pub enum Modal {
    Confirm(Confirm),
    Input(Input),
    Picker(Picker),
    Wizard(Wizard),
    TaskLog {
        id: TaskId,
        /// Lines scrolled up from the end; 0 follows the output.
        scroll: usize,
    },
    Message {
        title: String,
        body: Vec<String>,
        error: bool,
    },
}

pub struct Confirm {
    pub title: String,
    pub body: Vec<String>,
    pub argv: Vec<String>,
    /// Irreversible or customer-facing: red styling.
    pub danger: bool,
    pub action: PendingAction,
}

pub struct Input {
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub purpose: InputPurpose,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputPurpose {
    TestClasses { org: String },
    OrgName { org_key: String },
    Limit,
}

pub struct Picker {
    pub title: String,
    pub items: Vec<PickItem>,
    pub cursor: usize,
    pub purpose: PickPurpose,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PickItem {
    pub label: String,
    pub detail: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PickPurpose {
    InstallOrg { version_id: String, label: String },
    DeployOrg,
    Setting(SettingKey),
}

/// What a confirmed dialog runs.
#[derive(Clone, Debug, PartialEq)]
pub enum PendingAction {
    SchedulePush(ScheduleSpec),
    AbortRequest(String),
    Promote {
        version_id: String,
        label: String,
    },
    Install {
        version_id: String,
        label: String,
        org: String,
    },
    CreateVersion,
    DeleteScratch(String),
    Deploy(String),
    RunTests {
        org: String,
        classes: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Version,
    Orgs,
    Time,
    Confirm,
}

impl Step {
    pub fn number(self) -> usize {
        match self {
            Step::Version => 1,
            Step::Orgs => 2,
            Step::Time => 3,
            Step::Confirm => 4,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Step::Version => "choose the version",
            Step::Orgs => "choose the orgs",
            Step::Time => "choose the start time",
            Step::Confirm => "confirm",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WizardVersion {
    pub id: String,
    pub label: String,
    pub name: String,
    pub key: VersionKey,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrgChoice {
    pub key: String,
    pub name: String,
    pub org_type: String,
    pub installed: String,
    pub installed_key: Option<VersionKey>,
    pub important: bool,
    pub checked: bool,
    pub warn: Option<&'static str>,
}

pub struct Wizard {
    pub hub: String,
    pub step: Step,
    pub versions: Vec<WizardVersion>,
    pub version: usize,
    pub orgs: Vec<OrgChoice>,
    pub cursor: usize,
    pub start_time: String,
    /// The failed request this retry is based on, if any.
    pub retry_of: Option<String>,
    /// Org keys to check instead of "every org behind the version".
    preselected: Option<Vec<String>>,
}

impl Wizard {
    pub fn new(
        hub: String,
        versions: Vec<WizardVersion>,
        version: usize,
        orgs: Vec<OrgChoice>,
        preselected: Option<Vec<String>>,
        retry_of: Option<String>,
    ) -> Self {
        let mut wizard = Self {
            hub,
            step: Step::Version,
            version: version.min(versions.len().saturating_sub(1)),
            versions,
            orgs,
            cursor: 0,
            start_time: String::new(),
            retry_of,
            preselected,
        };
        wizard.apply_selection();
        wizard
    }

    pub fn selected_version(&self) -> &WizardVersion {
        &self.versions[self.version]
    }

    /// Re-derives warnings and the default selection for the chosen version.
    /// Marked orgs behind the version if any org is marked, else every org behind it.
    pub fn apply_selection(&mut self) {
        let key = self.selected_version().key;
        let any_important = self.has_important();
        for org in &mut self.orgs {
            org.warn = match org.installed_key {
                Some(installed) if installed == key => Some("already on this version"),
                Some(installed) if installed > key => Some("has a newer version"),
                None => Some("installed version unknown"),
                _ => None,
            };
            org.checked = match &self.preselected {
                Some(keys) => keys.contains(&org.key),
                None => org.warn.is_none() && (org.important || !any_important),
            };
        }
    }

    pub fn select_version(&mut self, index: usize) {
        if index < self.versions.len() && index != self.version {
            self.version = index;
            self.apply_selection();
        }
    }

    pub fn toggle(&mut self, index: usize) {
        if let Some(org) = self.orgs.get_mut(index) {
            org.checked = !org.checked;
        }
    }

    pub fn check_all_behind(&mut self) {
        for org in &mut self.orgs {
            org.checked = org.warn.is_none();
        }
    }

    pub fn check_important_behind(&mut self) {
        for org in &mut self.orgs {
            org.checked = org.important && org.warn.is_none();
        }
    }

    pub fn has_important(&self) -> bool {
        self.orgs.iter().any(|o| o.important)
    }

    pub fn check_none(&mut self) {
        for org in &mut self.orgs {
            org.checked = false;
        }
    }

    pub fn checked(&self) -> Vec<&OrgChoice> {
        self.orgs.iter().filter(|o| o.checked).collect()
    }

    pub fn start_time_error(&self) -> Option<String> {
        let value = self.start_time.trim();
        if value.is_empty() {
            return None;
        }
        NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
            .err()
            .map(|_| "use the format 2026-12-06T21:00:00 (UTC)".to_string())
    }

    pub fn spec(&self) -> ScheduleSpec {
        let version = self.selected_version();
        let checked = self.checked();
        ScheduleSpec {
            version_id: version.id.clone(),
            version_label: version.label.clone(),
            org_keys: checked.iter().map(|o| o.key.clone()).collect(),
            org_names: checked.iter().map(|o| o.name.clone()).collect(),
            start_time: Some(self.start_time.trim().to_string()).filter(|t| !t.is_empty()),
        }
    }

    pub fn argv(&self) -> Vec<String> {
        let spec = self.spec();
        build_schedule(
            &self.hub,
            &spec.version_id,
            &spec.org_keys,
            spec.start_time.as_deref(),
        )
    }
}
