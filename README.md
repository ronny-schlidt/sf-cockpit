# sf-push-inspector

A terminal UI for Salesforce ISVs to see what happened to your **managed package push upgrades**: which orgs failed, why they failed, what to do next, and which subscriber org is on which version.

The Setup UI and `sf package push-upgrade report` only show a summary. This tool puts push requests, jobs, error messages and subscribers side by side, with full mouse support.

<!-- Add a screenshot: run `sf-push-inspector --demo`, take a screenshot and save it as docs/screenshot.png -->
<!-- ![sf-push-inspector](docs/screenshot.png) -->

## Features

- **Push requests** with version, status, results (`12✓ 3✗`) and duration.
- **Jobs per request**, failures first, with org name, local `sf` alias, org type, instance and org ID.
- **Error details** for each job, plus a plain-language **next step** for common errors such as `IneligibleUpgrade` or `UnclassifiedError`.
- **Subscribers** with their installed version, highlighting orgs that are behind the latest released version. You can filter by name, alias, org ID, instance or version.
- **Mouse support:** click rows, tabs and buttons, scroll with the wheel, drag the divider to resize panes, and drag to select and copy text.
- **Read-only:** it only runs `sf data query`. Nothing is written to any org, and no data leaves your machine.
- A **demo mode** with sample data, so you can try it without an org.

## Requirements

- The [Salesforce CLI](https://developer.salesforce.com/tools/salesforcecli) (`sf`) on your `PATH`.
- Access to the **packaging org** that owns your managed package (for 2GP, the Dev Hub), logged in with `sf`.
- Push upgrades enabled for your package, so the org can query `PackagePushRequest`.

## Installation

### Option 1: prebuilt binary

Download the archive for your platform from the [latest release](https://github.com/ronny-schlidt/sf-push-inspector/releases/latest):

| Platform | File |
|---|---|
| macOS (Apple Silicon) | `sf-push-inspector-<version>-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `sf-push-inspector-<version>-x86_64-apple-darwin.tar.gz` |
| Linux (x86_64) | `sf-push-inspector-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux (ARM64) | `sf-push-inspector-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| Windows | `sf-push-inspector-<version>-x86_64-pc-windows-msvc.zip` |

macOS and Linux:

```bash
tar xzf sf-push-inspector-*.tar.gz
sudo mv sf-push-inspector-*/sf-push-inspector /usr/local/bin/
# macOS only: the binary is not notarized, so remove the download quarantine flag
sudo xattr -d com.apple.quarantine /usr/local/bin/sf-push-inspector
```

Windows: unzip the archive and put `sf-push-inspector.exe` in a folder on your `PATH`.

### Option 2: with Cargo

You need [Rust](https://rustup.rs) 1.88 or newer.

```bash
cargo install --git https://github.com/ronny-schlidt/sf-push-inspector
```

### Option 3: from source

```bash
git clone https://github.com/ronny-schlidt/sf-push-inspector
cd sf-push-inspector
cargo build --release
./target/release/sf-push-inspector --demo
```

## Quick start

```bash
# Try it without an org
sf-push-inspector --demo

# Log in to your packaging org once and make it the default Dev Hub
sf org login web --alias my-packaging-org --set-default-dev-hub

# Open the TUI
sf-push-inspector

# Or name the org explicitly
sf-push-inspector --target-org my-packaging-org
```

Without `--target-org`, the tool uses `target-dev-hub` from your `sf` config: first the project config (`.sf/config.json` in the current or a parent directory), then the global one.

### Options

| Option | Description |
|---|---|
| `-o, --target-org <alias>` | Packaging org alias or username. |
| `-l, --limit <n>` | Number of recent push requests to load (default 30). |
| `--print` | Print a plain-text summary instead of opening the TUI. Handy for scripts, CI logs or AI agents. |
| `--demo` | Use fictional sample data. |

## Controls

| Mouse | Action |
|---|---|
| Click a row | Select a push request, job or subscriber. |
| Click a tab or a button in the footer | Switch tab or run the action. |
| Scroll wheel | Scroll the list or panel under the pointer. |
| Drag the divider between the panels | Resize the panels. |
| Drag over text | Select it. It is copied to the clipboard when you release the button. |

| Key | Action |
|---|---|
| `1` / `2` | Push Upgrades / Subscribers tab |
| `Tab`, `←` `→` | Move between panels |
| `↑` `↓`, `j` `k`, `PgUp` `PgDn`, `g` `G` | Move the selection or scroll the details |
| `c` | Copy the error details of the selected job |
| `/` | Filter subscribers (`Esc` clears the filter) |
| `[` `]` | Resize the panels |
| `r` | Reload |
| `q`, `Esc` | Quit |

### Copying text

Selections stay inside the panel where you started dragging, so borders and neighbouring panels are never copied. The text goes to the system clipboard through `pbcopy` (macOS), `clip` (Windows), `wl-copy`, `xclip` or `xsel` (Linux). If none is available, the tool falls back to the OSC 52 escape sequence, which works in most modern terminals, also over SSH.

You can also use your terminal's own selection. Most terminals bypass mouse capture while you hold `Shift` (in iTerm2 it is `Option`).

## Common push upgrade errors

| Error | Meaning | Next step |
|---|---|---|
| `IneligibleUpgrade`: "This package is not yet available" | The new version has not reached the subscriber's Salesforce instance yet. This is common right after promoting a version. | Schedule the push again in a few hours. |
| `IneligibleUpgrade` (other messages) | The package is not installed, a beta version is installed, or the org already has this or a newer version. | Check the org on the Subscribers tab. |
| `UnclassifiedError`: "Unexpected Failure" | Salesforce does not reveal the cause. | Install the version into that org by hand (`sf package install`) to see the real error. If that works, push again. Otherwise open a support case with the error number. |
| `ApexTestFailure` | An Apex test failed in the subscriber org during the upgrade. | Fix the test or the code and create a new package version. |

## What it queries

All queries use the standard Data API through `sf data query` against your packaging org:

- `PackagePushRequest`: push requests and their status
- `PackagePushJob`: one job per subscriber org
- `PackagePushError`: errors of failed jobs
- `MetadataPackageVersion`: version numbers and release state
- `PackageSubscriber`: subscriber org names, types, instances and installed versions

Local `sf` aliases are read from `~/.sfdx` to show which subscriber orgs you are logged in to.

## Troubleshooting

- **"could not run `sf`"**: install the Salesforce CLI and make sure `sf --version` works in the same terminal.
- **"sObject type 'PackagePushRequest' is not supported"** or **"No such column"**: the org is not the packaging org of your package, or push upgrades are not enabled for it. Check `--target-org`. A "No such column" error can also mean that `sf` could not reach the org and fell back to an old API version; check your connection with `sf org display --target-org <alias>`.
- **Loading takes a few seconds**: every query starts the `sf` CLI. Five queries run in parallel, usually in 3 to 8 seconds.
- **No colors or broken borders**: use a terminal with true color and Unicode support, for example iTerm2, WezTerm, Ghostty, Kitty or Windows Terminal.

## Development

```bash
cargo run -- --demo   # run with sample data
cargo test            # render and mouse tests against an in-memory terminal
cargo clippy --all-targets -- -D warnings
```

Pushing a tag like `v0.1.0` builds the binaries and creates a GitHub release.

Built with [Ratatui](https://ratatui.rs) and [crossterm](https://github.com/crossterm-rs/crossterm). The color palette is [Catppuccin Mocha](https://catppuccin.com).

## License

[MIT](LICENSE). Not affiliated with or endorsed by Salesforce, Inc. Salesforce is a trademark of Salesforce, Inc.
