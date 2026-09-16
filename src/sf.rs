//! Read-only data access through the Salesforce CLI (`sf data query`, standard Data API).

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, FixedOffset};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::process::Command;
use std::thread::ScopedJoinHandle;

pub type Timestamp = DateTime<FixedOffset>;

#[derive(Clone, Debug)]
pub struct PushRequest {
    pub id: String,
    pub status: String,
    pub version_id: String,
    pub scheduled: Option<Timestamp>,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
}

#[derive(Clone, Debug)]
pub struct PushJob {
    pub id: String,
    pub request_id: String,
    pub org_key: String,
    pub status: String,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
}

#[derive(Clone, Debug)]
pub struct PushError {
    pub job_id: String,
    pub severity: String,
    pub kind: String,
    pub title: String,
    pub message: String,
    pub details: String,
}

#[derive(Clone, Debug)]
pub struct Version {
    pub name: String,
    pub state: String,
    pub major: i64,
    pub minor: i64,
    pub patch: i64,
    pub build: i64,
}

impl Version {
    pub fn label(&self) -> String {
        format!("{}.{}.{}.{}", self.major, self.minor, self.patch, self.build)
    }

    fn key(&self) -> (i64, i64, i64, i64) {
        (self.major, self.minor, self.patch, self.build)
    }
}

#[derive(Clone, Debug)]
pub struct Subscriber {
    pub org_key: String,
    pub name: String,
    pub org_type: String,
    pub org_status: String,
    pub instance: String,
    pub version_id: String,
}

#[derive(Clone, Debug)]
pub struct LocalOrg {
    pub alias: String,
    pub username: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Counts {
    pub succeeded: usize,
    pub failed: usize,
    pub other: usize,
}

impl Counts {
    pub fn total(&self) -> usize {
        self.succeeded + self.failed + self.other
    }
}

#[derive(Clone, Debug, Default)]
pub struct Data {
    pub requests: Vec<PushRequest>,
    pub jobs: Vec<PushJob>,
    pub errors: Vec<PushError>,
    pub versions: HashMap<String, Version>,
    pub subscribers: Vec<Subscriber>,
    pub local_orgs: HashMap<String, LocalOrg>,
    pub latest_released: Option<String>,
}

impl Data {
    /// Jobs of one request: failures first, then by org name.
    pub fn jobs_for(&self, request_id: &str) -> Vec<&PushJob> {
        let mut jobs: Vec<&PushJob> = self.jobs.iter().filter(|j| j.request_id == request_id).collect();
        jobs.sort_by(|a, b| match (a.status == "Failed", b.status == "Failed") {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => self.org_name(&a.org_key).cmp(&self.org_name(&b.org_key)),
        });
        jobs
    }

    pub fn errors_for(&self, job_id: &str) -> Vec<&PushError> {
        self.errors.iter().filter(|e| e.job_id == job_id).collect()
    }

    pub fn counts(&self, request_id: &str) -> Counts {
        let mut counts = Counts::default();
        for job in self.jobs.iter().filter(|j| j.request_id == request_id) {
            match job.status.as_str() {
                "Succeeded" => counts.succeeded += 1,
                "Failed" => counts.failed += 1,
                _ => counts.other += 1,
            }
        }
        counts
    }

    pub fn subscriber(&self, org_key: &str) -> Option<&Subscriber> {
        self.subscribers.iter().find(|s| s.org_key == org_key)
    }

    pub fn org_name(&self, org_key: &str) -> String {
        self.subscriber(org_key)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "Unknown org".into())
    }

    pub fn alias(&self, org_key: &str) -> Option<&LocalOrg> {
        self.local_orgs.get(org_key)
    }

    pub fn version_label(&self, version_id: &str) -> String {
        self.versions
            .get(version_id)
            .map(Version::label)
            .unwrap_or_else(|| "?".into())
    }

    pub fn is_latest(&self, version_id: &str) -> bool {
        self.latest_released.as_deref() == Some(version_id)
    }
}

pub fn load(org: &str, limit: usize) -> Result<Data> {
    let requests_soql = format!(
        "SELECT Id, Status, PackageVersionId, ScheduledStartTime, StartTime, EndTime \
         FROM PackagePushRequest ORDER BY ScheduledStartTime DESC NULLS LAST LIMIT {limit}"
    );

    std::thread::scope(|scope| {
        let versions = scope.spawn(|| {
            query(
                org,
                "SELECT Id, Name, MajorVersion, MinorVersion, PatchVersion, BuildNumber, ReleaseState \
                 FROM MetadataPackageVersion",
            )
        });
        let subscribers = scope.spawn(|| {
            query(
                org,
                "SELECT OrgKey, OrgName, OrgType, OrgStatus, InstanceName, MetadataPackageVersionId \
                 FROM PackageSubscriber",
            )
        });
        let local_orgs = scope.spawn(local_orgs);

        let requests = query(org, &requests_soql)?;
        let ids = requests
            .iter()
            .map(|r| text(r, "Id"))
            .filter(|id| id.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(",");

        let (jobs, errors) = if ids.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let errors_soql = format!(
                "SELECT PackagePushJobId, ErrorSeverity, ErrorType, ErrorTitle, ErrorMessage, ErrorDetails \
                 FROM PackagePushError WHERE PackagePushJob.PackagePushRequestId IN ({ids})"
            );
            let jobs_soql = format!(
                "SELECT Id, PackagePushRequestId, SubscriberOrganizationKey, Status, StartTime, EndTime \
                 FROM PackagePushJob WHERE PackagePushRequestId IN ({ids})"
            );
            let errors = scope.spawn(move || query(org, &errors_soql));
            (query(org, &jobs_soql)?, join(errors)?)
        };

        let versions: HashMap<String, Version> = join(versions)?
            .iter()
            .map(|v| {
                let version = Version {
                    name: text(v, "Name"),
                    state: text(v, "ReleaseState"),
                    major: number(v, "MajorVersion"),
                    minor: number(v, "MinorVersion"),
                    patch: number(v, "PatchVersion"),
                    build: number(v, "BuildNumber"),
                };
                (text(v, "Id"), version)
            })
            .collect();
        let latest_released = versions
            .iter()
            .filter(|(_, v)| v.state == "Released")
            .max_by_key(|(_, v)| v.key())
            .map(|(id, _)| id.clone());

        Ok(Data {
            requests: requests
                .iter()
                .map(|r| PushRequest {
                    id: text(r, "Id"),
                    status: text(r, "Status"),
                    version_id: text(r, "PackageVersionId"),
                    scheduled: time(r, "ScheduledStartTime"),
                    start: time(r, "StartTime"),
                    end: time(r, "EndTime"),
                })
                .collect(),
            jobs: jobs
                .iter()
                .map(|j| PushJob {
                    id: text(j, "Id"),
                    request_id: text(j, "PackagePushRequestId"),
                    org_key: org_key(&text(j, "SubscriberOrganizationKey")),
                    status: text(j, "Status"),
                    start: time(j, "StartTime"),
                    end: time(j, "EndTime"),
                })
                .collect(),
            errors: errors
                .iter()
                .map(|e| PushError {
                    job_id: text(e, "PackagePushJobId"),
                    severity: text(e, "ErrorSeverity"),
                    kind: text(e, "ErrorType"),
                    title: text(e, "ErrorTitle"),
                    message: text(e, "ErrorMessage"),
                    details: text(e, "ErrorDetails"),
                })
                .collect(),
            subscribers: join(subscribers)?
                .iter()
                .map(|s| Subscriber {
                    org_key: org_key(&text(s, "OrgKey")),
                    name: text(s, "OrgName"),
                    org_type: text(s, "OrgType"),
                    org_status: text(s, "OrgStatus"),
                    instance: text(s, "InstanceName"),
                    version_id: text(s, "MetadataPackageVersionId"),
                })
                .collect(),
            versions,
            local_orgs: local_orgs.join().unwrap_or_default(),
            latest_released,
        })
    })
}

fn query(org: &str, soql: &str) -> Result<Vec<Value>> {
    // On Windows the Salesforce CLI is a .cmd shim, which Command does not resolve on its own.
    let program = if cfg!(windows) { "sf.cmd" } else { "sf" };
    let output = Command::new(program)
        .args(["data", "query", "--json", "--target-org", org, "--query", soql])
        .output()
        .context("could not run `sf`, is the Salesforce CLI installed and on PATH?")?;
    let json: Value = serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "unexpected output from sf: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })?;
    if json["status"].as_i64() != Some(0) {
        bail!(
            "{}",
            json["message"].as_str().unwrap_or("sf data query failed").trim()
        );
    }
    Ok(json["result"]["records"].as_array().cloned().unwrap_or_default())
}

fn join<T>(handle: ScopedJoinHandle<'_, Result<T>>) -> Result<T> {
    handle.join().map_err(|_| anyhow!("query thread panicked"))?
}

/// Orgs authenticated in the local sf CLI, keyed by the 15-character org ID.
fn local_orgs() -> HashMap<String, LocalOrg> {
    let mut orgs = HashMap::new();
    let Some(home) = std::env::home_dir() else {
        return orgs;
    };
    let dir = home.join(".sfdx");

    let mut aliases: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(json) = read_json(&dir.join("alias.json"))
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

    let Ok(entries) = std::fs::read_dir(&dir) else {
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
        let mut names = aliases.get(username).cloned().unwrap_or_default();
        names.sort();
        orgs.insert(
            org_key(org_id),
            LocalOrg {
                alias: names.join(", "),
                username: username.to_string(),
            },
        );
    }
    orgs
}

fn read_json(path: &std::path::Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn org_key(id: &str) -> String {
    id.chars().take(15).collect()
}

fn text(record: &Value, field: &str) -> String {
    record[field].as_str().unwrap_or_default().to_string()
}

fn number(record: &Value, field: &str) -> i64 {
    record[field].as_i64().unwrap_or_default()
}

fn time(record: &Value, field: &str) -> Option<Timestamp> {
    DateTime::parse_from_str(record[field].as_str()?, "%Y-%m-%dT%H:%M:%S%.f%z").ok()
}
