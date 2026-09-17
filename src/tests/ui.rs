use crate::app::Loadable;
use crate::app::modal::{Modal, Step};
use crate::app::{App, TabId, Target};
use crate::config::Config;
use crate::demo::{self, GLOBEX, INITECH, STARK, UMBRELLA};
use crate::sf::push::build_schedule;
use crate::ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::time::{Duration, Instant};

fn app() -> App {
    let mut app = App::new(Config::demo(), true);
    app.demo_delay = Duration::ZERO;
    app.set_push(demo::push());
    app
}

fn render(app: &mut App) -> String {
    render_sized(app, 150, 40)
}

fn render_sized(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let frame = terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    app.screen = frame.buffer.clone();
    let buffer = &app.screen;
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn key(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    app.poll();
}

fn keys(app: &mut App, text: &str) {
    for c in text.chars() {
        key(app, KeyCode::Char(c));
    }
}

fn mouse(app: &mut App, kind: MouseEventKind, (column, row): (u16, u16)) {
    app.on_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

fn click(app: &mut App, pos: (u16, u16)) {
    mouse(app, MouseEventKind::Down(MouseButton::Left), pos);
    mouse(app, MouseEventKind::Up(MouseButton::Left), pos);
}

fn find(screen: &str, needle: &str) -> (u16, u16) {
    screen
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find(needle)
                .map(|i| (line[..i].chars().count() as u16, y as u16))
        })
        .unwrap_or_else(|| panic!("`{needle}` not on screen:\n{screen}"))
}

fn wait_for_tasks(app: &mut App) {
    let start = Instant::now();
    loop {
        app.poll();
        if app.tasks.iter().all(|t| !t.running()) {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "task did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn push_tab_shows_failed_jobs_first_with_next_step() {
    let mut app = app();
    let screen = render(&mut app);
    println!("{screen}");

    assert!(screen.contains("sf-cockpit"));
    assert!(screen.contains("2.4.0.1 · Autumn '26"));
    assert!(screen.contains("2 succeeded"));
    assert!(screen.contains("3 failed"));
    assert!(screen.contains("Globex Industries"));
    assert!(
        screen.contains("Unexpected Failure"),
        "first failed job is selected"
    );
    assert!(screen.contains("Next step"));
}

#[test]
fn mouse_selects_rows_tabs_filter_and_resizes() {
    let mut app = app();
    let screen = render(&mut app);

    click(&mut app, find(&screen, "2.3.1.2"));
    assert_eq!(app.requests.selected(), Some(1));
    let screen = render(&mut app);
    assert!(screen.contains("Stark Logistics"), "{screen}");

    let (x, y) = find(&screen, "2 Subscribers");
    click(&mut app, (x + 2, y));
    assert_eq!(app.tab, TabId::Subscribers);
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("3 behind latest released 2.4.0.1"));

    let (x, y) = find(&screen, "Filter");
    click(&mut app, (x, y + 1));
    assert!(app.editing_filter);
    keys(&mut app, "initech");
    assert_eq!(app.filtered_subscribers().len(), 1);

    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Char('1'));
    render(&mut app);
    let row = app.hits.body.y + 5;
    let middle = app.hits.body.x + app.hits.body.width / 2;
    let divider = app.hits.divider_x;
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), (divider, row));
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), (middle, row));
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), (middle, row));
    assert_eq!(app.split, 50);
}

#[test]
fn dragging_selects_and_copies_text_within_a_pane() {
    let mut app = app();
    let screen = render(&mut app);

    let (x, y) = find(&screen, "An unexpected failure");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), (x, y));
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), (x + 12, y + 1));
    let screen = render(&mut app);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), (x + 12, y + 1));

    let copied = app.last_copied.clone().expect("selection was copied");
    assert!(copied.starts_with("An unexpected failure"), "{copied}");
    assert_eq!(copied.lines().count(), 2);
    assert!(!copied.contains('│'), "selection stays inside the pane: {copied}");
    assert!(screen.contains("An unexpected failure"));

    // Dragging far outside the pane is clamped to the pane.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), (x, y));
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), (0, 0));
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), (0, 0));
    let copied = app.last_copied.clone().unwrap();
    assert!(!copied.contains("sf-cockpit"), "{copied}");

    // A plain click only selects, it copies nothing new.
    app.last_copied = None;
    click(&mut app, (x, y));
    assert!(app.last_copied.is_none());
    assert!(app.selection.is_none());
}

#[test]
fn schedule_wizard_preselects_orgs_behind_and_runs_the_push() {
    let mut app = app();
    key(&mut app, KeyCode::Char('s'));
    let Some(Modal::Wizard(wizard)) = &app.modal else {
        panic!("wizard is open");
    };
    assert_eq!(wizard.step, Step::Version);
    assert_eq!(wizard.selected_version().label, "2.4.0.1");
    let screen = render(&mut app);
    assert!(screen.contains("step 1 of 4"), "{screen}");

    key(&mut app, KeyCode::Enter);
    let screen = render(&mut app);
    println!("{screen}");
    assert!(
        screen.contains("2 of 5 orgs"),
        "marked orgs behind are preselected: {screen}"
    );
    assert!(screen.contains("already on this version"));
    let Some(Modal::Wizard(wizard)) = &app.modal else {
        panic!("wizard is open");
    };
    assert!(
        wizard.orgs[0].important && wizard.orgs[1].important,
        "marked orgs first"
    );

    key(&mut app, KeyCode::Char('a'));
    assert!(
        render(&mut app).contains("3 of 5 orgs"),
        "a checks every org behind"
    );
    key(&mut app, KeyCode::Char('m'));
    assert!(
        render(&mut app).contains("2 of 5 orgs"),
        "m checks the marked orgs behind"
    );
    key(&mut app, KeyCode::Char('a'));

    key(&mut app, KeyCode::Enter);
    keys(&mut app, "tomorrow");
    key(&mut app, KeyCode::Enter);
    let Some(Modal::Wizard(wizard)) = &app.modal else {
        panic!("wizard is still open");
    };
    assert_eq!(
        wizard.step,
        Step::Time,
        "an invalid start time blocks the next step"
    );
    for _ in 0.."tomorrow".len() {
        key(&mut app, KeyCode::Backspace);
    }
    key(&mut app, KeyCode::Enter);

    let expected = build_schedule(
        "demo",
        "04t000000000003",
        &[GLOBEX.to_string(), STARK.to_string(), INITECH.to_string()],
        None,
    );
    assert_eq!(app.pending_argv(), Some(expected));
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("This upgrades customer orgs"), "{screen}");
    assert!(screen.contains("--org-list"));

    key(&mut app, KeyCode::Enter);
    wait_for_tasks(&mut app);
    assert!(app.tasks[0].succeeded());
    assert!(app.toast.as_ref().unwrap().text.contains("0DV000000000004"));
    assert!(app.modal.is_none());
}

#[test]
fn retry_failed_preselects_the_failed_orgs_and_mouse_cancels() {
    let mut app = app();
    key(&mut app, KeyCode::Char('f'));
    let Some(Modal::Wizard(wizard)) = &app.modal else {
        panic!("wizard is open");
    };
    let mut checked: Vec<&str> = wizard.checked().iter().map(|o| o.key.as_str()).collect();
    checked.sort();
    assert_eq!(checked, [GLOBEX, INITECH, STARK]);
    assert_eq!(wizard.retry_of.as_deref(), Some("0DV000000000001"));

    let screen = render(&mut app);
    let (x, y) = find(&screen, " Cancel");
    click(&mut app, (x + 1, y));
    assert!(app.modal.is_none());
}

#[test]
fn abort_is_refused_for_finished_requests() {
    let mut app = app();
    key(&mut app, KeyCode::Char('a'));
    assert!(app.modal.is_none());
    assert!(app.toast.as_ref().unwrap().is_error);
}

#[test]
fn orgs_tab_lists_orgs_with_duplicate_aliases_and_installed_versions() {
    let mut app = app();
    key(&mut app, KeyCode::Char('3'));
    assert_eq!(app.tab, TabId::Orgs);
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("Orgs · 5 · 2 scratch"), "{screen}");
    assert!(screen.contains("acme, acme-prod"));
    assert!(screen.contains("Expired"));

    key(&mut app, KeyCode::Char('I'));
    let screen = render(&mut app);
    assert!(screen.contains("not installed"), "{screen}");
    assert!(screen.contains("2.4.0.1"));

    keys(&mut app, "/acme");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.filtered_orgs().len(), 1);
    let screen = render(&mut app);
    assert!(screen.contains("2 aliases point at this org"), "{screen}");

    key(&mut app, KeyCode::Char('d'));
    assert!(app.modal.is_none(), "production orgs cannot be deleted");
}

#[test]
fn versions_tab_shows_beta_and_promote_asks_first() {
    let mut app = app();
    key(&mut app, KeyCode::Char('4'));
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("2.4.1.1"), "{screen}");
    assert!(screen.contains("Beta"));
    assert!(screen.contains("installPackage.apexp?p0=04t000000000004"));

    key(&mut app, KeyCode::Char('p'));
    let Some(Modal::Confirm(confirm)) = &app.modal else {
        panic!("promote asks first");
    };
    assert!(confirm.danger);
    let screen = render(&mut app);
    assert!(screen.contains("Promotion cannot be undone"), "{screen}");
    key(&mut app, KeyCode::Esc);
    assert!(app.modal.is_none());
    assert!(app.tasks.is_empty());
}

#[test]
fn install_into_the_scratch_org_is_not_marked_dangerous() {
    let mut app = app();
    key(&mut app, KeyCode::Char('3'));
    key(&mut app, KeyCode::Char('4'));
    key(&mut app, KeyCode::Char('i'));
    let Some(Modal::Picker(picker)) = &app.modal else {
        panic!("org picker is open");
    };
    assert_eq!(picker.items[picker.cursor].value, "scratch");
    key(&mut app, KeyCode::Enter);
    let Some(Modal::Confirm(confirm)) = &app.modal else {
        panic!("install asks first");
    };
    assert!(!confirm.danger);
    key(&mut app, KeyCode::Enter);
    wait_for_tasks(&mut app);
    assert!(app.tasks[0].succeeded());
    assert!(matches!(app.modal, Some(Modal::TaskLog { .. })));
    let screen = render(&mut app);
    assert!(screen.contains("finished in"), "{screen}");
}

#[test]
fn deploy_tab_runs_tests_and_shows_coverage() {
    let mut app = app();
    key(&mut app, KeyCode::Char('5'));
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("Deployments on scratch · 3"), "{screen}");
    assert!(screen.contains("1204/1204"));

    key(&mut app, KeyCode::Char('t'));
    assert!(matches!(app.modal, Some(Modal::Input(_))));
    keys(&mut app, "LicenseCheckerTest, ScanServiceTest");
    key(&mut app, KeyCode::Enter);
    let argv = app.pending_argv().expect("tests ask first");
    assert!(argv.windows(2).any(|w| w == ["--class-names", "ScanServiceTest"]));
    key(&mut app, KeyCode::Enter);
    wait_for_tasks(&mut app);

    let run = app.test_run.as_ref().expect("test result parsed");
    assert_eq!(run.failing, 1);
    assert_eq!(app.focus, Target::DeployDetails);
    let screen = render(&mut app);
    println!("{screen}");
    assert!(
        screen.contains("LicenseCheckerTest.expiredLicenseBlocksScan"),
        "{screen}"
    );
    assert!(screen.contains("64.8%"));
}

#[test]
fn deploy_streams_its_log() {
    let mut app = app();
    key(&mut app, KeyCode::Char('5'));
    key(&mut app, KeyCode::Char('D'));
    assert_eq!(
        app.pending_argv().unwrap(),
        crate::sf::deploy::build_deploy("force-app", "scratch")
    );
    key(&mut app, KeyCode::Enter);
    wait_for_tasks(&mut app);
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("Components: 1204/1204 (100%)"), "{screen}");
    key(&mut app, KeyCode::Esc);
    assert!(app.modal.is_none());
    key(&mut app, KeyCode::Char('L'));
    assert!(matches!(app.modal, Some(Modal::TaskLog { .. })));
}

#[test]
fn cached_data_shows_at_once_while_refreshing() {
    let mut app = app();
    app.push.loading = true;
    app.push.from_cache = true;
    app.push.loaded_at = Some(chrono::Local::now() - chrono::Duration::hours(2));
    app.orgs = Loadable::cached(demo::orgs(), chrono::Local::now());
    app.orgs.loading = true;
    let screen = render(&mut app);
    println!("{screen}");
    assert!(
        screen.contains("Globex Industries"),
        "cached rows are visible: {screen}"
    );
    assert!(screen.contains("refreshing demo"), "{screen}");
    assert!(screen.contains("showing data from 2 h ago"), "{screen}");
    assert!(screen.contains("1 Push Upgrades ⠋"), "tab spinner: {screen}");
    assert!(
        screen.contains("3 Orgs ⠋"),
        "background tabs show their refresh too: {screen}"
    );

    app.push.loading = false;
    app.push.error = Some("network down".into());
    let screen = render(&mut app);
    assert!(screen.contains("refresh failed"), "{screen}");
    assert!(
        screen.contains("Globex Industries"),
        "data stays after a failed refresh"
    );
}

#[test]
fn settings_tab_changes_the_scratch_org_and_saves_it() {
    let dir = crate::tests::temp_dir("settings");
    let path = dir.join("sf-cockpit.toml");
    std::fs::write(
        &path,
        "# keep me\ndev_hub = \"demo\"\nscratch_org = \"scratch\"\n",
    )
    .unwrap();
    let mut app = app();
    app.cfg.save_path = Some(path.clone());
    app.reload_orgs();
    app.poll();

    key(&mut app, KeyCode::Char('6'));
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("Dev Hub"), "{screen}");
    assert!(screen.contains("Scratch org"));
    assert!(screen.contains("off in demo mode"));

    keys(&mut app, "jj");
    key(&mut app, KeyCode::Enter);
    let Some(Modal::Picker(picker)) = &app.modal else {
        panic!("org picker is open");
    };
    assert_eq!(picker.items[0].label, "scratch", "scratch orgs first");
    let acme = picker.items.iter().position(|i| i.value == "acme").unwrap();
    for _ in 0..acme {
        key(&mut app, KeyCode::Down);
    }
    key(&mut app, KeyCode::Enter);

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# keep me\n"), "{text}");
    assert!(text.contains("scratch_org = \"acme\""), "{text}");
    assert_eq!(app.cfg.scratch_org.as_deref(), Some("acme"));
    assert_eq!(app.deploy_org, "acme", "deploys follow the scratch org");
    assert!(
        app.toast
            .as_ref()
            .unwrap()
            .text
            .contains("Saved scratch_org = acme")
    );
    let screen = render(&mut app);
    assert!(
        screen.contains("(project)"),
        "source shows the project file: {screen}"
    );

    key(&mut app, KeyCode::Char('k'));
    key(&mut app, KeyCode::Enter);
    let Some(Modal::Picker(picker)) = &app.modal else {
        panic!("package picker opens once the packages are loaded");
    };
    assert_eq!(picker.items.len(), 2);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.cfg.package.as_deref(), Some("Demo Package Extension"));
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("package = \"Demo Package Extension\"")
    );
    assert!(app.push.value.is_some(), "push data reloads for the new package");

    keys(&mut app, "jj");
    key(&mut app, KeyCode::Enter);
    assert!(matches!(app.modal, Some(Modal::Input(_))));
    for _ in 0..2 {
        key(&mut app, KeyCode::Backspace);
    }
    keys(&mut app, "abc");
    key(&mut app, KeyCode::Enter);
    assert!(app.toast.as_ref().unwrap().is_error, "limit must be a number");
    assert_eq!(app.cfg.limit, 30);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_tab_stays_visible_and_clickable_in_narrow_terminals() {
    for width in [70, 90, 110, 130, 150, 200] {
        let mut app = app();
        // A long status (demo badge, refreshing, data age) used to be drawn over the last tabs.
        app.push.loading = true;
        let screen = render_sized(&mut app, width, 30);
        let top = screen.lines().next().unwrap();
        for label in ["1 Push", "2 Sub", "3 Orgs", "4 Vers", "5 Deploy", "6 Settings"] {
            assert!(top.contains(label), "`{label}` missing at width {width}: {top}");
        }
        let (x, y) = find(&screen, "6 Settings");
        click(&mut app, (x + 1, y));
        assert_eq!(app.tab, TabId::Settings, "width {width}: {top}");
    }
}

#[test]
fn missing_dev_hub_opens_settings_and_guides_the_user() {
    let mut cfg = Config::demo();
    cfg.dev_hub = String::new();
    let mut app = App::new(cfg, true);
    app.demo_delay = Duration::ZERO;
    app.open_first_tab(TabId::Push);
    assert_eq!(app.tab, TabId::Settings);
    assert!(
        app.push.value.is_none() && !app.push.loading,
        "nothing is queried without a Dev Hub"
    );
    let screen = render(&mut app);
    println!("{screen}");
    assert!(
        screen.contains("Setup incomplete"),
        "a popup explains what is missing: {screen}"
    );
    assert!(
        screen.contains("Dev Hub: the org that owns your package"),
        "{screen}"
    );
    assert!(
        !screen.contains("not inside a Salesforce project"),
        "demo has a project dir"
    );
    key(&mut app, KeyCode::Enter);
    assert!(app.modal.is_none());
    let screen = render(&mut app);
    assert!(screen.contains("No Dev Hub chosen. Select Dev Hub"), "{screen}");

    key(&mut app, KeyCode::Char('1'));
    let screen = render(&mut app);
    assert!(screen.contains("No Dev Hub chosen yet"), "{screen}");
    assert!(screen.contains("to open Settings and choose the Dev Hub"));
    let (x, y) = find(&screen, " 6  to open");
    click(&mut app, (x + 1, y));
    assert_eq!(app.tab, TabId::Settings, "the 6 in the hint is clickable");

    key(&mut app, KeyCode::Enter);
    let Some(Modal::Picker(picker)) = &app.modal else {
        panic!("the org picker opens once the orgs are loaded");
    };
    assert_eq!(picker.items[0].value, "demo-hub", "Dev Hubs first");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.cfg.dev_hub, "demo-hub");
    assert!(
        app.push.value.is_some(),
        "push data loads right after choosing the Dev Hub"
    );
    let screen = render(&mut app);
    assert!(!screen.contains("No Dev Hub chosen"), "{screen}");
}

#[test]
fn subscribers_can_be_marked_and_named_and_are_saved() {
    let dir = crate::tests::temp_dir("org-notes");
    let path = dir.join("sf-cockpit.toml");
    std::fs::write(&path, "# keep me\ndev_hub = \"demo\"\n").unwrap();
    let mut app = app();
    app.cfg.save_path = Some(path.clone());

    key(&mut app, KeyCode::Char('2'));
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("2 marked"), "{screen}");
    assert!(screen.contains("Umbrella APAC"), "own name is shown: {screen}");
    assert!(
        screen.contains("Umbrella Health"),
        "with the Salesforce name next to it"
    );
    let first = app.filtered_subscribers()[0].org_key.clone();
    assert_eq!(first, GLOBEX, "marked orgs first");

    let umbrella = app
        .filtered_subscribers()
        .iter()
        .position(|s| s.org_key == UMBRELLA)
        .unwrap();
    app.subscribers.select(Some(umbrella));
    key(&mut app, KeyCode::Char('m'));
    assert!(app.cfg.is_important(UMBRELLA));
    let selected = app.filtered_subscribers()[app.subscribers.selected().unwrap()]
        .org_key
        .clone();
    assert_eq!(selected, UMBRELLA, "the cursor follows the org after re-sorting");

    key(&mut app, KeyCode::Char('e'));
    for _ in 0.."Umbrella APAC".len() {
        key(&mut app, KeyCode::Backspace);
    }
    keys(&mut app, "Umbrella Key Account");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.cfg.org_display_name(UMBRELLA, "x"), "Umbrella Key Account");

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# keep me\n"), "{text}");
    let saved = crate::config::FileConfig::parse(&text).unwrap().orgs.unwrap();
    let note = &saved[UMBRELLA];
    assert_eq!(note.name.as_deref(), Some("Umbrella Key Account"));
    assert_eq!(note.important, Some(true));

    keys(&mut app, "/★");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.filtered_subscribers().len(), 3, "★ filters to marked orgs");
}

#[test]
fn starting_outside_a_project_says_so_in_the_setup_popup() {
    let mut cfg = Config::demo();
    cfg.project_dir = None;
    cfg.package = None;
    cfg.scratch_org = None;
    cfg.save_path = Some(std::path::PathBuf::from("/tmp/sf-cockpit/config.toml"));
    let mut app = App::new(cfg, true);
    app.open_first_tab(TabId::Push);
    assert_eq!(app.tab, TabId::Settings);
    assert_eq!(
        app.setting_rows.selected(),
        Some(1),
        "the first missing setting is selected"
    );
    let screen = render(&mut app);
    println!("{screen}");
    assert!(
        screen.contains("You are not inside a Salesforce project"),
        "{screen}"
    );
    assert!(screen.contains("quit with q"), "{screen}");
    assert!(screen.contains("saved globally to"), "{screen}");
    assert!(screen.contains("/tmp/sf-cockpit/config.toml"), "{screen}");
    assert!(screen.contains("Package: which package"), "{screen}");
    assert!(screen.contains("Scratch org: the default org"), "{screen}");
    assert!(!screen.contains("  • Dev Hub:"), "the Dev Hub is set: {screen}");

    let mut complete = App::new(Config::demo(), true);
    complete.open_first_tab(TabId::Orgs);
    assert_eq!(complete.tab, TabId::Orgs, "no popup when nothing is missing");
    assert!(complete.modal.is_none());
}

#[test]
fn update_badge_and_dialog() {
    use crate::update::ReleaseInfo;
    let mut app = app();
    assert!(!render(&mut app).contains("available · N"));
    key(&mut app, KeyCode::Char('N'));
    assert!(app.modal.is_none(), "no dialog without a newer release");

    app.update = Some(ReleaseInfo {
        version: "9.9.9".into(),
        tag: "v9.9.9".into(),
        notes: "## What's Changed\n* Faster pushes".into(),
        url: "https://github.com/ronny-schlidt/sf-cockpit/releases/tag/v9.9.9".into(),
    });
    assert!(render(&mut app).contains("↑ 9.9.9 available · N"));
    key(&mut app, KeyCode::Char('N'));
    let screen = render(&mut app);
    assert!(screen.contains("Update available"));
    assert!(screen.contains("sf-cockpit 9.9.9 is available"));
    assert!(screen.contains("• Faster pushes"));
    assert!(screen.contains("Update now"));
    key(&mut app, KeyCode::Esc);
    assert!(app.modal.is_none());
}
