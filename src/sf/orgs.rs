//! Orgs known to the local sf CLI (`sf org list`), their aliases, and the org-level commands.

use super::query::{flag, org_key, text};
use super::run_json;
use anyhow::Result;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OrgKind {
    DevHub,
    Production,
    Sandbox,
    Scratch,
}

impl OrgKind {
    pub fn label(self) -> &'static str {
        match self {
            OrgKind::DevHub => "Dev Hub",
            OrgKind::Production => "Production",
            OrgKind::Sandbox => "Sandbox",
            OrgKind::Scratch => "Scratch",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Ok,
    Unknown,
    Down,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrgInfo {
    pub username: String,
    pub aliases: Vec<String>,
    pub org_id: String,
    pub instance_url: String,
    pub kind: OrgKind,
    pub status: String,
    pub expires: Option<NaiveDate>,
    pub expired: bool,
    pub is_default: bool,
    pub is_default_hub: bool,
    pub org_name: String,
    pub dev_hub: String,
}

impl OrgInfo {
    pub fn alias(&self) -> &str {
        self.aliases
            .first()
            .map_or(self.username.as_str(), String::as_str)
    }

    pub fn host(&self) -> String {
        self.instance_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string()
    }

    pub fn health(&self) -> Health {
        if self.expired {
            Health::Expired
        } else {
            match self.status.as_str() {
                "Connected" | "Active" => Health::Ok,
                "" | "Unknown" => Health::Unknown,
                _ => Health::Down,
            }
        }
    }

    pub fn has_duplicate_aliases(&self) -> bool {
        self.aliases.len() > 1
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LocalOrg {
    pub alias: String,
    pub username: String,
}

pub fn build_org_list() -> Vec<String> {
    super::argv(&["org", "list", "--all", "--json"])
}

pub fn load_orgs() -> Result<Vec<OrgInfo>> {
    let json = run_json(&build_org_list(), None)?;
    Ok(parse_org_list(&json, &local_aliases()))
}

/// Flattens the groups of `sf org list --json`. The same org can appear in several groups.
pub fn parse_org_list(json: &Value, aliases: &HashMap<String, Vec<String>>) -> Vec<OrgInfo> {
    let mut seen = HashSet::new();
    let mut orgs = Vec::new();
    for group in ["devHubs", "nonScratchOrgs", "sandboxes", "scratchOrgs", "other"] {
        for record in json["result"][group].as_array().into_iter().flatten() {
            let username = text(record, "username");
            if username.is_empty() || !seen.insert(username.clone()) {
                continue;
            }
            let kind = if flag(record, "isDevHub") {
                OrgKind::DevHub
            } else if flag(record, "isScratch") || group == "scratchOrgs" {
                OrgKind::Scratch
            } else if flag(record, "isSandbox") || group == "sandboxes" {
                OrgKind::Sandbox
            } else {
                OrgKind::Production
            };
            let mut names = aliases.get(&username).cloned().unwrap_or_default();
            let alias = text(record, "alias");
            if !alias.is_empty() && !names.contains(&alias) {
                names.push(alias);
            }
            names.sort();
            let status = text(record, "status");
            let connected = text(record, "connectedStatus");
            let expires = record["expirationDate"]
                .as_str()
                .and_then(|d| NaiveDate::parse_from_str(d.get(..10).unwrap_or(d), "%Y-%m-%d").ok());
            orgs.push(OrgInfo {
                username,
                aliases: names,
                org_id: text(record, "orgId"),
                instance_url: text(record, "instanceUrl"),
                kind,
                expired: flag(record, "isExpired") || status.eq_ignore_ascii_case("expired"),
                status: if connected.is_empty() { status } else { connected },
                expires,
                is_default: flag(record, "isDefaultUsername"),
                is_default_hub: flag(record, "isDefaultDevHubUsername"),
                org_name: text(record, "orgName"),
                dev_hub: text(record, "devHubUsername"),
            });
        }
    }
    orgs.sort_by_key(|o| (!o.is_default, !o.is_default_hub, o.kind, o.alias().to_lowercase()));
    orgs
}

/// Aliases per username from `~/.sfdx/alias.json`.
pub fn local_aliases() -> HashMap<String, Vec<String>> {
    let mut aliases: HashMap<String, Vec<String>> = HashMap::new();
    let Some(home) = std::env::home_dir() else {
        return aliases;
    };
    if let Some(json) = read_json(&home.join(".sfdx/alias.json"))
        && let Some(entries) = json["orgs"].as_object()
    {
        for (alias, username) in entries {
            if let Some(username) = username.as_str() {
                aliases
                    .entry(username.to_string())
                    .or_default()
                    .push(alias.clone());
            }
        }
    }
    for names in aliases.values_mut() {
        names.sort();
    }
    aliases
}

/// Orgs authenticated in the local sf CLI, keyed by the 15-character org ID.
pub fn local_orgs() -> HashMap<String, LocalOrg> {
    let mut orgs = HashMap::new();
    let Some(home) = std::env::home_dir() else {
        return orgs;
    };
    let aliases = local_aliases();
    let Ok(entries) = std::fs::read_dir(home.join(".sfdx")) else {
        return orgs;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") || !name.contains('@') {
            continue;
        }
        let Some(json) = read_json(&entry.path()) else {
            continue;
        };
        let (Some(org_id), Some(username)) = (json["orgId"].as_str(), json["username"].as_str()) else {
            continue;
        };
        orgs.insert(
            org_key(org_id),
            LocalOrg {
                alias: aliases
                    .get(username)
                    .map(|names| names.join(", "))
                    .unwrap_or_default(),
                username: username.to_string(),
            },
        );
    }
    orgs
}

fn read_json(path: &std::path::Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn build_open(org: &str) -> Vec<String> {
    super::argv(&["org", "open", "--target-org", org])
}

pub fn build_delete_scratch(org: &str) -> Vec<String> {
    super::argv(&[
        "org",
        "delete",
        "scratch",
        "--target-org",
        org,
        "--no-prompt",
        "--json",
    ])
}

pub fn build_installed_list(org: &str) -> Vec<String> {
    super::argv(&["package", "installed", "list", "--target-org", org, "--json"])
}

/// The installed version number of the package, or None when it is not installed.
/// Without a package filter, the version is only known when exactly one package is installed.
pub fn parse_installed(json: &Value, subscriber_package_id: Option<&str>) -> Option<String> {
    let packages = json["result"].as_array()?;
    let record = match subscriber_package_id {
        Some(id) => packages
            .iter()
            .find(|p| text(p, "SubscriberPackageId").starts_with(&org_key(id)))?,
        None if packages.len() == 1 => &packages[0],
        None => return None,
    };
    Some(text(record, "SubscriberPackageVersionNumber"))
}

pub fn load_installed(org: &str, subscriber_package_id: Option<&str>) -> Result<Option<String>> {
    let json = run_json(&build_installed_list(org), None)?;
    Ok(parse_installed(&json, subscriber_package_id))
}
