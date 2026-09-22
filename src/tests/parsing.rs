use crate::config::{FileConfig, OrgNote, load_from, save_org_note};
use crate::sf::deploy::{build_deploy, build_tests, parse_deploys, parse_test_result};
use crate::sf::orgs::{Health, OrgKind, build_delete_scratch, parse_installed, parse_org_list};
use crate::sf::push::{build_abort, build_schedule};
use crate::sf::query::{Api, build_query, id_list, literal};
use crate::sf::versions::{build_create, build_install, build_promote, install_url, parse_list};
use crate::sf::{parse_json, strip_control};
use crate::ui::modal::shell_join;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

#[test]
fn push_commands() {
    let orgs = strings(&["00DA", "00DB"]);
    assert_eq!(
        build_schedule("DevHub", "04tX", &orgs, None),
        strings(&[
            "package",
            "push-upgrade",
            "schedule",
            "--target-dev-hub",
            "DevHub",
            "--package",
            "04tX",
            "--org-list",
            "00DA,00DB",
            "--json",
        ])
    );
    assert_eq!(
        build_schedule("DevHub", "04tX", &orgs, Some("2026-12-06T21:00:00"))[9..],
        strings(&["--start-time", "2026-12-06T21:00:00", "--json"])[..]
    );
    assert_eq!(
        build_abort("DevHub", "0DVX"),
        strings(&[
            "package",
            "push-upgrade",
            "abort",
            "--target-dev-hub",
            "DevHub",
            "--push-request-id",
            "0DVX",
            "--json"
        ])
    );
}

#[test]
fn version_org_and_deploy_commands() {
    assert_eq!(
        build_promote("DevHub", "04tX"),
        strings(&[
            "package",
            "version",
            "promote",
            "--package",
            "04tX",
            "--target-dev-hub",
            "DevHub",
            "--no-prompt",
            "--json"
        ])
    );
    assert_eq!(
        build_install("04tX", "scratchOrg"),
        strings(&[
            "package",
            "install",
            "--package",
            "04tX",
            "--target-org",
            "scratchOrg",
            "--wait",
            "30",
            "--publish-wait",
            "30",
            "--no-prompt",
            "--json",
        ])
    );
    let create = build_create("DevHub", "0HoX", "config/def.json", true);
    assert!(create.contains(&"--skip-ancestor-check".to_string()));
    assert!(
        create
            .windows(2)
            .any(|w| w == ["--definition-file", "config/def.json"])
    );
    assert!(!build_create("DevHub", "0HoX", "d.json", false).contains(&"--skip-ancestor-check".to_string()));
    assert_eq!(
        build_delete_scratch("so"),
        strings(&[
            "org",
            "delete",
            "scratch",
            "--target-org",
            "so",
            "--no-prompt",
            "--json"
        ])
    );
    assert_eq!(
        build_deploy("force-app", "so"),
        strings(&[
            "project",
            "deploy",
            "start",
            "--source-dir",
            "force-app",
            "--target-org",
            "so",
            "--ignore-conflicts",
            "--wait",
            "30"
        ])
    );
    assert_eq!(build_tests("so", &[]).last().unwrap(), "--json");
    assert!(!build_tests("so", &[]).contains(&"--class-names".to_string()));
    assert_eq!(
        build_query("DevHub", "SELECT Id FROM Package2", Api::Tooling)
            .last()
            .unwrap(),
        "--use-tooling-api"
    );
    assert!(
        !build_query("DevHub", "SELECT Id FROM PackagePushJob", Api::Data)
            .contains(&"--use-tooling-api".to_string())
    );
    assert_eq!(
        install_url("04tX", true),
        "https://test.salesforce.com/packaging/installPackage.apexp?p0=04tX"
    );
}

#[test]
fn soql_values_are_escaped() {
    assert_eq!(literal("acme's \\ pkg"), "'acme\\'s \\\\ pkg'");
    assert_eq!(id_list(["0DV1", "x' OR Id != '", "", "0DV2"]), "'0DV1','0DV2'");
}

#[test]
fn shown_commands_can_be_pasted_into_a_shell() {
    assert_eq!(
        shell_join(&strings(&[
            "package",
            "version",
            "list",
            "--packages",
            "My Package",
            "it's"
        ])),
        "package version list --packages 'My Package' 'it'\\''s'"
    );
}

#[test]
fn sf_output_is_cleaned_before_parsing() {
    assert_eq!(strip_control("\x1b[32mhello\x1b[0m\r\nworld\x07"), "hello\nworld");
    assert_eq!(strip_control("\x1b]0;title\x07ok"), "ok");
    let json = parse_json("Warning: update available\n\x1b[2K{\"status\":0,\"result\":{\"a\":1}}\ntrailing")
        .unwrap();
    assert_eq!(json["result"]["a"], 1);
    assert!(parse_json("no json here").is_err());
}

#[test]
fn org_list_is_flattened_and_deduplicated() {
    let json = json!({"status": 0, "result": {
        "devHubs": [{"username": "hub@x.com", "alias": "hub", "orgId": "00DA00000000001AAA", "isDevHub": true,
                     "connectedStatus": "Connected", "isDefaultDevHubUsername": true,
                     "instanceUrl": "https://hub.my.salesforce.com"}],
        "nonScratchOrgs": [
            {"username": "hub@x.com", "alias": "hub", "orgId": "00DA00000000001AAA", "isDevHub": true,
             "connectedStatus": "Connected"},
            {"username": "sb@x.com.uat", "alias": "SecurityReview", "orgId": "00DB00000000002AAA",
             "isSandbox": true, "connectedStatus": "RefreshTokenAuthError"}
        ],
        "scratchOrgs": [{"username": "test-1@example.com", "alias": "so", "orgId": "00DC00000000003AAA",
                         "isScratch": true, "status": "Expired", "isExpired": true,
                         "expirationDate": "2026-09-01", "isDefaultUsername": true}]
    }});
    let aliases = HashMap::from([(
        "sb@x.com.uat".to_string(),
        vec!["SecrutiyReview".to_string(), "SecurityReview".to_string()],
    )]);
    let orgs = parse_org_list(&json, &aliases);
    assert_eq!(orgs.len(), 3);

    assert_eq!(orgs[0].alias(), "so");
    assert_eq!(orgs[0].kind, OrgKind::Scratch);
    assert_eq!(orgs[0].health(), Health::Expired);
    assert_eq!(orgs[0].expires.unwrap().to_string(), "2026-09-01");

    assert_eq!(orgs[1].kind, OrgKind::DevHub);
    assert_eq!(orgs[1].health(), Health::Ok);
    assert_eq!(orgs[1].host(), "hub.my.salesforce.com");

    assert_eq!(orgs[2].aliases, ["SecrutiyReview", "SecurityReview"]);
    assert!(orgs[2].has_duplicate_aliases());
    assert_eq!(orgs[2].health(), Health::Down);
}

#[test]
fn installed_version_is_matched_by_subscriber_package() {
    let json = json!({"status": 0, "result": [
        {"SubscriberPackageId": "033A00000000001", "SubscriberPackageVersionNumber": "1.8.2.2"},
        {"SubscriberPackageId": "033000000000999", "SubscriberPackageVersionNumber": "3.0.0.1"}
    ]});
    assert_eq!(
        parse_installed(&json, Some("033A00000000001AAA")),
        Some("1.8.2.2".into())
    );
    assert_eq!(parse_installed(&json, Some("033000000000123")), None);
    assert_eq!(parse_installed(&json, None), None);
}

#[test]
fn package_versions_newest_first() {
    let json = json!({"status": 0, "result": [
        {"SubscriberPackageVersionId": "04tA", "Version": "1.8.2.2", "Name": "May", "IsReleased": true,
         "CreatedDate": "2026-05-02 10:00", "CodeCoverage": "82%"},
        {"SubscriberPackageVersionId": "04tB", "Version": "1.8.10.1", "Name": "June", "IsReleased": false,
         "CreatedDate": "2026-06-02 10:00", "CodeCoverage": {"apexCodeCoveragePercentage": 81},
         "HasPassedCodeCoverageCheck": true, "BuildDurationInSeconds": 900}
    ]});
    let versions = parse_list(&json);
    assert_eq!(versions[0].id, "04tB", "1.8.10 is newer than 1.8.2");
    assert_eq!(versions[0].state(), "Beta");
    assert_eq!(versions[0].coverage, Some(81.0));
    assert_eq!(
        versions[1].coverage,
        Some(82.0),
        "the CLI prints coverage as text"
    );
}

#[test]
fn deploys_and_test_results() {
    let records = [json!({
        "Id": "0AfX", "Status": "Failed", "StartDate": "2026-09-16T09:31:02.000+0000",
        "CompletedDate": "2026-09-16T09:31:43.000+0000", "NumberComponentsDeployed": 3,
        "NumberComponentErrors": 1, "NumberComponentsTotal": 4, "CheckOnly": true,
        "ErrorMessage": "boom", "CreatedBy": {"Name": "Ronny"}
    })];
    let deploys = parse_deploys(&records);
    assert_eq!(deploys[0].created_by, "Ronny");
    assert_eq!(deploys[0].component_errors, 1);
    assert!(deploys[0].check_only);
    assert!(deploys[0].start.is_some());

    let run = parse_test_result(
        &crate::demo::task_result(crate::sf::runner::TaskKind::Tests),
        "so",
    );
    assert_eq!((run.passing, run.failing, run.ran), (41, 1, 42));
    assert_eq!(run.failures.len(), 1);
    assert_eq!(run.failures[0].class, "LicenseCheckerTest");
    assert_eq!(run.coverage[0].name, "LicenseChecker", "lowest coverage first");
    assert_eq!(run.time_ms, 18342);
}

#[test]
fn config_file_rejects_unknown_fields() {
    let error = FileConfig::parse("dev_hub = \"DevHub\"\ndevhub = \"x\"")
        .unwrap_err()
        .to_string();
    assert!(error.contains("devhub"), "{error}");
}

#[test]
fn config_precedence_and_discovery() {
    let root = std::env::temp_dir().join(format!("sf-cockpit-test-{}", std::process::id()));
    let project = root.join("project");
    let cwd = project.join("force-app/main");
    let home = root.join("home");
    let xdg = root.join("xdg");
    for dir in [&cwd, &home.join(".sf"), &xdg.join("sf-cockpit")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let write = |path: &Path, text: &str| std::fs::write(path, text).unwrap();
    write(
        &project.join("sf-cockpit.toml"),
        "dev_hub = \"DevHub\"\nscratch_org = \"scratchOrg\"\n",
    );
    write(
        &project.join("sfdx-project.json"),
        r#"{"packageAliases": {"Pkg": "0HoA00000000001", "Pkg@1.0.0-1": "04tA"}}"#,
    );
    write(
        &home.join(".sf/config.json"),
        r#"{"target-dev-hub": "GlobalHub", "target-org": "other"}"#,
    );
    write(
        &xdg.join("sf-cockpit/config.toml"),
        "limit = 5\nsource_dir = \"src\"\n",
    );

    let cli = FileConfig {
        limit: Some(7),
        ..Default::default()
    };
    let config = load_from(cli, &cwd, Some(&home), Some(&xdg)).unwrap();
    assert_eq!(config.dev_hub, "DevHub", "project file beats sf config");
    assert_eq!(config.scratch_org.as_deref(), Some("scratchOrg"));
    assert_eq!(config.limit, 7, "cli beats files");
    assert_eq!(config.source_dir, "src", "global file beats defaults");
    assert_eq!(
        config.package.as_deref(),
        Some("0HoA00000000001"),
        "package from sfdx-project.json"
    );
    assert_eq!(config.project_dir.as_deref(), Some(project.as_path()));

    let without_files = load_from(FileConfig::default(), &root, Some(&home), None).unwrap();
    assert_eq!(without_files.dev_hub, "GlobalHub");
    assert_eq!(without_files.scratch_org.as_deref(), Some("other"));

    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(
        load_from(FileConfig::default(), &empty, Some(&empty), None)
            .unwrap()
            .needs_setup(),
        "a missing Dev Hub is no error, the app asks for it"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn org_notes_are_parsed_merged_and_saved() {
    let error = FileConfig::parse("[orgs.00D000000000A01]\nimportent = true\n")
        .unwrap_err()
        .to_string();
    assert!(error.contains("importent"), "{error}");

    let dir = crate::tests::temp_dir("org-notes-config");
    let cwd = dir.join("project");
    let xdg = dir.join("xdg");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(xdg.join("sf-cockpit")).unwrap();
    std::fs::write(
        xdg.join("sf-cockpit/config.toml"),
        "[orgs.00D000000000A01AAA]\nname = \"Global name\"\nimportant = true\n",
    )
    .unwrap();
    let project = cwd.join("sf-cockpit.toml");
    std::fs::write(
        &project,
        "# comment\ndev_hub = \"hub\"\n\n[orgs.00D000000000A01AAA]\nname = \"ACME prod\"\n",
    )
    .unwrap();
    let config = load_from(FileConfig::default(), &cwd, None, Some(&xdg)).unwrap();
    assert_eq!(
        config.org_display_name("00D000000000A01", "x"),
        "ACME prod",
        "project wins per field"
    );
    assert!(
        config.is_important("00D000000000A01AAA"),
        "global marking stays, 18-char ids work"
    );

    let note = OrgNote {
        name: Some("ACME".into()),
        important: Some(true),
    };
    save_org_note(&project, "00D000000000A01", &note).unwrap();
    save_org_note(
        &project,
        "00D000000000B02",
        &OrgNote {
            name: None,
            important: Some(true),
        },
    )
    .unwrap();
    let text = std::fs::read_to_string(&project).unwrap();
    assert!(text.starts_with("# comment\n"), "{text}");
    assert!(
        !text.contains("[orgs.00D000000000A01]"),
        "the 18-char table is reused: {text}"
    );
    let orgs = FileConfig::parse(&text).unwrap().orgs.unwrap();
    assert_eq!(orgs["00D000000000A01AAA"], note);
    assert_eq!(orgs["00D000000000B02"].important, Some(true));

    save_org_note(&project, "00D000000000B02", &OrgNote::default()).unwrap();
    let text = std::fs::read_to_string(&project).unwrap();
    assert!(
        !text.contains("00D000000000B02"),
        "an empty note removes the table: {text}"
    );
}

#[test]
fn update_versions_and_targets() {
    use crate::update::{archive_name, is_newer, parse_version, target_for};
    assert_eq!(parse_version("v0.10.2"), Some((0, 10, 2)));
    assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
    assert_eq!(parse_version("1.2.3-beta.1"), Some((1, 2, 3)));
    assert_eq!(parse_version("latest"), None);
    assert!(is_newer("v0.10.0", "0.9.1"));
    assert!(!is_newer("0.2.0", "0.2.0"));
    assert!(!is_newer("0.1.9", "0.2.0"));
    assert!(!is_newer("nightly", "0.2.0"));
    assert_eq!(target_for("macos", "aarch64"), Some("aarch64-apple-darwin"));
    assert_eq!(target_for("linux", "x86_64"), Some("x86_64-unknown-linux-musl"));
    assert_eq!(target_for("freebsd", "x86_64"), None);
    assert_eq!(
        archive_name("x86_64-unknown-linux-musl"),
        "sf-cockpit-x86_64-unknown-linux-musl.tar.gz"
    );
    assert_eq!(
        archive_name("x86_64-pc-windows-msvc"),
        "sf-cockpit-x86_64-pc-windows-msvc.zip"
    );
}

#[test]
fn update_commands() {
    use crate::update::{Via, build_download, build_latest_release, parse_release};
    assert_eq!(
        build_latest_release(Via::Gh, "me/tool"),
        strings(&["gh", "api", "repos/me/tool/releases/latest"])
    );
    assert_eq!(
        build_latest_release(Via::Curl, "me/tool").last().unwrap(),
        "https://api.github.com/repos/me/tool/releases/latest"
    );
    let dir = Path::new("/tmp/x");
    assert_eq!(
        build_download(Via::Gh, "me/tool", "v1.0.0", "a.tar.gz", dir),
        vec![strings(&[
            "gh",
            "release",
            "download",
            "v1.0.0",
            "--repo",
            "me/tool",
            "--pattern",
            "a.tar.gz",
            "--pattern",
            "a.tar.gz.sha256",
            "--dir",
            "/tmp/x",
            "--clobber",
        ])]
    );
    let curl = build_download(Via::Curl, "me/tool", "v1.0.0", "a.tar.gz", dir);
    assert_eq!(curl.len(), 2);
    assert_eq!(
        curl[1],
        strings(&[
            "curl",
            "-fsSL",
            "-o",
            "/tmp/x/a.tar.gz.sha256",
            "https://github.com/me/tool/releases/download/v1.0.0/a.tar.gz.sha256",
        ])
    );

    let release = parse_release(&json!({
        "tag_name": "v1.4.0",
        "body": "## What's Changed\r\n* Faster pushes",
        "html_url": "https://github.com/me/tool/releases/tag/v1.4.0",
    }))
    .unwrap();
    assert_eq!(release.version, "1.4.0");
    assert_eq!(release.tag, "v1.4.0");
    assert_eq!(release.notes, "## What's Changed\n* Faster pushes");
    assert!(parse_release(&json!({ "message": "Not Found" })).is_none());
}

#[test]
fn update_check_parses_and_defaults_to_daily() {
    use crate::config::UpdateCheck;
    let parse = |text: &str| FileConfig::parse(text).unwrap().update_check;
    assert_eq!(parse("update_check = \"start\""), Some(UpdateCheck::Start));
    assert_eq!(parse("update_check = \"off\""), Some(UpdateCheck::Off));
    assert_eq!(parse(""), None);
    assert!(FileConfig::parse("update_check = \"hourly\"").is_err());
    assert_eq!(UpdateCheck::default(), UpdateCheck::Daily);
}
