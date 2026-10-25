# How Raycast updates itself, how Pane does today, and what to change

Researched 2026-10-24. Pane is at `main` 069bfab0 ("Move Pane to version 0.1.1").
Raw page captures used for quoting are in `C:\Users\ADMIN\AppData\Local\Temp\pane-wave\raw\`.
"Confirmed" means a primary Raycast source says it. "Inferred" or "Unconfirmed" means it does not.

---

## Part 1: Raycast

### 1A. Raycast for macOS

**The mechanism: Raycast's own updater, not Sparkle.**
- Raycast used Sparkle at first and dropped it because "it doesn't allow too custom UIs". It built its own updater instead. That updater stores builds on GitHub Releases, puts a small server in front of them, downloads the DMG, compares the code signature with the running app's, swaps the bundle and "show[s] release notes after the app is relaunched". Source: Thomas Paul Mann (Raycast), "How we built an app update flow our users love", 12 Oct 2020, https://medium.com/raycastapp/how-we-built-an-app-update-flow-our-users-love-89820602e8fe (archived copy: http://web.archive.org/web/20221206050226/https://medium.com/raycastapp/how-we-built-an-app-update-flow-our-users-love-89820602e8fe).
- The updater is still Raycast's own in v2. Raycast v2 beta 0.66 (24 Jun 2026) "Added privileged daemon process to ease updates for non admin users… authorize the daemon in System Settings > Login Items & Extensions > App Background Activity": https://www.raycast.com/changelog/macos/0-66. v2.7 (9 Oct 2026) fixed "querying the updater's Login Items status could freeze Raycast": https://www.raycast.com/changelog/macos/2-7.
- Homebrew's cask marks Raycast `auto_updates: true`, meaning the app updates itself: https://formulae.brew.sh/api/cask/raycast.json.

**When it checks.**
- "The app checks for updates at least once a day." It uses `NSBackgroundActivityScheduler`, and because that scheduler is unreliable it also compares against the date of the last check, so at least one check happens per day. There is also a **Check For Updates** command and "a status menu item". Source: Medium article above.
- Manual checks:
  - The manual says "In Raycast, search for `Check for Updates` and install any pending update": https://manual.raycast.com/troubleshooting.md.
  - The About tab: "You can also check for updates here": https://manual.raycast.com/settings.
  - The menu-bar icon, when it is enabled, lets you "access Settings, check for updates, and quit the app": https://manual.raycast.com/settings.

**Background download and install.**
- The download is automatic: "the client starts downloading the DMG in the background without asking the user." Source: Medium.
- Installing in the background came later: "We recently added automatic background updates." Source: Medium (2020).
- v1.21.0 (29 Jul 2021) introduced "Silent Updates", which "install in the background, they don't have any release notes and you most likely won't notice them". Regular updates with notes continued alongside them: https://www.raycast.com/changelog/1-21-0.
- Installation is still automatic today:
  - v2.8 (16 Oct 2026), Updater: "Avoid automatic installation while Dictation or AI Agents are active in the background".
  - v2.8 also: "Detect an update installed from another user account on the same Mac".
  - v2.8 also: "Raycast now relaunches itself if an update or restart fails after the app was already closed".
  - Source: https://www.raycast.com/changelog/macos/2-8.
- Unconfirmed: when exactly the automatic install fires (on idle, on quit, or at a time of day). The sources only say it is held back while Dictation or AI agents are busy.

**How the user learns an update is ready.**
- **A root-search item at the top.** "After successful security checks, the available app update is presented on the top of the root search. You just need to hit ↵ to install the update." Source: Medium.
- Users quoted in the same article: "I like the first entry when opening when there is a new update" and "The update prompt command that gets surfaced at the top and then release notes".
- v0.26.3 (8 Sep 2020) "Highlighted the **New Update Available** list item in the root search to make it easier to recognize when a new version is available": https://www.raycast.com/changelog/macos-v1/12 (the v1 changelog, page 12).
- A badge on the menu-bar icon, a system notification, or a toast/HUD for "update ready": **not found**. The menu-bar item does offer "check for updates".
- The status line in Settings → About: the manual only says you can check there. Its exact wording is unconfirmed.

**How it installs and relaunches.**
- Pressing Enter on the item removes the quarantine flag, moves the running app to the Trash and puts the new bundle in its place.
- It then schedules "the update to launch when the current version quits" (a trick borrowed from AppMover), with a launch argument marking it as an update. Last, it terminates the old version. **Relaunch is automatic.** Source: Medium.
- Since v2.8, Raycast relaunches itself if the update or restart fails after the app has already closed: https://www.raycast.com/changelog/macos/2-8.
- Restoring the window or state: unconfirmed. Because of the launch argument, the new process knows it was just updated and shows release notes (below).

**Settings.**
- The documented Settings have no auto-update toggle. Neither https://manual.raycast.com/settings nor the Advanced tab mentions one.
- An unofficial hidden default, `defaults write com.raycast.macos updaterEnabled -bool false`, turns the updater off "to allow `homebrew upgrade` to run without conflict". Source: a community knowledge base, https://github.com/HariSekhon/Knowledge-Base/blob/main/raycast.md (not Raycast documentation, unverified on current builds).
- A beta or internal channel exists internally: "support internal releases for our nightly builds" (Medium). A public beta channel toggle was not found. The v2 beta was a separate download (https://www.raycast.com/download-beta).
- An auto-update bug existed in 1.69–1.70. Raycast asked users to run "Check for Updates" manually (v1.73.0 notes, https://www.raycast.com/changelog/macos-v1/4; also posted on X, https://x.com/raycastapp/status/1778056813289869527, seen in search results only, not fetched).

**What's New after updating.**
- 2020: "When the update launches, we interpret the previously mentioned launch argument… detect if it's a new version and show the release notes", fetched from GitHub Releases through Raycast's server. Source: Medium.
- Later: a **"See What's New"** root-search item. v1.72.0 (24 Apr 2024): "Auto-dismiss the 'See What's New' item after one day": https://www.raycast.com/changelog/macos-v1/4.
- v1.65.0 added an action to copy a link to the release notes from the Changelog command (same page).
- v2 beta:
  - 0.52 (3 Apr 2026) "Added support for What's New item in compact mode": https://www.raycast.com/changelog/macos/0-52.
  - 0.53 (10 Apr 2026) "Added a setting to the Changelog command to disable 'What's new?' items in the root search": https://www.raycast.com/changelog/macos/0-53.
- Silent Updates have no release notes (v1.21.0 above).

### 1B. Raycast for Windows (beta until 2.0 on 25 Aug 2026)

**The mechanism: Microsoft Store / MSIX, plus winget.**
- v0.37 (19 Nov 2025): "we are moving into the Microsoft Store… you will be migrated into a new version of our app, which is distributed through the Microsoft Store… The app now updates itself automatically in the background, without interrupting your work": https://www.raycast.com/changelog/windows/0-37.
- v2.1 (1 Sep 2026), Updater: "Raycast now relaunches itself after an update installed by the Microsoft Store or winget, instead of staying closed until the next logon": https://www.raycast.com/changelog/windows/2-1.
- v2.3 (11 Sep 2026): "Fixed Raycast not starting at login when a Microsoft Store update was still being registered": https://www.raycast.com/changelog/windows/2-3.
- v2.7 (9 Oct 2026): "Fixed slow restarts after Microsoft Store updates on some systems": https://www.raycast.com/changelog/windows/2-7.
- v2.8 mentions "Raycast's package sandbox", which points to an MSIX package: https://www.raycast.com/changelog/windows/2-8.
- Third parties list winget id `Raycast.Raycast` with MSIX installers hosted on raycast-releases.com: https://wingetgui.com/apps/Raycast-Raycast and https://winstall.app/apps/Raycast.Raycast (secondary).
- Troubleshooting: "In Raycast, search for `Check for Updates`, or open the Microsoft Store, go to `Library`, and update Raycast from there… Microsoft sometimes takes a while to roll out updates regionally": https://manual.raycast.com/troubleshooting.md.

**When it checks.**
- 2.0 (25 Aug 2026): "Raycast will update in the background or you can run check for updates like you usually do": https://www.raycast.com/changelog/windows/2-0.
- v0.20 added "a Check For Updates button inside About settings": https://www.raycast.com/changelog/windows/0-20.
- How often Raycast itself polls on Windows is **unconfirmed**. Store installs also follow the Store's own automatic-update schedule (inferred from the Store-related entries above).

**How the user learns an update is ready.**
- Windows manual, "Installing Updates": "Whenever a new update is available, we will notify you directly in root search. You will be able to hit Enter to download the update, and then install and restart Raycast. You can also use the Check for Updates command to manually verify whether you have the latest available version." Source: https://manual.raycast.com/windows/product-updates (the page now redirects to the manual's home; quoted from the archived 13 Mar 2026 copy, http://web.archive.org/web/20260313052915/https://manual.raycast.com/windows/product-updates).
- The root item is called **"Install Update"**. v0.36 (15 Nov 2025): "Starting with the 'Install Update' item, we are now showing relevant items in compact mode, so next time you update you should see it in compact mode": https://www.raycast.com/changelog/windows/0-36.

**How it installs.**
- Enter downloads, then installs, then restarts Raycast (manual above). Background Store or winget updates relaunch Raycast by themselves since 2.1.
- Pane's earlier research noted "the Store build installs silently and relaunches without a window": `docs/research/raycast-deep-dive.md:223-224`. That is secondary and its source is not cited there.

**What's New.**
- Manual, "Changelog": "After an update is installed, you will be able to see a Changelog entry which will tell you all about what's new in that release… You can also always refer to previous entries through the Changelog command in Raycast" (archived Windows manual page above).
- v0.52 (3 Apr 2026): "You can now disable change-log notifications in Settings > Raycast > Changelog": https://www.raycast.com/changelog/windows/0-52.
- v0.65 (18 Jun 2026): "'What's New' now appears only for notable releases": https://www.raycast.com/changelog/windows/0-65.

### 1C. What Raycast's behaviour comes down to
1. It checks by itself and often: at least daily on macOS. On Windows it updates in the background through the Store.
2. It downloads in the background without asking.
3. It says so in root search, at the top, as a highlighted item ("New Update Available" / "Install Update"). Enter installs.
4. Installing relaunches Raycast automatically. Later versions also install automatically in the background, except while long-running work is active, and relaunch even after a Store or winget update.
5. After the new version starts, a What's New / changelog entry appears. It is dismissed after a day, shown only for notable releases, and can be turned off.
6. There is no documented user toggle to turn updates off.

---

## Part 2: Pane today (Windows, v0.1.0 → 0.1.1)

### Step by step
1. **Wiring at start.** A release build always reads `api.github.com/repos/pane-app/pane/releases?per_page=10` (`crates/pane/src/main.rs:281-309`, `crates/pane-core/src/releases.rs:153-154`). Wiring also deletes what an earlier update left behind (`pane.exe.old`, the `update\` folder): `crates/pane-core/src/launcher/application_update.rs:256-274` → `crates/pane-core/src/application_update.rs:186-191`.
2. **One check, at process start only.** `main.rs:509-516` spawns `check_application_update()` once. There is no timer, no interval and no check when the window is shown or woken. `docs/installer.md:605-612` says "Once, when Pane starts… That cadence is provisional; no interval is checked meanwhile", and ADR 0048 says the same (`docs/adr/0048-pane-updates-itself-from-github-releases.md:9`).
3. **What the check shows** (`launcher/application_update.rs:308-356`):
   - Newer release found: the state becomes `Offered`, and the status line says "Pane 0.1.1 is available" (`:332-336`).
   - Up to date: the state becomes `Current`, with **nothing shown** at start (`:337-343`).
   - Failure (including GitHub's rate limit): a status-line error, plus a "Check for a Pane update" row (`:344-349`, `:169-178`; rate-limit detection is at `releases.rs:300-309`).
4. **Where the offer appears.**
   - **Root search row.** "Update Pane to 0.1.1", with the subtitle "Your extensions and settings are kept; the new version is used the next time Pane starts" (`launcher/application_update.rs:154-168`).
     - The row is added after every built-in and extension command (`crates/pane-core/src/launcher.rs:5110-5124`).
     - With an empty query, root search sorts everything by frecency, then alias, then kind, then provider, then **title A–Z** (`crates/pane-core/src/search.rs:628-637`, `:788-795`). The row counts as an ordinary Command (`crates/pane-core/src/launcher/presentation.rs:537-551`) and starts with zero frecency, so "Update Pane…" sorts among the "U"s near the **bottom** of the Commands section. Nothing pins or highlights it.
   - **Status line.** One slot (`launcher/acquire.rs:647-653`). It is replaced by the next status from anything, for example the first-setup acquisitions spawned beside it (`main.rs:505-508`). It resets to Idle whenever a new view is made (`launcher.rs:584-590`). It is only visible while the launcher window is open. There is no toast, no HUD, no tray badge and no Windows notification.
   - **Settings → About.**
     - Status "Pane 0.1.1 is available" with an "Update to 0.1.1" button ("Keeps your extensions and settings. The new version starts next time.").
     - Or "Check for updates", which runs the same check but answers "Pane is up to date" when there is nothing (`crates/pane/src/features/settings/about.rs:180-243`, `:409-431`).
5. **Download.** This only happens when the user picks the row or the button. It fetches `.sha256`, then the zip, with progress on the status line ("Downloading Pane 0.1.1: …"), then checks the sha256 (`launcher/application_update.rs:368-416`, `application_update.rs:327-373`). The ADR states it outright: "Only the user's choice downloads anything (US76 stands: Pane never installs, or restarts, on its own)" (`docs/adr/0048…:11`).
6. **Apply.** The zip is unpacked into `%LOCALAPPDATA%\Pane\update\`. The running `pane.exe` is renamed to `pane.exe.old` (Windows allows renaming a running exe), and the new exe takes its name. If that fails, the old one is put back. Then the Inno Setup Apps entry's `DisplayVersion`, `MajorVersion` and `MinorVersion` are rewritten (`application_update.rs:405-511`; Apps entry at `:500-509`).
7. **After the swap: no restart.** The status line says "Installed Pane 0.1.1; the new version is used the next time Pane starts" (`launcher/application_update.rs:425-436`). About says "Pane 0.1.1 is installed and starts next time" (`about.rs:238-242`). The code is explicit that "Pane never restarts itself" (`launcher/application_update.rs:24-25`, `application_update.rs:17-23`, `CONTEXT.md:372`, `docs/installer.md:670-674`, `:818-823`).
   - No root row offers a restart.
   - To get 0.1.1 the user must quit through the tray's **Exit** (`crates/pane-core/src/tray/windows.rs:755-756`) and start Pane again from Start.
8. **Next start.** The new exe runs and deletes `pane.exe.old`. Nothing announces the new version: no What's New, no "Updated to 0.1.1".

### Why the user was probably never offered 0.1.1 at start
- **Timing (most likely).** The GitHub API shows v0.1.0 published 2026-10-23T23:07Z and **v0.1.1 published 2026-10-24T02:49Z**, about 3h40m later (`gh api repos/pane-app/pane/releases`).
  - Pane checks only when its process starts. A Pane started before 02:49Z (from the setup's "Start Pane", at sign-in, or left running in the tray) found itself up to date, said **nothing** (`launcher/application_update.rs:337-343`), and never checked again.
  - Opening and closing the launcher window does not restart the process (the window hides; the process stays in the tray), so the user never got a fresh check until they clicked "Check for updates".
- **Even when found, it is easy to miss.**
  - The root row sits near the bottom of the Commands section in alphabetical order (see 4).
  - The status-line text is overwritten by the next status and is not visible while the window is hidden.
  - There is no toast, notification or tray badge.
- **Not the cause, but possible.**
  - The rate limit (60 unauthenticated requests per hour per IP, `releases.rs:300-309`) only matters with many restarts or a shared IP. It would have shown an error and a "Check for a Pane update" row, not silence.
  - There is no cache of the check result between runs, so the cache is not a factor either.
  - The `prerelease` flag does not matter: 0.1.1 is a full release, and prereleases are kept anyway.
- **Why the user had to reopen Pane by hand.** The swap on purpose leaves the old process running (US76 / ADR 0048 / installer.md:818-823), and nothing offers a restart.

### Related facts that matter for a redesign
- **No single-instance guard.** Nothing stops a second `pane.exe` from starting.
  - The code only copes with collisions afterwards: the file index lock ("Another Pane is using file search on this computer", `crates/pane-core/src/file_index/indexer.rs:2765`) and the local channel ("another Pane already listens there", `crates/pane-core/src/local_channel.rs:821`).
  - Hotkey registration would also collide.
- **The setup already handles a running Pane.**
  - `scripts/setup-windows.iss:62-70`: `CloseApplications=force`, `CloseApplicationsFilter=*.exe,*.exe.old`, `RestartApplications=no`.
  - Its `[Run]` entries start Pane again after a non-silent install, except under `skipifsilent` (`:99-104`).
- **The Apps entry** is already kept in step by the swap (`application_update.rs:500-509`).

---

## Part 3: Proposal to match Raycast

This reverses the recorded decision **US76** ("Pane never installs, or restarts, on its own") and parts of ADR 0048, CONTEXT.md:372 and docs/installer.md. It needs a new ADR that supersedes those points, plus glossary updates, before code.

### Changes
1. **Periodic background check.**
   - Check at start, then every 6–24 h while running.
   - Also check when the launcher is shown, if the last check is older than N hours. Keep `last_checked` in the data folder so restarts do not burn the rate limit.
   - Use `If-None-Match`/ETag on the releases listing: a 304 answer does not count against GitHub's limit for conditional requests made with an ETag, which should be verified.
   - Keep the existing rate-limit backoff.
2. **Auto-download after the check.**
   - Download and sha256-verify silently as soon as an offer is found, keeping the bytes or a staged folder in the cache. Only staging is automatic; the swap is not.
   - Optional setting "Download updates automatically" (default on). Raycast has no toggle, but Pane's GPL and dev audience may want one.
3. **A pinned, highlighted root item.**
   - Before the download: "Update Pane to 0.1.1". Once it is ready: "Restart to Update Pane (0.1.1)".
   - Hoist it above frecency for the blank query (as aliases are hoisted, `launcher.rs` `Ranked.before`), give it its own section or accent, and show it in compact mode.
   - Keep the subtitle that extensions and settings are kept.
4. **A toast when the update is ready.** The toast should outlive the status line, plus optionally a tray badge or tooltip ("Pane 0.1.1 is ready – restart to update") and a Windows toast notification if the launcher is hidden. Settings → About mirrors it with a "Restart to update" button.
5. **Apply and relaunch automatically.** When the user chooses "Restart to Update":
   - swap (as today);
   - spawn the new `pane.exe --updated-from <old-version> --wait-pid <pid>`;
   - quit cleanly (hotkeys released, tray removed, `quit_cleanly`).
   - The new process waits for the old PID to exit before it registers hotkeys, the tray, the file index and the local channel, then shows the launcher.
   - Optionally also apply on the user's normal quit, or at idle (Raycast holds off while long work runs; Pane should hold off while a command, service or install is running).
6. **What's New after restart.**
   - Read `--updated-from` (or compare a stored `last_run_version` with `APP_VERSION`, which also covers updates installed by the setup).
   - Show a root item "See What's New in Pane 0.1.1" that opens the GitHub release body for `v0.1.1` (already in the API listing) as a detail view, or opens the release page. Auto-dismiss it after a day.
   - Add a toast "Updated to 0.1.1".
7. **Status wording.** Replace "the new version is used the next time Pane starts" with "Restart to update". Never leave the user in an "installed but not running" state without an action to fix it.

### Main risks on Windows
- **Replacing a running exe.**
  - Renaming works (that is what Pane already does).
  - Deleting `pane.exe.old` fails while the old process lives, so the new process must retry later (it already does, best effort).
  - Antivirus or Defender may lock the freshly written exe. Retry the rename with backoff, and verify the new exe runs (`pane.exe --version`) before quitting the old one.
  - If the relaunch fails, Raycast 2.8 had to add "relaunches itself if an update or restart fails after the app was already closed". Pane needs the same fallback: a watchdog in the new process, or a scheduled restart via the Run key/Task Scheduler.
- **Single-instance handoff.**
  - There is no guard today, so for a moment two Panes run.
  - Hotkeys (`RegisterHotKey`/the keyboard hook), the tray icon, the file-index lock and the local channel all collide.
  - The new process must wait for the old PID, or a named mutex, before it acquires them.
  - Adding a real single-instance mutex (and "second launch → show the existing window") would also fix double starts from Start and the setup's `[Run]`.
- **The installer's Apps entry.**
  - The swap already updates `DisplayVersion` (`application_update.rs:500-509`).
  - Keep the Inno AppId stable. Make sure the uninstaller's `ClosePanes` and `CloseApplicationsFilter` still find a process that was relaunched from `pane.exe`.
  - If the update is later switched to running `Pane-Setup-x.exe /VERYSILENT` instead of the swap, `RestartApplications=no` and `skipifsilent` mean the setup will **not** restart Pane. Pane would need to relaunch itself after the setup exits, which is the same problem Raycast fixed in Windows 2.1.
- **Unsigned binaries.** Signing is still a release prerequisite (ADR 0048 "Trust"). Auto-downloading and auto-relaunching unsigned exes raises the stakes: SmartScreen/Defender warnings on the relaunched exe, and sha256 from the same release only guards against corruption. Sign before making updates automatic, or at least verify a detached signature.
- **Rate limit.** A periodic check across many users behind one NAT can hit 60 requests per hour. Use ETag, jitter and a long interval.
- **Data compatibility.** Silent auto-apply must never run while an extension install or update, a clipboard batch write, or a command is mid-flight. Gate the restart on idle, as Raycast does with Dictation and AI agents.
