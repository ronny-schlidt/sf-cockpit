# Feature inventory

All features below are implemented (no `TODO`/`unimplemented!`/planned markers found in `src/`). Each row
maps a tab to its rendering, state/input, and data-source files.

| Tab (key) | Behavior | UI | App state / input | Data source |
|---|---|---|---|---|
| Push Upgrades (`1`) | List push requests and their per-org jobs/errors; schedule a new push (4-step wizard), abort a pending/created request, retry the failed orgs of a request. | `src/ui/push.rs` | `src/app/mod.rs`, `src/app/modal.rs` (`Wizard`) | `src/sf/push.rs` (`PackagePushRequest/Job/Error` via Data API) |
| Subscribers (`2`) | Every org with the package installed, and whether it's behind the latest released version; mark important orgs (`m`, listed first and preselected in the push wizard) and give orgs own names (`e`), saved as `[orgs.<id>]` in the config file. | `src/ui/subscribers.rs` | `src/app/mod.rs` | `src/sf/push.rs` (`PackageSubscriber`) |
| Orgs (`3`) | All orgs known to the local `sf` CLI: status, scratch org expiry, duplicate-alias detection, installed package version; open in browser, copy org id/username, delete a scratch org. | `src/ui/orgs.rs` | `src/app/mod.rs` | `src/sf/orgs.rs` (`sf org list --all`, `sf package installed list`, local `~/.sfdx/alias.json`) |
| Versions (`4`) | Package versions with coverage and subscriber count; copy production/sandbox install links, promote a beta, install into an org, create a new version. | `src/ui/versions.rs` | `src/app/mod.rs` | `src/sf/versions.rs` (`sf package version list --verbose`, `MetadataPackageVersion`) |
| Deploy & Test (`5`) | Deployment history of an org; deploy the project with a live streamed log; run Apex tests and show failures/coverage. | `src/ui/deploy.rs` | `src/app/mod.rs` | `src/sf/deploy.rs` (`DeployRequest` via Tooling API, `sf project deploy start`, `sf apex run test`) |
| Settings (`6`) | Pick Dev Hub / package / scratch org from live lists; shows the origin of every config value and a Setup box with what is still missing; clears the on-disk cache. Opens automatically when no Dev Hub is configured, and the tabs that need one show a hint instead of an error. | `src/ui/settings.rs` | `src/app/settings.rs` | `src/config.rs`, `src/cache.rs` |

Cross-cutting, not tied to one tab:

- **Background refresh + spinner** — every tab loads its last cached data instantly, refreshes in the
  background, and shows a spinner on its tab label while doing so (`src/cache.rs`, `App::poll()` in
  `src/app/mod.rs`, `SPINNER` in `src/ui/mod.rs`).
- **Confirmation + danger marking** — every write command shows the exact `sf` argv and asks first; push
  upgrades, promotions, and installs outside the scratch org are flagged `danger: true` (red styling) in
  `src/app/modal.rs::Confirm`.
- **Mouse support** — tabs, rows, footer buttons, wheel scroll, the Push-tab pane divider, and drag-to-copy
  text selection (`src/app/selection.rs`, `on_mouse` in `src/app/mod.rs`).
- **`--demo`** — every tab works against fixtures in `src/demo.rs`, no org or `sf` CLI needed; also the
  basis for the whole UI test suite (`src/tests/ui.rs`).
- **`--print --tab <tab>`** — plain-text summary of one tab instead of the interactive UI, meant for
  scripts, CI logs, and AI agents (`src/print.rs`).
- **Layered config with origin tracking** — see [architecture.md § Configuration precedence](architecture.md#configuration-precedence).

For the user-facing description (keybindings table, config file format, common push-upgrade error
meanings) see the repo root [README.md](../README.md), which this file does not duplicate.
