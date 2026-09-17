mod app;
mod cache;
mod clipboard;
mod config;
mod demo;
mod print;
mod sf;
mod theme;
mod ui;
mod update;

#[cfg(test)]
mod tests;

use anyhow::Result;
use app::App;
use app::tabs::TabId;
use clap::{Parser, ValueEnum};
use config::FileConfig;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use ratatui::crossterm::execute;
use std::io::stdout;
use std::time::Duration;

/// Salesforce cockpit in the terminal: push upgrades, subscribers, orgs, package versions, deployments
/// and Apex tests. Configuration: sf-cockpit.toml in the project, ~/.config/sf-cockpit/config.toml,
/// then the sf CLI config.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Alias or username of the Dev Hub that owns the package.
    #[arg(short = 'o', long, alias = "target-org", value_name = "ALIAS")]
    dev_hub: Option<String>,

    /// Package name or 0Ho id. Default: sf-cockpit.toml, then sfdx-project.json.
    #[arg(short, long, value_name = "NAME_OR_ID")]
    package: Option<String>,

    /// Number of recent push requests to load.
    #[arg(short, long)]
    limit: Option<usize>,

    /// Tab to open, or to print with --print.
    #[arg(short, long, value_enum, default_value_t = TabArg::Push)]
    tab: TabArg,

    /// Org for the Deploy & Test tab (alias or username). Default: scratch_org from the config.
    #[arg(long, value_name = "ALIAS")]
    org: Option<String>,

    /// Load once and print a plain-text summary of the tab instead of opening the TUI.
    #[arg(long)]
    print: bool,

    /// Show fictional sample data, no org needed.
    #[arg(long)]
    demo: bool,

    /// Install the latest release in place of this binary.
    #[arg(long, conflicts_with = "check_update")]
    update: bool,

    /// Print whether a newer release exists; exit code 10 if it does.
    #[arg(long)]
    check_update: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum TabArg {
    Push,
    Subscribers,
    Orgs,
    Versions,
    Deploys,
    Settings,
}

impl From<TabArg> for TabId {
    fn from(tab: TabArg) -> Self {
        match tab {
            TabArg::Push => TabId::Push,
            TabArg::Subscribers => TabId::Subscribers,
            TabArg::Orgs => TabId::Orgs,
            TabArg::Versions => TabId::Versions,
            TabArg::Deploys => TabId::Deploy,
            TabArg::Settings => TabId::Settings,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    update::clean_up_old_binary();
    if cli.update || cli.check_update {
        return update::run_cli(cli.update);
    }
    let config = if cli.demo {
        config::Config::demo()
    } else {
        config::load(FileConfig {
            dev_hub: cli.dev_hub.clone(),
            package: cli.package.clone(),
            limit: cli.limit,
            ..Default::default()
        })?
    };

    if cli.print {
        return print::run(&config, cli.tab, cli.org.as_deref(), cli.demo);
    }

    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        previous_hook(info);
    }));

    let mut app = App::new(config, cli.demo);
    if let Some(org) = cli.org {
        app.deploy_org = org;
    }
    if !cli.demo {
        app.cache = cache::Cache::default_location();
    }
    app.start();
    app.check_for_update();
    app.open_first_tab(cli.tab.into());
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
