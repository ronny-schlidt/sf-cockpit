//! Finds newer releases on GitHub and replaces the running binary with one.
//!
//! Works for a private and a public repository without code changes: `gh` first (needs a login that can
//! read the repository), then plain `curl` against the public GitHub URLs. When neither works, there is
//! simply no update to show. Downloads are checked against the `.sha256` file the release workflow attaches.

use crate::cache::Cache;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub const REPO: &str = "ronny-schlidt/sf-cockpit";
const CACHE_KEY: &str = "update";
/// How long a check result is reused before GitHub is asked again.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    /// Without the leading `v`.
    pub version: String,
    pub tag: String,
    /// Release notes as Markdown.
    pub notes: String,
    pub url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    Gh,
    Curl,
}

pub fn repo() -> String {
    std::env::var("SF_COCKPIT_REPO").unwrap_or_else(|_| REPO.into())
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Off with `SF_COCKPIT_NO_UPDATE_CHECK=1`.
pub fn check_enabled() -> bool {
    std::env::var_os("SF_COCKPIT_NO_UPDATE_CHECK").is_none_or(|v| v.is_empty() || v == "0")
}

/// `1.2.3` or `v1.2.3` as numbers; anything after `-` or `+` is ignored.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let version = (
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
        parts.next().flatten().unwrap_or(0),
    );
    Some(version)
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse_version(candidate), parse_version(current)), (Some(a), Some(b)) if a > b)
}

/// The Rust target triple the release workflow builds for this platform.
pub fn target_for(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => return None,
    })
}

pub fn archive_name(target: &str) -> String {
    let ext = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("sf-cockpit-{target}.{ext}")
}

pub fn build_latest_release(via: Via, repo: &str) -> Vec<String> {
    match via {
        Via::Gh => vec!["gh".into(), "api".into(), format!("repos/{repo}/releases/latest")],
        Via::Curl => vec![
            "curl".into(),
            "-fsSL".into(),
            "-H".into(),
            "Accept: application/vnd.github+json".into(),
            format!("https://api.github.com/repos/{repo}/releases/latest"),
        ],
    }
}

/// One command per file for curl, a single one for gh.
pub fn build_download(via: Via, repo: &str, tag: &str, archive: &str, dir: &Path) -> Vec<Vec<String>> {
    let dir_text = dir.display().to_string();
    match via {
        Via::Gh => vec![vec![
            "gh".into(),
            "release".into(),
            "download".into(),
            tag.into(),
            "--repo".into(),
            repo.into(),
            "--pattern".into(),
            archive.into(),
            "--pattern".into(),
            format!("{archive}.sha256"),
            "--dir".into(),
            dir_text,
            "--clobber".into(),
        ]],
        Via::Curl => [archive.to_string(), format!("{archive}.sha256")]
            .into_iter()
            .map(|file| {
                vec![
                    "curl".into(),
                    "-fsSL".into(),
                    "-o".into(),
                    dir.join(&file).display().to_string(),
                    format!("https://github.com/{repo}/releases/download/{tag}/{file}"),
                ]
            })
            .collect(),
    }
}

pub fn parse_release(json: &Value) -> Option<ReleaseInfo> {
    let tag = json["tag_name"].as_str()?.to_string();
    parse_version(&tag)?;
    Some(ReleaseInfo {
        version: tag.trim_start_matches('v').to_string(),
        notes: json["body"].as_str().unwrap_or_default().replace("\r\n", "\n"),
        url: json["html_url"].as_str().unwrap_or_default().to_string(),
        tag,
    })
}

fn run(argv: &[String]) -> Result<Vec<u8>> {
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("could not run `{}`", argv[0]))?;
    if !output.status.success() {
        bail!(
            "`{}` failed: {}",
            argv[..2.min(argv.len())].join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

/// The latest release on GitHub, and the way that reached it.
pub fn fetch_latest() -> Result<(ReleaseInfo, Via)> {
    let repo = repo();
    let mut errors = Vec::new();
    for via in [Via::Gh, Via::Curl] {
        match run(&build_latest_release(via, &repo)).and_then(|out| {
            let json: Value = serde_json::from_slice(&out)?;
            parse_release(&json).ok_or_else(|| anyhow!("no release found"))
        }) {
            Ok(release) => return Ok((release, via)),
            Err(error) => errors.push(format!("{error:#}")),
        }
    }
    bail!(
        "cannot read the releases of {repo} (gh auth login?): {}",
        errors.join("; ")
    )
}

/// A newer release than this binary, if any. Asks GitHub at most once a day and remembers the answer.
pub fn check(cache: Option<&Cache>) -> Option<ReleaseInfo> {
    let cached = cache.and_then(|c| c.load::<Option<ReleaseInfo>>(CACHE_KEY));
    let latest = match cached {
        Some((latest, saved_at))
            if (chrono::Local::now() - saved_at)
                .to_std()
                .is_ok_and(|age| age < CHECK_EVERY) =>
        {
            latest
        }
        _ => {
            let latest = fetch_latest().ok().map(|(release, _)| release);
            if let Some(cache) = cache {
                let _ = cache.save(CACHE_KEY, &latest);
            }
            latest
        }
    };
    latest.filter(|release| is_newer(&release.version, current_version()))
}

/// The cached latest release, even when it is not newer, for "what's new" after an update.
pub fn cached_release(cache: &Cache) -> Option<ReleaseInfo> {
    cache.load::<Option<ReleaseInfo>>(CACHE_KEY)?.0
}

/// Downloads `release` and replaces the running binary with it. Returns the path that was replaced.
pub fn install(release: &ReleaseInfo) -> Result<PathBuf> {
    let target = target_for(std::env::consts::OS, std::env::consts::ARCH)
        .context("no prebuilt binary for this platform, build from source with install.sh")?;
    let archive = archive_name(target);
    let repo = repo();
    let dir = std::env::temp_dir().join(format!("sf-cockpit-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let result = (|| {
        let mut errors = Vec::new();
        let downloaded = [Via::Gh, Via::Curl].into_iter().any(|via| {
            let ok = build_download(via, &repo, &release.tag, &archive, &dir)
                .iter()
                .try_for_each(|argv| run(argv).map(drop));
            if let Err(error) = &ok {
                errors.push(format!("{error:#}"));
            }
            ok.is_ok()
        });
        if !downloaded {
            bail!("download failed: {}", errors.join("; "));
        }

        let bytes = std::fs::read(dir.join(&archive))?;
        let expected = std::fs::read_to_string(dir.join(format!("{archive}.sha256")))?;
        let expected = expected
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_lowercase();
        let actual: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if expected != actual {
            bail!("checksum mismatch, refusing to install");
        }

        let tar = if cfg!(windows) { "tar.exe" } else { "tar" };
        let unpacked = Command::new(tar)
            .arg("-xf")
            .arg(dir.join(&archive))
            .arg("-C")
            .arg(&dir)
            .stdin(Stdio::null())
            .output()
            .context("could not run tar")?;
        if !unpacked.status.success() {
            bail!(
                "cannot unpack {archive}: {}",
                String::from_utf8_lossy(&unpacked.stderr).trim()
            );
        }
        let exe_name = if cfg!(windows) {
            "sf-cockpit.exe"
        } else {
            "sf-cockpit"
        };
        replace_current_exe(&dir.join(format!("sf-cockpit-{target}")).join(exe_name))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Copies next to the running binary first, so the final rename stays on one file system and is atomic.
fn replace_current_exe(new: &Path) -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let staged = exe.with_extension("new");
    std::fs::copy(new, &staged)
        .with_context(|| format!("cannot write next to {}, is it writable?", exe.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    if cfg!(target_os = "macos") {
        let _ = Command::new("xattr")
            .args(["-d", "com.apple.quarantine"])
            .arg(&staged)
            .stderr(Stdio::null())
            .status();
    }
    if cfg!(windows) {
        // A running .exe cannot be overwritten, but it can be renamed away.
        let old = exe.with_extension("old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old)?;
    }
    std::fs::rename(&staged, &exe).with_context(|| format!("cannot replace {}", exe.display()))?;
    Ok(exe)
}

/// Removes what a Windows update left behind.
pub fn clean_up_old_binary() {
    if cfg!(windows)
        && let Ok(exe) = std::env::current_exe()
    {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}

/// `--check-update` and `--update` on the command line.
pub fn run_cli(install_it: bool) -> Result<()> {
    let current = current_version();
    let (release, _) = fetch_latest()?;
    if !is_newer(&release.version, current) {
        println!("sf-cockpit {current} is up to date.");
        return Ok(());
    }
    println!(
        "sf-cockpit {} is available (installed: {current}).",
        release.version
    );
    if !release.notes.trim().is_empty() {
        println!("\n{}\n", release.notes.trim());
    }
    if !install_it {
        println!("Update with: sf-cockpit --update");
        std::process::exit(10);
    }
    let path = install(&release)?;
    if let Some(cache) = Cache::default_location() {
        let _ = cache.save(CACHE_KEY, &Some(release.clone()));
    }
    println!("Updated {} to {}.", path.display(), release.version);
    Ok(())
}
