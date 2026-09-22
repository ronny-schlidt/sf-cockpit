# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`sf-cockpit` is a Rust terminal UI (TUI) for Salesforce ISVs, wrapping the `sf` CLI for push upgrades,
subscribers, orgs, package versions, and deployments/Apex tests. Single binary, no server, no database —
every read/write goes through `sf` subprocesses. See [README.md](README.md) for user-facing behavior
(keybindings, config file format, common push-upgrade errors).

- **Language/stack:** Rust 2024 edition (rust-version 1.88), [Ratatui](https://ratatui.rs) for the TUI,
  `clap` for CLI parsing, `serde`/`toml`/`serde_json` for config and `sf --json` parsing, `chrono` for
  timestamps, `anyhow` for errors.
- **Distribution:** public GitHub repo `ronny-schlidt/sf-cockpit` (MIT). Pushing a `v*` tag builds release
  binaries for macOS/Linux/Windows (`.github/workflows/release.yml`); users install them with `install.sh`
  via `curl` from raw.githubusercontent.com, `gh` is optional (see README.md § Install). Everything
  committed is public: only fictional org ids/usernames in tests and demo data.

## Entry points and directories

- `src/main.rs` — CLI flags (`clap`), config loading, terminal setup, the event loop (`run()`).
- `src/app/` — application state and input handling (`App`, `Action` enum, per-tab logic). `mod.rs` holds
  shared state; `input.rs`, `modal.rs`, `selection.rs`, `settings.rs`, `tabs.rs` split by concern.
- `src/ui/` — rendering only, one file per tab (`push.rs`, `orgs.rs`, `versions.rs`, `deploy.rs`,
  `settings.rs`, `subscribers.rs`), plus `modal.rs` (dialogs) and `table.rs` (shared table widget).
  `ui/mod.rs::draw()` is the top-level render entry point.
- `src/sf/` — everything that talks to the Salesforce CLI: `query.rs` (SOQL + `sf --json` plumbing),
  `push.rs`, `orgs.rs`, `versions.rs`, `deploy.rs` (one domain each), `runner.rs` (background process
  execution and streaming). `sf/mod.rs` has the shared `command()`/`run_json()`/`parse_json()` helpers.
- `src/config.rs` — layered config (flags > project `sf-cockpit.toml` > global config > `sf` CLI config >
  `sfdx-project.json`); see README.md § Configuration for the precedence table.
- `src/cache.rs` — on-disk cache of last-loaded data (`~/Library/Caches/sf-cockpit` etc.), parsed data only,
  never tokens.
- `src/update.rs` — daily GitHub release check (`gh`, then `curl`), self-update (`N` in the TUI, `--update`).
- `src/demo.rs` — fictional sample data, used by `--demo` and by every test (no live org needed).
- `src/print.rs` — `--print`: plain-text tab summaries for scripts, CI logs, and AI agents.
- `src/theme.rs` — Catppuccin Mocha color constants.
- `src/tests/` — all tests (`cargo test`), see [docs/development.md](docs/development.md).
- `install.sh` — installs a prebuilt release binary or builds from source into `~/.local/bin`.

Read [docs/README.md](docs/README.md) for the full documentation index.

## Commands

```bash
cargo build                                  # debug build
cargo run -- --demo                          # run the TUI against fictional sample data, no org needed
cargo run -- --print --tab push --demo       # plain-text output, good for a quick sanity check
cargo test                                   # all tests: command builders, config, cache, UI rendering
cargo test <name_substring>                  # run a subset, e.g. `cargo test push_commands`
cargo fmt                                    # format (max_width = 110, see rustfmt.toml)
cargo fmt --check                            # what CI runs
cargo clippy --all-targets -- -D warnings    # what CI runs; treat every warning as an error
```

CI (`.github/workflows/ci.yml`) runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
`cargo test` on Linux only for every push/PR (the release workflow still compiles every platform). Run the same three commands locally before
considering a change done — no test org or Salesforce CLI login is needed for any of them.

## Conventions and architectural constraints

- **Commands are pure and tested without running them.** Every `sf` invocation is built by a `build_*`
  function in `src/sf/*.rs` that returns `Vec<String>` argv; tests in `src/tests/parsing.rs` assert on the
  argv directly. When adding a new `sf` call, follow this pattern rather than shelling out inline.
- **UI is data-driven and demo-testable.** `src/ui/*.rs` only reads `App` state; there is no direct `sf`
  access from rendering code. `src/tests/ui.rs` renders the app to a `TestBackend` buffer and asserts on
  the resulting text grid, using `src/demo.rs` fixtures — no live org or terminal required.
- **Long-running `sf` commands run in a background thread** (`src/sf/runner.rs`) and stream output line by
  line through an `mpsc::Sender<Msg>`; `App` drains it every `poll()`. Don't block the render loop on a
  subprocess.
- **Config keys are explicit and closed** (`#[serde(deny_unknown_fields)]` on `FileConfig`) — a typo in
  `sf-cockpit.toml` is a hard error, not a silent no-op. Add new keys to `KEYS` in `src/config.rs` too, or
  they won't be tracked for origin reporting.
- **Cache format is versioned** (`FORMAT` in `src/cache.rs`) — bump it when a cached struct's shape changes,
  so old cache files are ignored rather than misparsed.
- **Destructive/customer-facing `sf` commands require confirmation** (`Modal::Confirm`, `danger: true` for
  anything outside the scratch org — push upgrades, promotions, non-scratch installs). Preserve this when
  adding new write commands.
- Doc comments (`//!` at the top of a module) describe intent/invariants, not what the code obviously does
  — follow that style rather than commenting individual lines.

## Pitfalls

- On Windows, `sf` is a `.cmd` shim; `src/sf/mod.rs::program()` already handles this — don't hardcode `"sf"`
  elsewhere.
- `sf --json` output can have progress-bar/ANSI noise mixed in; always go through `parse_json`/
  `strip_control` (`src/sf/mod.rs`) rather than parsing stdout directly.
- The cache and demo/print paths must never see or store an access token or `sf org open`'s output (it
  contains a session id) — see README.md § Cache and § What it runs.

## Working rules for this repository

- Read this file first, then only the docs/code needed for the current task (see
  [docs/README.md](docs/README.md)). Follow existing patterns; keep changes focused.
- Before calling a change done, run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test` (or the relevant subset), and say which were actually run versus only inspected.
- Update the relevant file under `docs/` (and README.md if user-facing) in the same change when behavior,
  architecture, commands, or setup change.
- Record a non-obvious, verified fact that would save a future agent time in the most relevant existing
  doc instead of re-deriving it next time.
