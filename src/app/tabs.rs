//! The tab registry and the screen regions used to route mouse events.

use super::Action;
use ratatui::layout::{Position, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TabId {
    Push,
    Subscribers,
    Orgs,
    Versions,
    Deploy,
    Settings,
}

/// A focusable or clickable region of a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Requests,
    Jobs,
    Details,
    Filter,
    Subscribers,
    Orgs,
    OrgDetails,
    Versions,
    VersionDetails,
    Deploys,
    DeployDetails,
    Settings,
}

impl Target {
    pub fn is_details(self) -> bool {
        matches!(
            self,
            Target::Details | Target::OrgDetails | Target::VersionDetails | Target::DeployDetails
        )
    }
}

pub struct TabSpec {
    pub id: TabId,
    pub key: char,
    pub label: &'static str,
    /// Used when the terminal is too narrow for the full labels.
    pub short: &'static str,
    /// Panes in `Tab` order; the first one gets the focus when the tab opens.
    pub panes: &'static [Target],
}

pub const TABS: [TabSpec; 6] = [
    TabSpec {
        id: TabId::Push,
        key: '1',
        label: "Push Upgrades",
        short: "Push",
        panes: &[Target::Requests, Target::Jobs, Target::Details],
    },
    TabSpec {
        id: TabId::Subscribers,
        key: '2',
        label: "Subscribers",
        short: "Subs",
        panes: &[Target::Subscribers],
    },
    TabSpec {
        id: TabId::Orgs,
        key: '3',
        label: "Orgs",
        short: "Orgs",
        panes: &[Target::Orgs, Target::OrgDetails],
    },
    TabSpec {
        id: TabId::Versions,
        key: '4',
        label: "Versions",
        short: "Vers",
        panes: &[Target::Versions, Target::VersionDetails],
    },
    TabSpec {
        id: TabId::Deploy,
        key: '5',
        label: "Deploy & Test",
        short: "Deploy",
        panes: &[Target::Deploys, Target::DeployDetails],
    },
    TabSpec {
        id: TabId::Settings,
        key: '6',
        label: "Settings",
        short: "Settings",
        panes: &[Target::Settings],
    },
];

pub fn spec(tab: TabId) -> &'static TabSpec {
    TABS.iter().find(|t| t.id == tab).expect("every tab has a spec")
}

pub fn tab_for_key(key: char) -> Option<TabId> {
    TABS.iter().find(|t| t.key == key).map(|t| t.id)
}

/// Screen regions from the last rendered frame.
#[derive(Default)]
pub struct Hitboxes {
    pub body: Rect,
    pub divider_x: u16,
    pub regions: Vec<(Target, Rect)>,
    pub buttons: Vec<(Rect, Action)>,
    pub modal_rows: Vec<(Rect, usize)>,
}

impl Hitboxes {
    pub fn register(&mut self, target: Target, rect: Rect) {
        self.regions.push((target, rect));
    }

    pub fn rect(&self, target: Target) -> Rect {
        self.regions
            .iter()
            .find(|(t, _)| *t == target)
            .map(|(_, r)| *r)
            .unwrap_or_default()
    }

    pub fn at(&self, pos: Position) -> Option<Target> {
        self.regions
            .iter()
            .find(|(_, rect)| rect.contains(pos))
            .map(|(target, _)| *target)
    }

    pub fn button_at(&self, pos: Position) -> Option<Action> {
        self.buttons
            .iter()
            .find(|(rect, _)| rect.contains(pos))
            .map(|(_, action)| *action)
    }

    pub fn modal_row_at(&self, pos: Position) -> Option<usize> {
        self.modal_rows
            .iter()
            .find(|(rect, _)| rect.contains(pos))
            .map(|(_, row)| *row)
    }
}
