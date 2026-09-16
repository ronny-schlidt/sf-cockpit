# Development

## Setup

Requires Rust 1.88+ (`rust-version` in `Cargo.toml`). No Salesforce org or `sf` CLI login is needed for
building, linting, or testing — only for running the real (non-`--demo`) TUI against live data.

```bash
cargo build                                  # debug build
cargo run -- --demo                          # TUI against fictional sample data
cargo run -- --print --tab push --demo       # plain-text output, fastest way to sanity-check a change
```

## Verification (what CI runs — `.github/workflows/ci.yml`)

```bash
cargo fmt --check                            # rustfmt.toml: max_width = 110
cargo clippy --all-targets -- -D warnings    # every warning is a hard error
cargo test
```

Run all three before considering a change done. CI runs them on Linux for every push and PR; the release
workflow compiles macOS, Linux and Windows when a `v*` tag is pushed.

## Test organization (`src/tests/`, run via `cargo test`)

- **`parsing.rs`** — asserts the exact argv of every `build_*` function in `src/sf/*.rs` (e.g.
  `build_schedule`, `build_promote`, `build_deploy`), and record-parsing helpers (`parse_org_list`,
  `parse_test_result`, ...). These never spawn a subprocess. Also covers `src/config.rs` layering
  (`load_from` with flags/project/global/sfdx-project fixtures).
- **`cache.rs`** — on-disk `Cache` round-trips, format versioning, permission bits.
- **`ui.rs`** — renders the full `App` (seeded from `src/demo.rs` fixtures) to a Ratatui `TestBackend` and
  asserts on the resulting character grid; also drives key/mouse events through `App::on_key`/`on_mouse` to
  test interaction flows (the schedule wizard, filters, modals) end to end without a real terminal.

Run a subset with `cargo test <substring>`, e.g. `cargo test push_commands` or `cargo test ui::`.

When adding a new `sf` command: write the `build_*` function to return `Vec<String>` (don't shell out
inline), and add a `parsing.rs` case asserting its argv — this is the project's established way of testing
Salesforce CLI interaction without a live org, and keeping it consistent is more valuable than testing the
same command a different way.

## Troubleshooting

- **A change compiles but `cargo clippy --all-targets -- -D warnings` fails** — this is a real CI gate, not
  advisory; `cargo build` alone does not verify a change.
- **A UI test's rendered-text assertion fails after a visual change** — `src/tests/ui.rs` compares exact
  character grids (`render()`/`render_sized()`), so any layout/wording change to `src/ui/*.rs` requires
  updating the corresponding expected strings in the same commit.
- **`could not run \`sf\`\`** when *not* using `--demo` or `--print`: the Salesforce CLI must be on `PATH`
  and logged in to the Dev Hub that owns the package — see README.md § Troubleshooting for the full list of
  user-facing error messages and fixes (these are about the running app, not the dev loop).
