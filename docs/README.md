# Documentation index

Read the root [CLAUDE.md](../CLAUDE.md) first. Come here only for the doc relevant to the current task.

| File | Read this when you need to... |
|---|---|
| [push-upgrade-troubleshooting.md](push-upgrade-troubleshooting.md) | Investigate failed Salesforce package push upgrades, identify affected subscribers, inspect `PackagePushError`, and prepare retries using the Salesforce CLI or sf-cockpit. |
| [architecture.md](architecture.md) | Understand how data flows from `sf` to the screen, the module map, or the message/action loop before changing state or wiring in a new subprocess call. |
| [features.md](features.md) | Find which files implement a given tab/feature, or check whether something is implemented vs. only planned. |
| [development.md](development.md) | Set up a dev loop, add or run a test, or verify a change (build/lint/test commands, how the test suite is organized). |

`README.md` at the repo root covers installation, keybindings, configuration, and the common error
reference. The troubleshooting guide links to that reference and adds a worked investigation workflow.
