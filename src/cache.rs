//! Last loaded data on disk, so every tab shows something the moment the app starts.
//! Only parsed data is stored (org names, usernames, org ids), never tokens or raw CLI output.

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Bump when a cached struct changes shape; older files are then ignored.
const FORMAT: u32 = 1;

#[derive(Clone, Debug)]
pub struct Cache {
    dir: PathBuf,
}

#[derive(Serialize)]
struct EntryOut<'a, T> {
    format: u32,
    saved_at: DateTime<Local>,
    data: &'a T,
}

#[derive(Deserialize)]
struct EntryIn<T> {
    format: u32,
    saved_at: DateTime<Local>,
    data: T,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// `$XDG_CACHE_HOME/sf-cockpit`, `~/Library/Caches/sf-cockpit` on macOS, else `~/.cache/sf-cockpit`.
    pub fn default_location() -> Option<Self> {
        let base = match std::env::var_os("XDG_CACHE_HOME") {
            Some(dir) => PathBuf::from(dir),
            None if cfg!(target_os = "macos") => std::env::home_dir()?.join("Library/Caches"),
            None => std::env::home_dir()?.join(".cache"),
        };
        Some(Self::new(base.join("sf-cockpit")))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A file-name-safe key from its parts.
    pub fn key(parts: &[&str]) -> String {
        parts
            .join("-")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    pub fn load<T: DeserializeOwned>(&self, key: &str) -> Option<(T, DateTime<Local>)> {
        let text = std::fs::read_to_string(self.path(key)).ok()?;
        let entry: EntryIn<T> = serde_json::from_str(&text).ok()?;
        (entry.format == FORMAT).then_some((entry.data, entry.saved_at))
    }

    pub fn save<T: Serialize>(&self, key: &str, data: &T) -> Result<()> {
        create_private_dir(&self.dir)?;
        let json = serde_json::to_vec(&EntryOut {
            format: FORMAT,
            saved_at: Local::now(),
            data,
        })?;
        let tmp = self.dir.join(format!("{key}.json.tmp"));
        write_private(&tmp, &json)?;
        std::fs::rename(&tmp, self.path(key)).with_context(|| format!("cannot write cache {key}"))
    }

    /// Number of entries and their total size in bytes.
    pub fn summary(&self) -> (usize, u64) {
        self.entries()
            .filter_map(|path| path.metadata().ok())
            .fold((0, 0), |(count, bytes), meta| (count + 1, bytes + meta.len()))
    }

    pub fn clear(&self) -> Result<()> {
        for path in self.entries() {
            std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
        }
        Ok(())
    }

    fn entries(&self) -> impl Iterator<Item = PathBuf> {
        std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
    }
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if dir.is_dir() {
        return Ok(());
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("cannot create {}", dir.display()))
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("cannot write {}", path.display()))?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}
