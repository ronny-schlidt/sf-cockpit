//! Push upgrades: requests, jobs, errors, versions and subscribers of one package, plus the commands to
//! schedule and abort a push. `PackagePush*` objects must be queried through the standard Data API.

use super::orgs::{LocalOrg, local_orgs};
use super::query::{Api, Timestamp, id_list, literal, number, org_key, query, text, time};
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::thread::ScopedJoinHandle;

/// `Created` is left out: a request whose schedule call failed stays `Created` forever.
pub const ACTIVE_STATES: [&str; 2] = ["Pending", "InProgress"];

pub type VersionKey = (i64, i64, i64, i64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PackageIds {
    /// The 2GP package (0Ho).
    pub id: String,
    /// The subscriber package (033) that `MetadataPackageVersion` and `PackageSubscriber` refer to.
    pub subscriber_package_id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushRequest {
    pub id: String,
    pub status: String,
    pub version_id: String,
    pub scheduled: Option<Timestamp>,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
}

impl PushRequest {
    pub fn is_active(&self) -> bool {
        ACTIVE_STATES.contains(&self.status.as_str())
    }

    pub fn can_abort(&self) -> bool {
        matches!(self.status.as_str(), "Created" | "Pending")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushJob {
    pub id: String,
    pub request_id: String,
    pub org_key: String,
    pub status: String,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushError {
    pub job_id: String,
    pub severity: String,
    pub kind: String,
    pub title: String,
    pub message: String,
    pub details: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

    pub fn key(&self) -> VersionKey {
        (self.major, self.minor, self.patch, self.build)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subscriber {
    pub org_key: String,
    pub name: String,
    pub org_type: String,
    pub org_status: String,
    pub instance: String,
    pub version_id: String,
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

/// What the user chose in the schedule wizard.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleSpec {
    pub version_id: String,
    pub version_label: String,
    pub org_keys: Vec<String>,
    pub org_names: Vec<String>,
    /// UTC, `YYYY-MM-DDTHH:MM:SS`. None schedules as soon as possible.
    pub start_time: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PushData {
    pub package: Option<PackageIds>,
    pub requests: Vec<PushRequest>,
    pub jobs: Vec<PushJob>,
    pub errors: Vec<PushError>,
    pub versions: HashMap<String, Version>,
    pub subscribers: Vec<Subscriber>,
    pub local_orgs: HashMap<String, LocalOrg>,
    pub latest_released: Option<String>,
}

impl PushData {
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

    pub fn has_active_request(&self) -> bool {
        self.requests.iter().any(PushRequest::is_active)
    }

    /// Released versions, newest first.
    pub fn released_versions(&self) -> Vec<(&str, &Version)> {
        let mut versions: Vec<(&str, &Version)> = self
            .versions
            .iter()
            .filter(|(_, v)| v.state == "Released")
            .map(|(id, v)| (id.as_str(), v))
            .collect();
        versions.sort_by_key(|(_, v)| std::cmp::Reverse(v.key()));
        versions
    }

    pub fn subscribers_on(&self, version_id: &str) -> usize {
        self.subscribers
            .iter()
            .filter(|s| s.version_id == version_id)
            .count()
    }
}

const PACKAGE_SELECT: &str = "SELECT Id, Name, SubscriberPackageId FROM Package2";

fn package_ids(hub: &str, soql: &str) -> Result<Vec<PackageIds>> {
    Ok(query(hub, soql, Api::Tooling)?
        .iter()
        .map(|r| PackageIds {
            id: text(r, "Id"),
            subscriber_package_id: text(r, "SubscriberPackageId"),
            name: text(r, "Name"),
        })
        .collect())
}

/// All packages owned by the Dev Hub, by name.
pub fn load_packages(hub: &str) -> Result<Vec<PackageIds>> {
    package_ids(hub, &format!("{PACKAGE_SELECT} ORDER BY Name"))
}

/// Finds the package by 0Ho id or name. Without a name, the hub's only package is used, if there is one.
pub fn resolve_package(hub: &str, package: Option<&str>) -> Result<Option<PackageIds>> {
    let soql = match package {
        Some(p) if p.starts_with("0Ho") && p.chars().all(|c| c.is_ascii_alphanumeric()) => {
            format!("{PACKAGE_SELECT} WHERE Id = '{p}'")
        }
        Some(p) => format!("{PACKAGE_SELECT} WHERE Name = {}", literal(p)),
        None => PACKAGE_SELECT.into(),
    };
    let mut packages = package_ids(hub, &soql)?;
    match (package, packages.len()) {
        (_, 1) => Ok(Some(packages.remove(0))),
        (Some(p), 0) => bail!("package {p} not found in {hub}"),
        (Some(p), _) => bail!("package {p} matches several packages in {hub}, use the 0Ho id"),
        (None, _) => Ok(None),
    }
}

pub fn load(hub: &str, limit: usize, package: Option<&str>) -> Result<PushData> {
    let package = match package {
        Some(p) => resolve_package(hub, Some(p))?,
        None => resolve_package(hub, None).unwrap_or(None),
    };
    let package_filter = package
        .as_ref()
        .map(|p| format!(" WHERE MetadataPackageId = {}", literal(&p.subscriber_package_id)))
        .unwrap_or_default();
    let requests_soql = format!(
        "SELECT Id, Status, PackageVersionId, ScheduledStartTime, StartTime, EndTime \
         FROM PackagePushRequest ORDER BY ScheduledStartTime DESC NULLS LAST LIMIT {limit}"
    );
    let versions_soql = format!(
        "SELECT Id, Name, MajorVersion, MinorVersion, PatchVersion, BuildNumber, ReleaseState \
         FROM MetadataPackageVersion{package_filter}"
    );
    let subscribers_soql = format!(
        "SELECT OrgKey, OrgName, OrgType, OrgStatus, InstanceName, MetadataPackageVersionId \
         FROM PackageSubscriber{package_filter}"
    );

    std::thread::scope(|scope| {
        let versions = scope.spawn(|| query(hub, &versions_soql, Api::Data));
        let subscribers = scope.spawn(|| query(hub, &subscribers_soql, Api::Data));
        let local_orgs = scope.spawn(local_orgs);

        let requests = query(hub, &requests_soql, Api::Data)?;
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

        let mut requests: Vec<PushRequest> = requests
            .iter()
            .map(|r| PushRequest {
                id: text(r, "Id"),
                status: text(r, "Status"),
                version_id: text(r, "PackageVersionId"),
                scheduled: time(r, "ScheduledStartTime"),
                start: time(r, "StartTime"),
                end: time(r, "EndTime"),
            })
            .collect();
        if package.is_some() {
            requests.retain(|r| versions.contains_key(&r.version_id));
        }

        let ids = id_list(requests.iter().map(|r| r.id.as_str()));
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
            let errors = scope.spawn(move || query(hub, &errors_soql, Api::Data));
            (query(hub, &jobs_soql, Api::Data)?, join(errors)?)
        };

        let latest_released = versions
            .iter()
            .filter(|(_, v)| v.state == "Released")
            .max_by_key(|(_, v)| v.key())
            .map(|(id, _)| id.clone());

        Ok(PushData {
            package,
            requests,
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

fn join<T>(handle: ScopedJoinHandle<'_, Result<T>>) -> Result<T> {
    handle.join().map_err(|_| anyhow!("query thread panicked"))?
}

pub fn build_schedule(
    hub: &str,
    version_id: &str,
    org_keys: &[String],
    start_time: Option<&str>,
) -> Vec<String> {
    let mut argv = super::argv(&[
        "package",
        "push-upgrade",
        "schedule",
        "--target-dev-hub",
        hub,
        "--package",
        version_id,
        "--org-list",
        &org_keys.join(","),
    ]);
    if let Some(time) = start_time {
        argv.push("--start-time".into());
        argv.push(time.into());
    }
    argv.push("--json".into());
    argv
}

pub fn build_abort(hub: &str, request_id: &str) -> Vec<String> {
    super::argv(&[
        "package",
        "push-upgrade",
        "abort",
        "--target-dev-hub",
        hub,
        "--push-request-id",
        request_id,
        "--json",
    ])
}

/// The request id from a successful `push-upgrade schedule`.
pub fn scheduled_request_id(json: &serde_json::Value) -> Option<String> {
    json["result"]["PushRequestId"].as_str().map(str::to_string)
}
