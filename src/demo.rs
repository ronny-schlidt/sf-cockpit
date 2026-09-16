//! Fictional sample data for `--demo` and the tests.

use crate::sf::{Data, LocalOrg, PushError, PushJob, PushRequest, Subscriber, Timestamp, Version};
use chrono::DateTime;
use std::collections::HashMap;

const V_OLD: &str = "04t000000000001";
const V_PREV: &str = "04t000000000002";
const V_LATEST: &str = "04t000000000003";

const ACME: &str = "00D000000000A01";
const GLOBEX: &str = "00D000000000B02";
const INITECH: &str = "00D000000000C03";
const UMBRELLA: &str = "00D000000000D04";
const STARK: &str = "00D000000000E05";

pub fn data() -> Data {
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

    Data {
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
                    alias: "acme-prod".into(),
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

fn time(value: &str) -> Option<Timestamp> {
    DateTime::parse_from_rfc3339(value).ok()
}
