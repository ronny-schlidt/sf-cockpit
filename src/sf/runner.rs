//! Runs long `sf` commands in the background and streams their output line by line.

use super::{Msg, command, parse_json, strip_control};
use anyhow::{Context, Result};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub type TaskId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    Schedule,
    Abort,
    Promote,
    Install,
    CreateVersion,
    DeleteScratch,
    Deploy,
    Tests,
    Open,
}

impl TaskKind {
    /// Tasks that take long enough to show their log while they run.
    pub fn shows_log(self) -> bool {
        !matches!(self, TaskKind::Open)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskSpec {
    pub title: String,
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub kind: TaskKind,
    pub parse_json: bool,
}

pub struct TaskHandle {
    child: Option<Arc<Mutex<Child>>>,
}

impl TaskHandle {
    /// Best effort: on Windows this kills the `sf.cmd` shim, not necessarily node.
    pub fn cancel(&self) {
        if let Some(child) = &self.child
            && let Ok(mut child) = child.lock()
        {
            let _ = child.kill();
        }
    }
}

pub fn spawn(id: TaskId, spec: &TaskSpec, tx: Sender<Msg>) -> Result<TaskHandle> {
    let mut child = command(&spec.argv, spec.cwd.as_deref())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not run `sf`, is the Salesforce CLI installed and on PATH?")?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let child = Arc::new(Mutex::new(child));

    let out = {
        let tx = tx.clone();
        thread::spawn(move || read_lines(stdout, id, false, &tx))
    };
    let err = {
        let tx = tx.clone();
        thread::spawn(move || read_lines(stderr, id, true, &tx))
    };
    let waiter = Arc::clone(&child);
    let parse = spec.parse_json;
    thread::spawn(move || {
        let stdout = out.join().unwrap_or_default();
        let stderr = err.join().unwrap_or_default();
        let exit = wait(&waiter);
        let json = if parse { parse_json(&stdout).ok() } else { None };
        let _ = tx.send(Msg::TaskDone {
            id,
            exit,
            json,
            stdout,
            stderr,
        });
    });
    Ok(TaskHandle { child: Some(child) })
}

/// A fake task for demo mode and tests: prints the lines, then finishes successfully with `result`.
pub fn spawn_demo(
    id: TaskId,
    lines: Vec<String>,
    result: Value,
    delay: Duration,
    tx: Sender<Msg>,
) -> TaskHandle {
    thread::spawn(move || {
        for line in lines {
            thread::sleep(delay);
            let _ = tx.send(Msg::TaskLine {
                id,
                line,
                stderr: false,
            });
        }
        thread::sleep(delay);
        let _ = tx.send(Msg::TaskDone {
            id,
            exit: Some(0),
            json: Some(result),
            stdout: String::new(),
            stderr: String::new(),
        });
    });
    TaskHandle { child: None }
}

/// Polls instead of blocking in `wait()`, so `cancel()` can take the lock in between.
fn wait(child: &Mutex<Child>) -> Option<i32> {
    loop {
        if let Ok(mut child) = child.lock() {
            match child.try_wait() {
                Ok(Some(status)) => return status.code(),
                Ok(None) => {}
                Err(_) => return None,
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Forwards each output line as it arrives (`\r` redraws count as lines) and returns the whole output.
fn read_lines(mut stream: impl Read, id: TaskId, stderr: bool, tx: &Sender<Msg>) -> String {
    let mut all = String::new();
    let mut pending = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = stream.read(&mut buf)
        && n > 0
    {
        for &byte in &buf[..n] {
            if byte == b'\n' || byte == b'\r' {
                flush(&mut pending, &mut all, id, stderr, tx);
            } else {
                pending.push(byte);
            }
        }
    }
    flush(&mut pending, &mut all, id, stderr, tx);
    all
}

fn flush(pending: &mut Vec<u8>, all: &mut String, id: TaskId, stderr: bool, tx: &Sender<Msg>) {
    if pending.is_empty() {
        return;
    }
    let raw = String::from_utf8_lossy(pending).into_owned();
    pending.clear();
    all.push_str(&raw);
    all.push('\n');
    let line = strip_control(&raw);
    if !line.trim().is_empty() {
        let _ = tx.send(Msg::TaskLine { id, line, stderr });
    }
}
