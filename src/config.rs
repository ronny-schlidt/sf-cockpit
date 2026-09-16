//! Configuration, highest precedence first: CLI flags, `sf-cockpit.toml` in the project (found by walking up
//! from the current directory), `~/.config/sf-cockpit/config.toml`, the sf CLI config, `sfdx-project.json`.

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const PROJECT_FILE: &str = "sf-cockpit.toml";
const GLOBAL_FILE: &str = "sf-cockpit/config.toml";

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    /// Alias or username of the Dev Hub that owns the package.
    pub dev_hub: Option<String>,
    /// Package name or 0Ho id.
    pub package: Option<String>,
    /// Alias of the org used for deployments and installs by default.
    pub scratch_org: Option<String>,
    /// Salesforce project directory, relative to the config file.
    pub project_dir: Option<PathBuf>,
    /// Source directory to deploy, relative to the project directory.
    pub source_dir: Option<String>,
    /// Scratch org definition used when creating package versions.
    pub definition_file: Option<String>,
    pub skip_ancestor_check: Option<bool>,
    /// Number of recent push requests to load.
    pub limit: Option<usize>,
}

impl FileConfig {
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).map_err(|e| anyhow!("{e}"))
    }

    fn read(path: &Path) -> Result<Option<Self>> {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Ok(None);
        };
        let mut config = Self::parse(&text).with_context(|| format!("invalid {}", path.display()))?;
        if let (Some(dir), Some(base)) = (config.project_dir.take(), path.parent()) {
            config.project_dir = Some(base.join(dir));
        }
        Ok(Some(config))
    }

    /// Fields set in `over` win.
    pub fn merge(self, over: Self) -> Self {
        Self {
            dev_hub: over.dev_hub.or(self.dev_hub),
            package: over.package.or(self.package),
            scratch_org: over.scratch_org.or(self.scratch_org),
            project_dir: over.project_dir.or(self.project_dir),
            source_dir: over.source_dir.or(self.source_dir),
            definition_file: over.definition_file.or(self.definition_file),
            skip_ancestor_check: over.skip_ancestor_check.or(self.skip_ancestor_check),
            limit: over.limit.or(self.limit),
        }
    }

    fn has(&self, key: &str) -> bool {
        match key {
            "dev_hub" => self.dev_hub.is_some(),
            "package" => self.package.is_some(),
            "scratch_org" => self.scratch_org.is_some(),
            "project_dir" => self.project_dir.is_some(),
            "source_dir" => self.source_dir.is_some(),
            "definition_file" => self.definition_file.is_some(),
            "skip_ancestor_check" => self.skip_ancestor_check.is_some(),
            "limit" => self.limit.is_some(),
            _ => false,
        }
    }
}

const KEYS: [&str; 8] = [
    "dev_hub",
    "package",
    "scratch_org",
    "project_dir",
    "source_dir",
    "definition_file",
    "skip_ancestor_check",
    "limit",
];

/// Where a setting came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    Flag,
    Project(PathBuf),
    Global(PathBuf),
    SfConfig,
    SfdxProject,
    Default,
}

impl Origin {
    pub fn label(&self) -> String {
        match self {
            Origin::Flag => "command-line flag".into(),
            Origin::Project(_) => format!("{PROJECT_FILE} (project)"),
            Origin::Global(path) => format!("{} (global)", tilde(path)),
            Origin::SfConfig => "sf CLI config".into(),
            Origin::SfdxProject => "sfdx-project.json".into(),
            Origin::Default => "default".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub dev_hub: String,
    pub package: Option<String>,
    pub scratch_org: Option<String>,
    pub project_dir: Option<PathBuf>,
    pub source_dir: String,
    pub definition_file: String,
    pub skip_ancestor_check: bool,
    pub limit: usize,
    pub origins: HashMap<&'static str, Origin>,
    /// The file the Settings tab writes: the project file if there is one, else the global file.
    pub save_path: Option<PathBuf>,
    /// Values from command-line flags; they win over every file.
    pub cli: FileConfig,
}

impl Config {
    pub fn demo() -> Self {
        Self {
            dev_hub: "demo".into(),
            package: Some("Demo Package".into()),
            scratch_org: Some("scratch".into()),
            project_dir: Some(PathBuf::from(".")),
            source_dir: "force-app".into(),
            definition_file: "config/project-scratch-def.json".into(),
            skip_ancestor_check: false,
            limit: 30,
            origins: HashMap::new(),
            save_path: None,
            cli: FileConfig::default(),
        }
    }

    /// No Dev Hub yet: the app opens the Settings tab, `--print` stops with a hint.
    pub fn needs_setup(&self) -> bool {
        self.dev_hub.is_empty()
    }

    pub fn origin(&self, key: &str) -> Origin {
        self.origins.get(key).cloned().unwrap_or(Origin::Default)
    }

    /// The origin a value gets once it is written to `save_path`.
    pub fn save_origin(&self) -> Option<Origin> {
        let path = self.save_path.clone()?;
        Some(if path.file_name().is_some_and(|name| name == PROJECT_FILE) {
            Origin::Project(path)
        } else {
            Origin::Global(path)
        })
    }
}

pub fn load(cli: FileConfig) -> Result<Config> {
    let cwd = std::env::current_dir().context("cannot read the current directory")?;
    let home = std::env::home_dir();
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    load_from(cli, &cwd, home.as_deref(), xdg.as_deref())
}

pub fn load_from(cli: FileConfig, cwd: &Path, home: Option<&Path>, xdg: Option<&Path>) -> Result<Config> {
    let project_file = find_up(cwd, PROJECT_FILE);
    let global_file = xdg
        .map(Path::to_path_buf)
        .or_else(|| home.map(|h| h.join(".config")))
        .map(|dir| dir.join(GLOBAL_FILE));
    let read = |path: &Option<PathBuf>| -> Result<FileConfig> {
        match path {
            Some(path) => Ok(FileConfig::read(path)?.unwrap_or_default()),
            None => Ok(FileConfig::default()),
        }
    };
    let project = read(&project_file)?;
    let global = read(&global_file)?;
    let sf = sf_config(cwd, home);

    // Lowest precedence first.
    let layers = [
        (&sf, Origin::SfConfig),
        (&global, Origin::Global(global_file.clone().unwrap_or_default())),
        (
            &project,
            Origin::Project(project_file.clone().unwrap_or_default()),
        ),
        (&cli, Origin::Flag),
    ];
    let mut origins: HashMap<&'static str, Origin> = HashMap::new();
    for key in KEYS {
        if let Some((_, origin)) = layers.iter().rev().find(|(layer, _)| layer.has(key)) {
            origins.insert(key, origin.clone());
        }
    }

    let mut merged = sf.clone().merge(global).merge(project).merge(cli.clone());
    if merged.project_dir.is_none() {
        if let Some(dir) = project_file.as_deref().and_then(Path::parent) {
            merged.project_dir = Some(dir.to_path_buf());
            origins.insert(
                "project_dir",
                Origin::Project(project_file.clone().unwrap_or_default()),
            );
        } else if let Some(dir) =
            find_up(cwd, "sfdx-project.json").and_then(|p| p.parent().map(Path::to_path_buf))
        {
            merged.project_dir = Some(dir);
            origins.insert("project_dir", Origin::SfdxProject);
        }
    }
    if merged.package.is_none()
        && let Some(dir) = &merged.project_dir
    {
        merged.package = package_from_sfdx_project(&dir.join("sfdx-project.json"));
        if merged.package.is_some() {
            origins.insert("package", Origin::SfdxProject);
        }
    }

    // Inside a Salesforce project without sf-cockpit.toml, settings create one next to sfdx-project.json.
    let save_path = project_file
        .or_else(|| find_up(cwd, "sfdx-project.json").and_then(|p| p.parent().map(|d| d.join(PROJECT_FILE))))
        .or(global_file);
    Ok(Config {
        dev_hub: merged.dev_hub.unwrap_or_default(),
        package: merged.package,
        scratch_org: merged.scratch_org,
        project_dir: merged.project_dir,
        source_dir: merged.source_dir.unwrap_or_else(|| "force-app".into()),
        definition_file: merged
            .definition_file
            .unwrap_or_else(|| "config/project-scratch-def.json".into()),
        skip_ancestor_check: merged.skip_ancestor_check.unwrap_or(false),
        limit: merged.limit.unwrap_or(30),
        origins,
        save_path,
        cli,
    })
}

/// Sets one key in a TOML config file and keeps comments, order and all other keys.
pub fn save_value(path: &Path, key: &str, value: impl Into<toml_edit::Value>) -> Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("invalid {}", path.display()))?;
    doc[key] = toml_edit::value(value);
    let updated = doc.to_string();
    FileConfig::parse(&updated)
        .with_context(|| format!("refusing to write an invalid {}", path.display()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    std::fs::write(path, updated).with_context(|| format!("cannot write {}", path.display()))
}

/// `~/…` instead of the home directory, for display.
pub fn tilde(path: &Path) -> String {
    match std::env::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// `target-dev-hub` and `target-org` from the sf CLI: the project config first, then the global one.
fn sf_config(cwd: &Path, home: Option<&Path>) -> FileConfig {
    let project = find_up(cwd, ".sf/config.json").and_then(|p| read_json(&p));
    let global = home.and_then(|h| read_json(&h.join(".sf/config.json")));
    let pick = |key: &str| {
        [&project, &global]
            .into_iter()
            .flatten()
            .find_map(|json| json[key].as_str().filter(|s| !s.is_empty()).map(str::to_string))
    };
    FileConfig {
        dev_hub: pick("target-dev-hub"),
        scratch_org: pick("target-org"),
        ..Default::default()
    }
}

/// The 0Ho id of the first package alias in `sfdx-project.json`.
fn package_from_sfdx_project(path: &Path) -> Option<String> {
    let json = read_json(path)?;
    json["packageAliases"]
        .as_object()?
        .iter()
        .find_map(|(key, value)| {
            let id = value.as_str()?;
            (!key.contains('@') && id.starts_with("0Ho")).then(|| id.to_string())
        })
}

fn find_up(start: &Path, name: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}
