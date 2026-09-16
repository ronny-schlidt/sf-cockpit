//! SOQL through `sf data query` and helpers to read the records it returns.

use super::run_json;
use anyhow::Result;
use chrono::{DateTime, FixedOffset};
use serde_json::Value;

pub type Timestamp = DateTime<FixedOffset>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    Data,
    Tooling,
}

pub fn build_query(org: &str, soql: &str, api: Api) -> Vec<String> {
    let mut argv = super::argv(&["data", "query", "--json", "--target-org", org, "--query", soql]);
    if api == Api::Tooling {
        argv.push("--use-tooling-api".into());
    }
    argv
}

pub fn query(org: &str, soql: &str, api: Api) -> Result<Vec<Value>> {
    let json = run_json(&build_query(org, soql, api), None)?;
    Ok(json["result"]["records"].as_array().cloned().unwrap_or_default())
}

/// A SOQL string literal with quotes and backslashes escaped.
pub fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// `'a','b'` for an `IN (...)` clause; ids that are not plain alphanumerics are dropped.
pub fn id_list<'a>(ids: impl IntoIterator<Item = &'a str>) -> String {
    ids.into_iter()
        .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(|id| format!("'{id}'"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The 15-character form of an org id, as used by `PackagePushJob` and `PackageSubscriber`.
pub fn org_key(id: &str) -> String {
    id.chars().take(15).collect()
}

pub fn text(record: &Value, field: &str) -> String {
    match &record[field] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

pub fn number(record: &Value, field: &str) -> i64 {
    record[field]
        .as_i64()
        .or_else(|| record[field].as_f64().map(|f| f as i64))
        .unwrap_or_default()
}

pub fn flag(record: &Value, field: &str) -> bool {
    record[field].as_bool().unwrap_or_default()
}

pub fn time(record: &Value, field: &str) -> Option<Timestamp> {
    let value = record[field].as_str()?;
    DateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f%z")
        .or_else(|_| DateTime::parse_from_rfc3339(value))
        .ok()
}
