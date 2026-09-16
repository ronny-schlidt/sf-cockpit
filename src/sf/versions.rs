//! Package versions of the Dev Hub (`sf package version list`) and the commands to promote, install and
//! create versions.

use super::push::VersionKey;
use super::query::{flag, text};
use super::run_json;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PackageVersion {
    /// 04t id.
    pub id: String,
    pub version: String,
    pub name: String,
    pub released: bool,
    pub created: String,
    pub coverage: Option<f64>,
    pub coverage_ok: Option<bool>,
    pub ancestor_id: String,
    pub ancestor_version: String,
    pub branch: String,
    pub tag: String,
    pub build_seconds: Option<i64>,
}

impl PackageVersion {
    pub fn key(&self) -> VersionKey {
        let mut parts = self
            .version
            .split('.')
            .map(|p| p.parse::<i64>().unwrap_or_default());
        let mut next = || parts.next().unwrap_or_default();
        (next(), next(), next(), next())
    }

    pub fn state(&self) -> &'static str {
        if self.released { "Released" } else { "Beta" }
    }

    pub fn label(&self) -> String {
        format!("{} · {}", self.version, self.name)
    }
}

pub fn install_url(version_id: &str, sandbox: bool) -> String {
    let host = if sandbox { "test" } else { "login" };
    format!("https://{host}.salesforce.com/packaging/installPackage.apexp?p0={version_id}")
}

pub fn build_list(hub: &str, package_id: &str) -> Vec<String> {
    super::argv(&[
        "package",
        "version",
        "list",
        "--packages",
        package_id,
        "--target-dev-hub",
        hub,
        "--verbose",
        "--json",
    ])
}

/// Newest first.
pub fn parse_list(json: &Value) -> Vec<PackageVersion> {
    let mut versions: Vec<PackageVersion> = json["result"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| PackageVersion {
            id: text(v, "SubscriberPackageVersionId"),
            version: text(v, "Version"),
            name: text(v, "Name"),
            released: flag(v, "IsReleased"),
            created: text(v, "CreatedDate"),
            coverage: coverage(&v["CodeCoverage"]),
            coverage_ok: v["HasPassedCodeCoverageCheck"].as_bool(),
            ancestor_id: text(v, "AncestorId"),
            ancestor_version: text(v, "AncestorVersion"),
            branch: text(v, "Branch"),
            tag: text(v, "Tag"),
            build_seconds: v["BuildDurationInSeconds"].as_i64(),
        })
        .collect();
    versions.sort_by_key(|v| std::cmp::Reverse(v.key()));
    versions
}

/// `--verbose` prints `"82%"`; older CLI versions return `{"apexCodeCoveragePercentage": 82}`.
fn coverage(value: &Value) -> Option<f64> {
    match value {
        Value::String(text) => text.trim().trim_end_matches('%').parse().ok(),
        Value::Number(number) => number.as_f64(),
        Value::Object(_) => value["apexCodeCoveragePercentage"].as_f64(),
        _ => None,
    }
}

pub fn load(hub: &str, package_id: &str) -> Result<Vec<PackageVersion>> {
    Ok(parse_list(&run_json(&build_list(hub, package_id), None)?))
}

pub fn build_promote(hub: &str, version_id: &str) -> Vec<String> {
    super::argv(&[
        "package",
        "version",
        "promote",
        "--package",
        version_id,
        "--target-dev-hub",
        hub,
        "--no-prompt",
        "--json",
    ])
}

pub fn build_install(version_id: &str, org: &str) -> Vec<String> {
    super::argv(&[
        "package",
        "install",
        "--package",
        version_id,
        "--target-org",
        org,
        "--wait",
        "30",
        "--publish-wait",
        "30",
        "--no-prompt",
        "--json",
    ])
}

pub fn build_create(
    hub: &str,
    package_id: &str,
    definition_file: &str,
    skip_ancestor_check: bool,
) -> Vec<String> {
    let mut argv = super::argv(&[
        "package",
        "version",
        "create",
        "--package",
        package_id,
        "--target-dev-hub",
        hub,
        "--installation-key-bypass",
        "--code-coverage",
        "--wait",
        "40",
        "--definition-file",
        definition_file,
    ]);
    if skip_ancestor_check {
        argv.push("--skip-ancestor-check".into());
    }
    argv.push("--json".into());
    argv
}

/// The 04t id from a successful `package version create`.
pub fn created_version_id(json: &Value) -> Option<String> {
    json["result"]["SubscriberPackageVersionId"]
        .as_str()
        .map(str::to_string)
}
