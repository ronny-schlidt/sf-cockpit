use crate::app::{App, Tab};
use crate::{demo, ui};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

fn app() -> App {
    let mut app = App::new("demo".into(), true, 30);
    app.set_data(demo::data());
    app
}

fn render(app: &mut App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(150, 40)).unwrap();
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

#[test]
fn push_tab_shows_failed_jobs_first_with_next_step() {
    let mut app = app();
    let screen = render(&mut app);
    println!("{screen}");

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
    assert_eq!(app.tab, Tab::Subscribers);
    let screen = render(&mut app);
    println!("{screen}");
    assert!(screen.contains("3 behind latest released 2.4.0.1"));

    let (x, y) = find(&screen, "Filter");
    click(&mut app, (x, y + 1));
    assert!(app.editing_filter);
    for c in "initech".chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(app.filtered_subscribers().len(), 1);

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.on_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
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
    assert!(!copied.contains("sf-push-inspector"), "{copied}");

    // A plain click only selects, it copies nothing new.
    app.last_copied = None;
    click(&mut app, (x, y));
    assert!(app.last_copied.is_none());
    assert!(app.selection.is_none());
}
