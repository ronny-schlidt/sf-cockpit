# Architecture

## Technology stack

- **Rust**, 2024 edition, minimum 1.88 (`Cargo.toml`).
- **[Ratatui](https://ratatui.rs)** (0.30) for the TUI, on top of `crossterm` for terminal/mouse events.
- **clap** (derive) for CLI flags.
- **serde / serde_json / toml / toml_edit** for config files and parsing `sf --json` output; `toml_edit`
  specifically so the Settings tab can rewrite one key in `sf-cockpit.toml` without disturbing comments or
  key order.
- **chrono** for timestamps (`DateTime<Local>`, `DateTime<FixedOffset>` for Salesforce's API timestamps).
- **anyhow** for error handling throughout.
- No async runtime — background work uses plain `std::thread` + `std::sync::mpsc` (see below).
- No database, no network client: all Salesforce access goes through the external `sf` CLI as a subprocess;
  the update check shells out to `gh` or `curl` the same way (`src/update.rs`).
- **sha2** to verify downloaded release archives against their `.sha256` file.

## Data flow: `sf` subprocess → UI

```
src/sf/*.rs (build_* fns)        argv: Vec<String>, pure, unit-tested
        │
        ▼
src/sf/mod.rs::run_json()        synchronous: spawn `sf ... --json`, parse_json(), map errors
   or
src/sf/runner.rs::spawn()        background thread: spawn `sf`, stream stdout/stderr line by line
        │                        as Msg::TaskLine, then Msg::TaskDone { json, .. } over mpsc::Sender<Msg>
        ▼
src/app/mod.rs                   App::poll() drains the Receiver<Msg> each frame, updates App state
        │                        (Loadable<T> per tab: value / loading / error)
        ▼
src/ui/*.rs                      draw(frame, app, area) — pure rendering of App state, one file per tab
```

Two paths exist because most tab loads (`OrgsLoaded`, `VersionsLoaded`, ...) are one-shot queries fired at
startup/reload and finish via a plain background thread that sends a single `Msg`, while user-triggered
write commands (schedule a push, deploy, run tests) use `runner::spawn`, which streams every output line
live into the Task Log modal (`Modal::TaskLog`) so long-running commands aren't silent.

`--demo` and `--print` skip the runner/thread machinery: `src/demo.rs` provides fixtures directly, and
`src/print.rs` calls the same `load`/`query` functions synchronously and formats the result as text instead
of drawing a frame.

## Module map

```
main.rs           CLI flags, config load, terminal init, the event loop (run())
update.rs         latest-release check (cached 24 h), download + checksum + in-place binary swap, --update
app/
  mod.rs          App struct: Loadable<T> per tab, Toast, Action enum, on_key/on_mouse dispatch, poll()
  tabs.rs         TabId, Target (focusable regions), the TABS registry (key, label, panes)
  input.rs        key/mouse → Action mapping per tab
  modal.rs        Modal enum: Confirm, Input, Picker, Wizard (push-upgrade scheduling), TaskLog, Update, Message
  selection.rs    mouse drag-to-select for copying text
  settings.rs     SettingKey enum + editing logic for the Settings tab
ui/
  mod.rs          draw() entry point, top/bottom bars, selection highlight overlay
  push.rs, orgs.rs, versions.rs, deploy.rs, settings.rs, subscribers.rs   one file per tab, render-only
  modal.rs        renders whatever `app.modal` currently holds
  table.rs        shared scrollable/selectable table widget
sf/
  mod.rs          command()/run_json()/parse_json()/strip_control() — shared `sf` invocation plumbing, Msg enum
  query.rs        SOQL query builder + `sf --json` record helpers (text/number/flag/time), Timestamp type
  push.rs         PackagePushRequest/Job/Error, Subscriber, Version; build_schedule/build_abort
  orgs.rs         sf org list + local alias file; OrgKind, Health, build_delete_scratch
  versions.rs     MetadataPackageVersion; build_promote/build_install/build_create, install_url
  deploy.rs       DeployRequest + Apex test runs; build_deploy/build_tests, parse_test_result
  runner.rs       background TaskHandle: spawn() (real sf child process), spawn_demo() (fake, for --demo/tests)
config.rs         FileConfig (deny_unknown_fields), layered load_from(), Origin tracking, save_value()
cache.rs          on-disk Cache: versioned JSON entries, 0700/0600 perms on Unix, never stores tokens
demo.rs           fixtures shared by --demo and every test (ACME/GLOBEX/INITECH/... orgs)
print.rs          --print: synchronous load + plain-text formatting per tab
theme.rs          Catppuccin Mocha Color constants + status_color()
clipboard.rs      OS clipboard write
```

## State and messaging

- `App` (`src/app/mod.rs`) owns one `Loadable<T>` per tab (`value: Option<T>`, `loading: bool`,
  `error: Option<String>`), so a tab can show stale data while a refresh is in flight, and keep showing it
  if the refresh fails.
- `Action` is the single enum for "the user did something" (key or mouse), independent of which key/button
  triggered it — `src/app/input.rs` maps input events to `Action`, `App` methods act on it. Follow this
  indirection when adding a new keybinding rather than matching on `KeyCode` deep in tab logic.
- `Msg` (`src/sf/mod.rs`) is the single enum for "background work produced something" — every new
  background load or task should add a variant here rather than a bespoke channel.
- Tabs whose background load is still running show a spinner next to their tab label
  (`SPINNER` in `src/ui/mod.rs`, driven by `App::tick`).

## Configuration precedence

Implemented in `src/config.rs::load_from()`, highest priority first: CLI flags → project
`sf-cockpit.toml` (found by walking up from the cwd) → `~/.config/sf-cockpit/config.toml` (or
`$XDG_CONFIG_HOME`) → the `sf` CLI's own config (`target-dev-hub`/`target-org`) → `sfdx-project.json`'s
`packageAliases` (for `package` only). Each resolved field also records an `Origin` so the Settings tab can
show the user where a value came from and which file a write will land in. `[orgs.<org id>]` notes (own name,
`important`) merge per org and per field across the files, are normalized to 15-character org keys in
`Config.orgs`, and are written with `config::save_org_note()`. They live only in the config, never in
`PushData` or the cache, so changing them needs no reload and no cache `FORMAT` bump. See README.md § Configuration
for the user-facing config file format and keys.
