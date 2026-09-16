//! Fictional sample data for `--demo` and the tests.

use crate::sf::deploy::DeployRecord;
use crate::sf::orgs::{LocalOrg, OrgInfo, OrgKind};
use crate::sf::push::{PackageIds, PushData, PushError, PushJob, PushRequest, Subscriber, Version};
use crate::sf::query::Timestamp;
use crate::sf::runner::TaskKind;
use crate::sf::versions::PackageVersion;
use chrono::{DateTime, NaiveDate};
use serde_json::{Value, json};
use std::collections::HashMap;

const V_OLD: &str = "04t000000000001";
const V_PREV: &str = "04t000000000002";
const V_LATEST: &str = "04t000000000003";
const V_BETA: &str = "04t000000000004";

pub const ACME: &str = "00D000000000A01";
pub const GLOBEX: &str = "00D000000000B02";
pub const INITECH: &str = "00D000000000C03";
pub const UMBRELLA: &str = "00D000000000D04";
pub const STARK: &str = "00D000000000E05";

pub fn push() -> PushData {
    let version = |name: &str, minor, patch, build| Version {
        name: name.into(),
        state: "Released".into(),
        major: 2,
        minor,
        patch,
        build,
    };
    let subscriber = |org: &str, name: &str, org_type: &str, instance: &str, version: &str| Subscriber {
        org_key: org.into(),
        name: name.into(),
        org_type: org_type.into(),
        org_status: "Active".into(),
        instance: instance.into(),
        version_id: version.into(),
    };

    let mut jobs = Vec::new();
    let mut errors = Vec::new();
    let mut add_job = |request: &str,
                       org: &str,
                       status: &str,
                       start: &str,
                       secs: i64,
                       error: Option<(&str, &str, &str, &str)>| {
        let id = format!("0DX{:012}", jobs.len() + 1);
        let start = time(start);
        if let Some((kind, title, message, details)) = error {
            errors.push(PushError {
                job_id: id.clone(),
                severity: "Error".into(),
                kind: kind.into(),
                title: title.into(),
                message: message.into(),
                details: details.into(),
            });
        }
        jobs.push(PushJob {
            id,
            request_id: request.into(),
            org_key: org.into(),
            status: status.into(),
            start,
            end: start.map(|s| s + chrono::Duration::seconds(secs)),
        });
    };

    let r1 = "0DV000000000001";
    add_job(r1, ACME, "Succeeded", "2026-09-15T08:00:07Z", 94, None);
    add_job(r1, UMBRELLA, "Succeeded", "2026-09-15T08:00:09Z", 121, None);
    add_job(
        r1,
        GLOBEX,
        "Failed",
        "2026-09-15T08:00:08Z",
        640,
        Some((
            "UnclassifiedError",
            "Unexpected Failure",
            "An unexpected failure was experienced during the upgrade. The subscriber's organization was \
             unaffected. Contact salesforce.com Support through your normal channels and provide the \
             following error number: 12345678-90123 (1029384756).",
            "",
        )),
    );
    add_job(
        r1,
        INITECH,
        "Failed",
        "2026-09-15T08:00:10Z",
        3,
        Some((
            "IneligibleUpgrade",
            "Not Available",
            "This package is not yet available. Please try again later or contact the package owner.",
            "",
        )),
    );
    add_job(
        r1,
        STARK,
        "Failed",
        "2026-09-15T08:00:11Z",
        412,
        Some((
            "ApexTestFailure",
            "Apex Test Failure",
            "The upgrade failed because an Apex test failed in the subscriber org.",
            "InvoiceServiceTest.testCreateInvoice: System.AssertException: Assertion Failed: Expected 2, Actual 0",
        )),
    );

    let r2 = "0DV000000000002";
    for (org, secs) in [(ACME, 58), (GLOBEX, 61), (INITECH, 49), (UMBRELLA, 70)] {
        add_job(r2, org, "Succeeded", "2026-08-20T07:30:05Z", secs, None);
    }
    add_job(
        r2,
        STARK,
        "Failed",
        "2026-08-20T07:30:06Z",
        2,
        Some((
            "IneligibleUpgrade",
            "Ineligible Upgrade",
            "The installed package version cannot be upgraded to this version.",
            "",
        )),
    );

    let r3 = "0DV000000000003";
    for (org, secs) in [
        (ACME, 44),
        (GLOBEX, 47),
        (INITECH, 39),
        (UMBRELLA, 52),
        (STARK, 45),
    ] {
        add_job(r3, org, "Succeeded", "2026-07-02T06:15:03Z", secs, None);
    }

    let request = |id: &str, status: &str, version: &str, start: &str, secs: i64| {
        let start = time(start);
        PushRequest {
            id: id.into(),
            status: status.into(),
            version_id: version.into(),
            scheduled: start,
            start,
            end: start.map(|s| s + chrono::Duration::seconds(secs)),
        }
    };

    PushData {
        package: Some(PackageIds {
            id: "0Ho000000000001".into(),
            subscriber_package_id: "033000000000001".into(),
            name: "Demo Package".into(),
        }),
        requests: vec![
            request(r1, "Failed", V_LATEST, "2026-09-15T08:00:03Z", 745),
            request(r2, "Failed", V_PREV, "2026-08-20T07:30:02Z", 96),
            request(r3, "Succeeded", V_OLD, "2026-07-02T06:15:01Z", 70),
        ],
        jobs,
        errors,
        versions: HashMap::from([
            (V_OLD.into(), version("Summer '26", 3, 0, 4)),
            (V_PREV.into(), version("Summer '26", 3, 1, 2)),
            (V_LATEST.into(), version("Autumn '26", 4, 0, 1)),
        ]),
        subscribers: vec![
            subscriber(ACME, "Acme Corporation", "Production", "NA224", V_LATEST),
            subscriber(GLOBEX, "Globex Industries", "Production", "EU46", V_PREV),
            subscriber(INITECH, "Initech UAT", "Sandbox", "CS162", V_PREV),
            subscriber(UMBRELLA, "Umbrella Health", "Production", "AP27", V_LATEST),
            subscriber(STARK, "Stark Logistics", "Production", "NA201", V_OLD),
        ],
        local_orgs: HashMap::from([
            (
                ACME.into(),
                LocalOrg {
                    alias: "acme, acme-prod".into(),
                    username: "admin@acme.example".into(),
                },
            ),
            (
                INITECH.into(),
                LocalOrg {
                    alias: "initech-uat".into(),
                    username: "admin@initech.example.uat".into(),
                },
            ),
        ]),
        latest_released: Some(V_LATEST.into()),
    }
}

pub fn packages() -> Vec<PackageIds> {
    let package = |id: &str, subscriber_package_id: &str, name: &str| PackageIds {
        id: id.into(),
        subscriber_package_id: subscriber_package_id.into(),
        name: name.into(),
    };
    vec![
        package("0Ho000000000001", "033000000000001", "Demo Package"),
        package("0Ho000000000002", "033000000000002", "Demo Package Extension"),
    ]
}

pub fn orgs() -> Vec<OrgInfo> {
    let org = |aliases: &[&str], username: &str, org_id: &str, kind: OrgKind, status: &str| OrgInfo {
        username: username.into(),
        aliases: aliases.iter().map(|a| a.to_string()).collect(),
        org_id: org_id.into(),
        instance_url: format!("https://{}.my.salesforce.com", aliases[0]),
        kind,
        status: status.into(),
        expires: None,
        expired: false,
        is_default: false,
        is_default_hub: false,
        org_name: String::new(),
        dev_hub: String::new(),
    };
    vec![
        OrgInfo {
            is_default: true,
            expires: NaiveDate::from_ymd_opt(2026, 9, 21),
            dev_hub: "admin@demo-hub.example".into(),
            org_name: "Demo scratch org".into(),
            ..org(
                &["scratch"],
                "test-demo1@example.com",
                "00D000000000S01AAA",
                OrgKind::Scratch,
                "Active",
            )
        },
        OrgInfo {
            is_default_hub: true,
            org_name: "Demo ISV".into(),
            ..org(
                &["demo-hub"],
                "admin@demo-hub.example",
                "00D000000000H01AAA",
                OrgKind::DevHub,
                "Connected",
            )
        },
        org(
            &["acme", "acme-prod"],
            "admin@acme.example",
            "00D000000000A01AAB",
            OrgKind::Production,
            "Connected",
        ),
        org(
            &["initech-uat"],
            "admin@initech.example.uat",
            "00D000000000C03AAC",
            OrgKind::Sandbox,
            "RefreshTokenAuthError",
        ),
        OrgInfo {
            expired: true,
            expires: NaiveDate::from_ymd_opt(2026, 8, 30),
            dev_hub: "admin@demo-hub.example".into(),
            ..org(
                &["old-scratch"],
                "test-demo0@example.com",
                "00D000000000S00AAA",
                OrgKind::Scratch,
                "Expired",
            )
        },
    ]
}

pub fn installed(username: &str) -> Option<String> {
    match username {
        "admin@acme.example" => Some("2.4.0.1".into()),
        "admin@initech.example.uat" => Some("2.3.1.2".into()),
        _ => None,
    }
}

pub fn versions() -> Vec<PackageVersion> {
    let version =
        |id: &str, version: &str, name: &str, released: bool, created: &str, coverage: f64| PackageVersion {
            id: id.into(),
            version: version.into(),
            name: name.into(),
            released,
            created: created.into(),
            coverage: Some(coverage),
            coverage_ok: Some(coverage >= 75.0),
            ancestor_id: String::new(),
            ancestor_version: String::new(),
            branch: String::new(),
            tag: String::new(),
            build_seconds: Some(1260),
        };
    vec![
        PackageVersion {
            ancestor_id: V_LATEST.into(),
            ancestor_version: "2.4.0.1".into(),
            ..version(V_BETA, "2.4.1.1", "Autumn '26", false, "2026-09-16 09:12", 81.0)
        },
        version(V_LATEST, "2.4.0.1", "Autumn '26", true, "2026-09-10 16:40", 84.0),
        version(V_PREV, "2.3.1.2", "Summer '26", true, "2026-08-12 11:05", 83.0),
        version(V_OLD, "2.3.0.4", "Summer '26", true, "2026-06-28 14:22", 79.0),
    ]
}

pub fn deploys() -> Vec<DeployRecord> {
    let deploy = |id: &str, status: &str, start: &str, secs: i64, components: (i64, i64, i64)| {
        let start = time(start);
        DeployRecord {
            id: id.into(),
            status: status.into(),
            start,
            end: start.map(|s| s + chrono::Duration::seconds(secs)),
            created: start,
            components_deployed: components.0,
            component_errors: components.1,
            components_total: components.2,
            tests_completed: 0,
            test_errors: 0,
            tests_total: 0,
            check_only: false,
            test_level: "NoTestRun".into(),
            error_message: String::new(),
            created_by: "Demo User".into(),
        }
    };
    vec![
        deploy(
            "0Af000000000003",
            "Succeeded",
            "2026-09-16T09:40:12Z",
            118,
            (1204, 0, 1204),
        ),
        DeployRecord {
            error_message: "LicenseChecker: Variable does not exist: orgLimit (58:17)".into(),
            ..deploy(
                "0Af000000000002",
                "Failed",
                "2026-09-16T09:31:02Z",
                41,
                (0, 1, 1204),
            )
        },
        DeployRecord {
            check_only: true,
            tests_completed: 312,
            tests_total: 312,
            test_level: "RunLocalTests".into(),
            ..deploy(
                "0Af000000000001",
                "Succeeded",
                "2026-09-15T17:02:44Z",
                486,
                (1204, 0, 1204),
            )
        },
    ]
}

pub fn task_lines(kind: TaskKind) -> Vec<String> {
    let lines: &[&str] = match kind {
        TaskKind::Deploy => &[
            "Deploying v65.0 metadata to test-demo1@example.com using the v65.0 SOAP API.",
            "Status: In Progress",
            "Components: 604/1204 (50%)",
            "Components: 1204/1204 (100%)",
            "Status: Succeeded",
        ],
        TaskKind::Tests => &["Running Apex tests…"],
        TaskKind::CreateVersion => &[
            "Request in progress. Status: Verifying metadata",
            "Status: Success",
        ],
        _ => &["Sending the request to Salesforce…"],
    };
    lines.iter().map(|l| l.to_string()).collect()
}

pub fn task_result(kind: TaskKind) -> Value {
    let result = match kind {
        TaskKind::Schedule => json!({
            "PushRequestId": "0DV000000000004",
            "ScheduledStartTime": null,
            "Status": "Pending"
        }),
        TaskKind::CreateVersion => json!({
            "Status": "Success",
            "SubscriberPackageVersionId": "04t000000000005"
        }),
        TaskKind::Tests => json!({
            "summary": {
                "outcome": "Failed",
                "testsRan": 42,
                "passing": 41,
                "failing": 1,
                "skipped": 0,
                "passRate": "98%",
                "testRunCoverage": "86%",
                "orgWideCoverage": "81%",
                "testTotalTimeInMs": 18342
            },
            "tests": [
                {
                    "ApexClass": {"Name": "LicenseCheckerTest"},
                    "MethodName": "expiredLicenseBlocksScan",
                    "Outcome": "Fail",
                    "Message": "System.AssertException: Assertion Failed: Expected: false, Actual: true",
                    "StackTrace": "Class.LicenseCheckerTest.expiredLicenseBlocksScan: line 44, column 1"
                },
                {
                    "ApexClass": {"Name": "ScanServiceTest"},
                    "MethodName": "cleanFileIsReleased",
                    "Outcome": "Pass",
                    "Message": null,
                    "StackTrace": null
                }
            ],
            "coverage": {
                "coverage": [
                    {"name": "ScanService", "totalLines": 210, "totalCovered": 197, "coveredPercent": 93.8},
                    {"name": "LicenseChecker", "totalLines": 88, "totalCovered": 57, "coveredPercent": 64.8}
                ]
            }
        }),
        _ => json!({"success": true}),
    };
    json!({"status": 0, "result": result})
}

fn time(value: &str) -> Option<Timestamp> {
    DateTime::parse_from_rfc3339(value).ok()
}
