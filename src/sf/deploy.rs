//! Deployment history (`DeployRequest`, Tooling API), deploys and Apex test runs against one org.

use super::query::{Api, Timestamp, flag, number, query, text, time};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeployRecord {
    pub id: String,
    pub status: String,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub created: Option<Timestamp>,
    pub components_deployed: i64,
    pub component_errors: i64,
    pub components_total: i64,
    pub tests_completed: i64,
    pub test_errors: i64,
    pub tests_total: i64,
    pub check_only: bool,
    pub test_level: String,
    pub error_message: String,
    pub created_by: String,
}

pub const DEPLOYS_SOQL: &str = "SELECT Id, Status, StartDate, CompletedDate, CreatedDate, NumberComponentsDeployed, \
     NumberComponentErrors, NumberComponentsTotal, NumberTestsCompleted, NumberTestErrors, NumberTestsTotal, \
     CheckOnly, TestLevel, ErrorMessage, CreatedBy.Name FROM DeployRequest ORDER BY CreatedDate DESC LIMIT 30";

pub fn parse_deploys(records: &[Value]) -> Vec<DeployRecord> {
    records
        .iter()
        .map(|r| DeployRecord {
            id: text(r, "Id"),
            status: text(r, "Status"),
            start: time(r, "StartDate"),
            end: time(r, "CompletedDate"),
            created: time(r, "CreatedDate"),
            components_deployed: number(r, "NumberComponentsDeployed"),
            component_errors: number(r, "NumberComponentErrors"),
            components_total: number(r, "NumberComponentsTotal"),
            tests_completed: number(r, "NumberTestsCompleted"),
            test_errors: number(r, "NumberTestErrors"),
            tests_total: number(r, "NumberTestsTotal"),
            check_only: flag(r, "CheckOnly"),
            test_level: text(r, "TestLevel"),
            error_message: text(r, "ErrorMessage"),
            created_by: text(&r["CreatedBy"], "Name"),
        })
        .collect()
}

pub fn load_deploys(org: &str) -> Result<Vec<DeployRecord>> {
    Ok(parse_deploys(&query(org, DEPLOYS_SOQL, Api::Tooling)?))
}

pub fn build_deploy(source_dir: &str, org: &str) -> Vec<String> {
    super::argv(&[
        "project",
        "deploy",
        "start",
        "--source-dir",
        source_dir,
        "--target-org",
        org,
        "--ignore-conflicts",
        "--wait",
        "30",
    ])
}

pub fn build_tests(org: &str, classes: &[String]) -> Vec<String> {
    let mut argv = super::argv(&[
        "apex",
        "run",
        "test",
        "--target-org",
        org,
        "--code-coverage",
        "--wait",
        "30",
    ]);
    for class in classes {
        argv.push("--class-names".into());
        argv.push(class.clone());
    }
    argv.push("--json".into());
    argv
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TestRun {
    pub org: String,
    pub outcome: String,
    pub ran: i64,
    pub passing: i64,
    pub failing: i64,
    pub skipped: i64,
    pub pass_rate: String,
    pub run_coverage: String,
    pub org_coverage: String,
    pub time_ms: i64,
    pub failures: Vec<TestFailure>,
    /// Lowest coverage first.
    pub coverage: Vec<ClassCoverage>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TestFailure {
    pub class: String,
    pub method: String,
    pub message: String,
    pub stack: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassCoverage {
    pub name: String,
    pub total_lines: i64,
    pub covered: i64,
    pub percent: f64,
}

pub fn parse_test_result(json: &Value, org: &str) -> TestRun {
    let result = &json["result"];
    let summary = &result["summary"];
    let failures = result["tests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| text(t, "Outcome") != "Pass")
        .map(|t| TestFailure {
            class: text(&t["ApexClass"], "Name"),
            method: text(t, "MethodName"),
            message: text(t, "Message"),
            stack: text(t, "StackTrace"),
        })
        .collect();
    let mut coverage: Vec<ClassCoverage> = result["coverage"]["coverage"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let total_lines = number(c, "totalLines");
            let covered = number(c, "totalCovered");
            let percent = c["coveredPercent"].as_f64().unwrap_or_else(|| {
                if total_lines > 0 {
                    covered as f64 * 100.0 / total_lines as f64
                } else {
                    0.0
                }
            });
            ClassCoverage {
                name: text(c, "name"),
                total_lines,
                covered,
                percent,
            }
        })
        .collect();
    coverage.sort_by(|a, b| a.percent.total_cmp(&b.percent).then_with(|| a.name.cmp(&b.name)));
    let time_ms = summary["testTotalTimeInMs"].as_i64().unwrap_or_else(|| {
        text(summary, "testTotalTime")
            .trim_end_matches(" ms")
            .parse()
            .unwrap_or_default()
    });
    TestRun {
        org: org.to_string(),
        outcome: text(summary, "outcome"),
        ran: number(summary, "testsRan"),
        passing: number(summary, "passing"),
        failing: number(summary, "failing"),
        skipped: number(summary, "skipped"),
        pass_rate: text(summary, "passRate"),
        run_coverage: text(summary, "testRunCoverage"),
        org_coverage: text(summary, "orgWideCoverage"),
        time_ms,
        failures,
        coverage,
    }
}
