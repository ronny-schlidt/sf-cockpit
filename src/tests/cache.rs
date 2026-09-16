use super::temp_dir;
use crate::cache::Cache;
use crate::config::{Config, FileConfig, Origin, load_from, save_value};
use crate::demo;
use crate::sf::push::PushData;

#[test]
fn cache_roundtrip_and_clear() {
    let dir = temp_dir("cache");
    let cache = Cache::new(dir.join("nested"));
    let key = Cache::key(&["push", "DevHub", "Acme Invoicing"]);
    assert_eq!(key, "push-DevHub-Acme_Invoicing");
    assert!(cache.load::<PushData>(&key).is_none());

    cache.save(&key, &demo::push()).unwrap();
    let (data, saved_at) = cache.load::<PushData>(&key).expect("cached");
    assert_eq!(data.requests.len(), 3);
    assert_eq!(data.jobs_for("0DV000000000001").len(), 5);
    assert!(data.requests[0].start.is_some(), "timestamps survive");
    assert!((chrono::Local::now() - saved_at).num_seconds() < 5);
    assert_eq!(cache.summary().0, 1);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("nested").join(format!("{key}.json")))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "cache files are private");
    }

    cache.clear().unwrap();
    assert!(cache.load::<PushData>(&key).is_none());
    assert_eq!(cache.summary(), (0, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn old_or_broken_cache_files_are_ignored() {
    let dir = temp_dir("cache-broken");
    let cache = Cache::new(dir.clone());
    std::fs::write(dir.join("orgs.json"), "{not json").unwrap();
    assert!(cache.load::<Vec<String>>("orgs").is_none());
    std::fs::write(
        dir.join("orgs.json"),
        r#"{"format": 999, "saved_at": "2026-09-16T10:00:00+02:00", "data": ["a"]}"#,
    )
    .unwrap();
    assert!(cache.load::<Vec<String>>("orgs").is_none());
    std::fs::write(
        dir.join("orgs.json"),
        r#"{"format": 1, "saved_at": "2026-09-16T10:00:00+02:00", "data": {"wrong": "shape"}}"#,
    )
    .unwrap();
    assert!(cache.load::<Vec<String>>("orgs").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saving_a_setting_keeps_comments_and_other_keys() {
    let dir = temp_dir("save");
    let path = dir.join("sf-cockpit.toml");
    std::fs::write(
        &path,
        "# Configuration for sf-cockpit\ndev_hub = \"DevHub\"\nscratch_org = \"scratchOrg\"\n",
    )
    .unwrap();

    save_value(&path, "scratch_org", "other").unwrap();
    save_value(&path, "limit", 50).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.starts_with("# Configuration for sf-cockpit\ndev_hub = \"DevHub\"\n"),
        "{text}"
    );
    assert!(text.contains("scratch_org = \"other\""), "{text}");
    assert!(text.contains("limit = 50"), "{text}");
    let parsed = FileConfig::parse(&text).unwrap();
    assert_eq!(parsed.limit, Some(50));

    let global = dir.join("xdg/sf-cockpit/config.toml");
    save_value(&global, "dev_hub", "Hub").unwrap();
    assert_eq!(std::fs::read_to_string(&global).unwrap(), "dev_hub = \"Hub\"\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn origins_and_save_path() {
    let dir = temp_dir("origins");
    let project = dir.join("project");
    let home = dir.join("home");
    let xdg = dir.join("xdg");
    std::fs::create_dir_all(project.join("force-app")).unwrap();
    std::fs::create_dir_all(home.join(".sf")).unwrap();
    std::fs::write(project.join("sf-cockpit.toml"), "scratch_org = \"so\"\n").unwrap();
    std::fs::write(home.join(".sf/config.json"), r#"{"target-dev-hub": "Hub"}"#).unwrap();

    let cli = FileConfig {
        limit: Some(5),
        ..Default::default()
    };
    let config: Config = load_from(cli, &project.join("force-app"), Some(&home), Some(&xdg)).unwrap();
    assert_eq!(config.origin("dev_hub"), Origin::SfConfig);
    assert_eq!(
        config.origin("scratch_org"),
        Origin::Project(project.join("sf-cockpit.toml"))
    );
    assert_eq!(config.origin("limit"), Origin::Flag);
    assert_eq!(config.origin("source_dir"), Origin::Default);
    assert_eq!(
        config.save_path.as_deref(),
        Some(project.join("sf-cockpit.toml").as_path())
    );
    assert!(matches!(config.save_origin(), Some(Origin::Project(_))));

    let outside = load_from(FileConfig::default(), &home, Some(&home), Some(&xdg)).unwrap();
    assert_eq!(
        outside.save_path.as_deref(),
        Some(xdg.join("sf-cockpit/config.toml").as_path())
    );
    assert!(matches!(outside.save_origin(), Some(Origin::Global(_))));
    let _ = std::fs::remove_dir_all(&dir);
}
