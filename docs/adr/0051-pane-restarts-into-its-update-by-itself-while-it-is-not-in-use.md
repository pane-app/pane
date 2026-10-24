# Pane restarts into its update by itself while it is not in use

Accepted 2026-10-25 by the user's decision that Pane installs a downloaded update by itself while the user is not using it, as newer Raycast does ("do like Raycast"). It supersedes the deferred item "Installing without the user's Enter" in [ADR 0050](0050-pane-downloads-its-updates-by-itself-and-restarts-into-them.md). Everything else in ADR 0050 stands, including Enter on Restart to Update Pane.

**Why.** Under ADR 0050, a staged update waited for the user's Enter. A user who rarely opens root search, or never opens it on a blank query, could keep running the old version for days, even though the new one was already downloaded and checked. Raycast installs its updates by itself in the background. Its macOS 2.8 changelog says the updater avoids "automatic installation while Dictation or AI Agents are active in the background". On Windows, it updates through the Microsoft Store and relaunches itself afterwards (2.1). ADR 0050 deferred this until Pane's packages are signed. The user chose not to wait. The risk is the one ADR 0050 already accepts for Enter: the swap runs only after the `.sha256` check, a `--version` check of the new program, and the rollback when the new version does not start. Nobody reads a prompt before pressing Enter on the row either.

**When.** Pane restarts by itself once all of these hold:

- An update is staged and ready (Restart to Update Pane would be listed).
- The launcher has been hidden for 10 minutes. Showing it, expanded or collapsed to its search field, starts the 10 minutes over.
- Nothing is running that a restart would cut short or lose:
  - root search is the screen left on display, not a command, form, preview or extension list;
  - no command the user ran is still running, and no launch an extension asked for is running;
  - Pane is not setting up, installing, enabling, disabling, reloading, updating or removing an extension;
  - no clipboard history waits to be written;
  - the Settings window is closed. This also covers its hotkey recorders. The launcher's own hotkey screen is a screen other than root search.

Pane looks again within a minute once whatever held the restart ends. Raycast publishes no exact rule (its changelogs name only Dictation and AI agents). Ten minutes hidden is long enough that the user has finished with Pane, and short enough that an update lands the same day.

**How.** It is the same restart Enter starts (ADR 0050): the swap, the `--version` check, `--updated-from` and `--wait-pid`, the 20-second wait for the new version's report, and the undo. One argument is added: `--hidden`. The new version starts with its launcher hidden and does not take the focus, because the old one was hidden. It says "Updated to Pane <version>" the next time the launcher opens, and What's New is listed as after any update.

**When it fails.** A restart by itself that fails (the swap cannot be made, or the new version does not answer, start or report) is undone as Enter's is. Pane does not try that version again by itself during this run. The row stays, saying why, for the user to choose.

**No setting.** Raycast documents no setting that turns automatic installation off. Its settings and manual name none, and only an unofficial hidden default (`updaterEnabled`) is reported. So Pane adds none, as ADR 0050 did for the download. One can be added if users ask.

**Consequences.**

- The glossary's Application update and [installer](../installer.md#updating-pane-itself) say that Pane also restarts by itself.
- A Pane that restarted by itself is briefly not running, for the second or two between the old process exiting and the new one taking the hotkeys. A hotkey pressed in that moment does nothing. If the user summons the launcher while a restart by itself is under way, the new version still starts hidden, and the next summon shows it.
- The tests drive the 10 minutes with the launcher's clock. The smokes keep checking the restart Enter starts, which is the same path.
