# Plan: build ADR 0050 (Pane updates like Raycast) in three PRs

The decision is in `docs/adr/0050-pane-downloads-its-updates-by-itself-and-restarts-into-them.md`, on branch `adr-raycast-updates` until it merges. Read it first, then `docs/installer.md` "Updating Pane itself" and the research at `%TEMP%\pane-wave\research-raycast-updates.md` (Part 2 has file:line references for today's flow).

The PRs are in order, and each one ships something that works by itself. Each PR rewrites the parts of `docs/installer.md` it changes, and removes the "Being replaced" note's bullets as they ship (the last PR removes the note). Each PR also keeps the glossary (`CONTEXT.md`: Application update, What's New item, Result section) and `docs/root-search.md` (Pane's own rows) in step.

Shared files (today's flow):
- `crates/pane-core/src/releases.rs`: the listing request and parsing, the rate limit.
- `crates/pane-core/src/application_update.rs`: the choice of release, the download, the sha256, unpacking, staging, the swap, the Apps entry and plist rewrites, the start cleanup.
- `crates/pane-core/src/launcher/application_update.rs`: the launcher state (`Updates`, `Notice`), the rows, the status line, `check_application_update`, `install_application_update`.
- `crates/pane-core/src/launcher.rs` (~5110): where the update rows are added to root search.
- `crates/pane/src/main.rs` (~281-309 wiring, ~509-516 the one check at start).
- `crates/pane/src/features/settings/about.rs`: the About page.
- Tests: `crates/pane-core/tests/application_update.rs` and `crates/pane-core/tests/support/releases.rs` (the in-process stand-in). The update tests spawn no processes, but the relaunch in PR 2 does, so expect CI-only families there.
- Smokes (CI only; never run them locally): the update phases in `scripts/smoke-windows.ps1` (~2862, "#54"), `scripts/smoke-macos.sh` (~2313) and `scripts/smoke-linux.sh` (~2497), with `scripts/release_server.py`.

## PR 1: background check and automatic download (pane-core, About)

The outcome: a running Pane finds a new release within hours, without a restart, and downloads and stages it by itself. Until PR 2 lands, the ready row still installs for the next start, as today, so this PR ships alone.

- **Schedule.** Check at start, then every 4 h, plus up to 15 min of random delay. Also check when the launcher is shown, if the last check is more than 4 h old. Find the window-shown hook beside `Launcher::load_icons_as_shown` (`launcher.rs` ~3358), and look at `launcher/timers.rs` and `launcher/schedules.rs` for how the core keeps a clock. A development build reads a shorter interval from an env var (for example `PANE_UPDATE_INTERVAL`), as it reads `PANE_RELEASES`. A release build cannot change it.
- **Record.** Write `application-update.json` in the cache folder, with a version field as the house records have. It holds the last check time, the `ETag`, and what the choice of release needs from the listing (tags, this system's asset names, and the newest release's `body` and `html_url` for PR 3). Send `If-None-Match`. On a `304`, choose from the record. Keep the 403/429 rate-limit backoff, and keep its reset time in the record so a restart honours it.
- **Quiet failures.** A background check or download that fails writes to Pane's log, records why and when for About, and retries at the next due check. No row, no status line. A check the user asked for still explains a failure.
- **Check for Pane Updates.** A root command, always listed when a release source is configured, with the alternate title "Check for Updates". It replaces the failure-only "Check for a Pane update" row (`Entry::CheckUpdate`). About's "Check for updates" runs the same thing.
- **Download by itself.** As soon as a check finds a newer release, run today's download, verify and stage steps, and write a staged marker (version and sha256) last. Run one download at a time, and let a newer release replace an older staged one. At start, keep a staging folder whose marker names a version newer than the running one, and remove anything else. Today's start removes everything (`application_update.rs` ~186-191).
- **Ready state.** Add a `Notice` state for "staged and ready", next to Offered. In this PR, the ready row can keep today's "Update Pane to <version>" title and swap. It must not download again.
- **About.** Show checking, downloading with progress, ready, the last check time and the last failure.
- **Tests.** In `tests/application_update.rs`, against the stand-in:
  - a check found by the timer, and one found when the window is shown;
  - a 304 answered from the record;
  - the record surviving a new launcher;
  - a quiet failure, with no row;
  - the download starting by itself, with the release server's log showing the package request without any choice;
  - the staged marker kept across a start, and a stale one removed;
  - Check for Pane Updates answering "up to date".
  The stand-in needs to serve `ETag` and `304`.
- **Smokes.** All three update phases assert "no request for the package until the row is chosen", which this PR breaks. Change them to wait for the package request, then the row. Dispatch `ci.yml -f only=smoke` per system.

## PR 2: Restart to Update Pane, the relaunch and the handoff

The outcome: Enter on a pinned, highlighted row restarts Pane into the new version, and failures fall back safely.

- **Pinned row.** The ready row becomes "Restart to Update Pane", with the subtitle "Pane <version> is ready. Your extensions and settings are kept".
  - For a blank query it is hoisted first, above the pinned home: see how alias matches are hoisted (`Ranked.before` in `launcher.rs`), the blank-query order in `crates/pane-core/src/search.rs` (~628-637, ~788-795), and `launcher/presentation.rs` (~537-551), where the row is a Command today.
  - It gets its own "Update" result section and the accent colour, in `crates/pane/src/features/root_search/` and `ui/`, within ADR 0029 and ADR 0035's accepted look.
  - Show it in compact mode (`features/compact_pins.rs`).
  - It is first, so it is selected by default.
  - While an extension install, update or acquisition runs, set `Row.unavailable` with the reason.
- **Toast.** "Pane <version> is ready", with a Restart action (`launcher/feedback.rs`, `features/toast.rs`). Show it once, when the update becomes ready while the window is open, or on the next open. Never show it as a HUD while Pane is hidden.
- **Restart sequence** (`application_update.rs` and `launcher/application_update.rs`, wired in `main.rs`):
  1. Run today's swap, which keeps the Apps entry's `DisplayVersion`, `MajorVersion` and `MinorVersion` and the macOS plist rewrite.
  2. Run `<new program> --version` and expect the new version. Otherwise undo the swap.
  3. Spawn the new program with `--updated-from <old> --wait-pid <pid>`. On Windows that is `pane.exe` at the install path (no console window; look at how `crates/pane-core/src/autostart/windows.rs` names the program). On macOS use `open -n <bundle> --args …`. On Linux use the program or the AppImage file (`appimage.rs`).
  4. Wait up to 20 s for a started signal from the new process. A simple choice: the new process creates a file named for its pid in the cache folder before it initialises GPUI, and the old process polls for it. No signal means: kill the child if it still runs, undo the swap, explain, and keep the row.
  5. Quit cleanly: `Launcher::quit_cleanly` (`launcher/crash_notice.rs` ~147) and the tray Exit path (`crates/pane-core/src/tray/windows.rs` ~755).
- **The new process** (`crates/pane/src/main.rs` argument parsing, ~23-71):
  - Parse `--updated-from` and `--wait-pid` (and keep `--version` and `--install` working).
  - With `--wait-pid`, wait before taking hotkeys or the keyboard hook, the tray, the local channel (`local_channel.rs`), the file index lock (`file_index/indexer.rs`) or the clipboard listener. Wait up to 30 s. Then end the old process only if its image path is the old program's path in the install folder (guard against pid reuse).
  - Once its window, hotkeys and tray are up, record "settled". Only a start after a settled run removes `pane.exe.old`, `pane.old` or `<name>.old`; change the start cleanup accordingly.
  - Show the launcher with the toast "Updated to Pane <version>".
- **About.** A "Restart to update" button. Remove the "installed and starts next time" wording, except as the explanation when a relaunch failed.
- **Windows setup.** Check that `scripts/setup-windows.iss` `CloseApplicationsFilter=*.exe,*.exe.old` and the uninstaller still find a relaunched Pane. Nothing should need to change.
- **Tests and smokes.** Unit-test the sequence's decisions with the spawn behind a seam (a fake that fails to start, exits, or never signals). Real relaunch evidence comes from the smokes. Each update phase:
  1. installs the older package;
  2. waits for the ready row;
  3. presses Enter;
  4. waits for the old pid to exit and a new `pane` to run from the same path;
  5. checks that it reports 99.0.0 and that the Apps entry says 99.0.0 (the Windows setup leg, if the phase uses the setup, or check it by hand with the registry);
  6. checks that the data is kept.
  Process-spawning tests hang locally, so verify them in CI (`ci-fast.yml` filters, then `ci.yml -f only=smoke` per system).

## PR 3: What's New

The outcome: after an update, root search shows "What's New in Pane <version>" for a day, and it opens the release notes.

- **Last run version.** Record the running version in the data folder at each start, in a small record or a field of `host_settings` (`crates/pane-core/src/host_settings.rs`). A start on a different version is an update. This includes `--updated-from` and an update installed by the setup. A data folder with no record but with Pane's data in it (a Pane before this flow) counts as an update. A fresh data folder doesn't.
- **The row.** "What's New in Pane <version>": hoisted and sectioned like the restart row, listed below it when both apply. It stays for 24 h from that start, or until it is opened or dismissed through an Actions panel entry. The crash notice (`launcher/crash_notice.rs`) is the model for a dismissible start-time row and its record.
- **Detail view.** The release body as Markdown (the renderer is in `crates/pane/src/extension_views/designed/markdown.rs`), with an "Open Release Page" action (`html_url`). Take the body from PR 1's record. If the record lacks it, fetch `GET /repos/pane-app/pane/releases/tags/v<version>` once.
- **Release notes.** `.github/release-notes.md` becomes the in-app What's New, so keep its first section short and readable there. Release 0.1.2 is the first that 0.1.1 users update to, by hand, so its notes say how. Coordinate with work item F.
- **Tests.**
  - The row after a version change, none on a fresh folder, and one for a legacy folder.
  - The row expiring after 24 h (inject the clock as the clipboard expiry does).
  - Dismissing it.
  - The detail view's text, and the fallback request.

## Not in these PRs

These are deferred in ADR 0050:
- installing without Enter (idle or on quit);
- a watchdog relaunch;
- a general single-instance guard;
- settings to turn checks off or hide What's New;
- system notifications or a tray badge.
