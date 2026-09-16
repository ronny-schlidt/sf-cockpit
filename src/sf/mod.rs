//! Everything that talks to the Salesforce CLI. Every command is built by a pure `build_*` function that
//! returns the argv, so tests can check commands without running them.

pub mod deploy;
pub mod orgs;
pub mod push;
pub mod query;
pub mod runner;
pub mod versions;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

use deploy::DeployRecord;
use orgs::OrgInfo;
use push::{PackageIds, PushData};
use runner::TaskId;
use versions::PackageVersion;

/// Results of background work, delivered to the app through a channel. `key` is the cache key of the
/// settings the load was started with, so results for an old Dev Hub or package are dropped.
pub enum Msg {
    PushLoaded {
        key: String,
        result: Result<Box<PushData>, String>,
    },
    OrgsLoaded(Result<Vec<OrgInfo>, String>),
    InstalledLoaded {
        username: String,
        result: Result<Option<String>, String>,
    },
    VersionsLoaded {
        key: String,
        result: Result<Vec<PackageVersion>, String>,
    },
    PackagesLoaded {
        hub: String,
        result: Result<Vec<PackageIds>, String>,
    },
    DeploysLoaded {
        org: String,
        result: Result<Vec<DeployRecord>, String>,
    },
    TaskLine {
        id: TaskId,
        line: String,
        stderr: bool,
    },
    TaskDone {
        id: TaskId,
        exit: Option<i32>,
        json: Option<Value>,
        stdout: String,
        stderr: String,
    },
}

pub fn program() -> &'static str {
    // On Windows the Salesforce CLI is a .cmd shim, which Command does not resolve on its own.
    if cfg!(windows) { "sf.cmd" } else { "sf" }
}

pub fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| part.to_string()).collect()
}

/// A `sf` command with plain, non-interactive output.
pub fn command(argv: &[String], cwd: Option<&Path>) -> Command {
    let mut cmd = Command::new(program());
    cmd.args(argv)
        .env("CI", "1")
        .env("SF_USE_PROGRESS_BAR", "false")
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .env("TERM", "dumb")
        .stdin(Stdio::null());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd
}

/// Runs a `--json` command to completion and returns its JSON, or the CLI's error message.
pub fn run_json(argv: &[String], cwd: Option<&Path>) -> Result<Value> {
    let output = command(argv, cwd)
        .output()
        .context("could not run `sf`, is the Salesforce CLI installed and on PATH?")?;
    let json = parse_json(&String::from_utf8_lossy(&output.stdout)).with_context(|| {
        format!(
            "unexpected output from `sf {}`: {}",
            argv.iter().take(3).cloned().collect::<Vec<_>>().join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })?;
    if json["status"].as_i64() != Some(0) {
        bail!("{}", error_message(&json));
    }
    Ok(json)
}

/// Parses the JSON that `sf --json` prints, ignoring progress output and warnings around it.
pub fn parse_json(stdout: &str) -> Result<Value> {
    let text = strip_control(stdout);
    let start = text.find('{').context("no JSON in output")?;
    serde_json::Deserializer::from_str(&text[start..])
        .into_iter::<Value>()
        .next()
        .context("no JSON in output")?
        .map_err(Into::into)
}

pub fn error_message(json: &Value) -> String {
    json["message"]
        .as_str()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .or_else(|| json["name"].as_str())
        .unwrap_or("sf command failed")
        .to_string()
}

/// Removes ANSI escape sequences and control characters (progress bars, spinners, colors).
pub fn strip_control(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    let mut previous = '\0';
                    for c in chars.by_ref() {
                        if c == '\x07' || (previous == '\x1b' && c == '\\') {
                            break;
                        }
                        previous = c;
                    }
                }
                _ => {}
            },
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}
