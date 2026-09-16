mod app;
mod clipboard;
mod demo;
mod sf;
mod theme;
mod ui;

#[cfg(test)]
mod tests;

use anyhow::{Result, bail};
use app::App;
use clap::Parser;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use ratatui::crossterm::execute;
use serde_json::Value;
use std::io::stdout;
use std::path::Path;
use std::time::Duration;

/// Inspect push upgrades and subscribers of your Salesforce managed packages, right in the terminal.
/// Read-only: it only runs `sf data query` against your packaging org.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Alias or username of the packaging org (the Dev Hub that owns the package).
    /// Default: `target-dev-hub` from the sf CLI config.
    #[arg(short = 'o', long)]
    target_org: Option<String>,

    /// Number of recent push requests to load.
    #[arg(short, long, default_value_t = 30)]
    limit: usize,

    /// Load once and print a plain-text summary instead of opening the TUI.
    #[arg(long)]
    print: bool,

    /// Show fictional sample data, no org needed.
    #[arg(long)]
    demo: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let org = if cli.demo {
        "demo".to_string()
    } else {
        match cli.target_org.or_else(configured_dev_hub) {
            Some(org) => org,
            None => bail!(
                "no packaging org given. Pass --target-org <alias>, or set a default with \
                 `sf config set target-dev-hub=<alias> --global`. Try --demo to look around first."
            ),
        }
    };

    if cli.print {
        let data = if cli.demo {
            demo::data()
        } else {
            sf::load(&org, cli.limit)?
        };
        print_summary(&org, &data);
        return Ok(());
    }

    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        previous_hook(info);
    }));

    let mut app = App::new(org, cli.demo, cli.limit);
    app.reload();
    let result = run(&mut terminal, app);

    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, mut app: App) -> Result<()> {
    while !app.quit {
        app.poll();
        let frame = terminal.draw(|frame| ui::draw(frame, &mut app))?;
        app.screen = frame.buffer.clone();

        if event::poll(Duration::from_millis(80))? {
            // Drain queued events so mouse drags stay smooth.
            loop {
                match event::read()? {
                    Event::Key(key) => app.on_key(key),
                    Event::Mouse(mouse) => app.on_mouse(mouse),
                    _ => {}
                }
                if app.quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        app.tick = app.tick.wrapping_add(1);
    }
    Ok(())
}

/// `target-dev-hub` from the project config (`.sf/config.json` in this or a parent directory),
/// then from the global config (`~/.sf/config.json`).
fn configured_dev_hub() -> Option<String> {
    let project = std::env::current_dir().ok().and_then(|dir| {
        dir.ancestors()
            .find_map(|d| dev_hub_from(&d.join(".sf/config.json")))
    });
    project.or_else(|| dev_hub_from(&std::env::home_dir()?.join(".sf/config.json")))
}

fn dev_hub_from(path: &Path) -> Option<String> {
    let json: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    json["target-dev-hub"]
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn print_summary(org: &str, data: &sf::Data) {
    println!(
        "{} push requests, {} subscribers on {org}\n",
        data.requests.len(),
        data.subscribers.len()
    );
    for request in &data.requests {
        let counts = data.counts(&request.id);
        println!(
            "{}  {:<9} {:<10} {} succeeded, {} failed, {} other  ({})",
            request.id,
            data.version_label(&request.version_id),
            request.status,
            counts.succeeded,
            counts.failed,
            counts.other,
            ui::fmt_duration(request.start, request.end),
        );
        for job in data
            .jobs_for(&request.id)
            .into_iter()
            .filter(|j| j.status == "Failed")
        {
            let alias = data.alias(&job.org_key).map(|o| o.alias.as_str()).unwrap_or("-");
            println!(
                "    ✗ {} [{}] {}",
                data.org_name(&job.org_key),
                job.org_key,
                alias
            );
            for error in data.errors_for(&job.id) {
                println!("      {} ({}): {}", error.title, error.kind, error.message);
            }
        }
    }
}
