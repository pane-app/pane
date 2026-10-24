# Pane downloads its updates by itself and restarts into them

Accepted 2026-10-24 by the user's decision that Pane updates itself as Raycast does ("yes, do like Raycast"), after the first real update, 0.1.0 to 0.1.1, worked but was clunky. It supersedes the parts of [ADR 0048](0048-pane-updates-itself-from-github-releases.md) and of the [specification](https://github.com/pane-app/pane/issues/1) that kept Pane from checking more than once, from downloading before the user chose, and from restarting itself:

- US76, "so that the app does not update or restart itself without my action";
- decision 17's "automatic application downloads or restarts are not accepted";
- scenario T16's "download/install/restart behavior does not silently adopt automatic app updates";
- the non-goal "automatic application update installation/restarts";
- Q38 ("do not infer automatic application downloads or restarts").

What still holds from ADR 0048: the release source, the assets and their names, the choice of release, the checksum, the redirect rules, the rate-limit handling, the trust note and the stand-in for tests. [ADR 0049](0049-an-appimage-updates-from-the-releases-appimage.md) stands unchanged. Installing still waits for the user: what changes is that everything before the install happens by itself, and the install ends with Pane running the new version.

**Why.** Under ADR 0048, Pane checked only when its process started. Closing the launcher hides it, and the process keeps running in the tray. So a Pane started before 0.1.1 was published, 3 hours 40 minutes after 0.1.0, never heard of 0.1.1. When a check did find it, the offer was an ordinary row, sorted by title near the bottom of the Commands, plus a status line that the next status replaced. After the install, the user had to quit from the tray and start Pane again by hand, and nothing said what had changed. Raycast instead:

- checks by itself, at least daily;
- downloads in the background without asking;
- puts a highlighted item at the top of root search;
- relaunches itself when the user presses Enter on it;
- shows what's new for a day afterwards.

**The check.** Pane checks at start, then every 4 hours while it runs. It also checks when the launcher is shown, if the last check is more than 4 hours old. Each scheduled check is delayed by a random amount of up to 15 minutes, so that Panes sharing one address do not all ask at once. Pane keeps a record in its cache folder (`application-update.json`) holding:

- the time of the last check;
- the listing's `ETag`;
- what the choice of release needs from the listing: the tags, this system's assets, and the newest release's notes.

So a restart does not check again before a check is due. A listing that has not changed is answered `304 Not Modified`, and nothing in it is read again. Pane does not rely on GitHub leaving a 304 for an address that is not signed in out of the 60 requests an hour, because six scheduled checks a day stay inside the limit either way. A used-up limit is waited out until it lifts, as ADR 0048 says.

A background check that fails says nothing. It is written to Pane's log, About shows when and why it failed, and Pane tries again when the next check is due. It lists no row and puts nothing on the status line. Only a check the user asked for explains a failure.

**Check for Pane Updates.** This root search command is always listed when a release source is configured. Its alternate title, "Check for Updates", lets Raycast's name find it. It checks at once, whatever the schedule, as About's "Check for updates" does, and it answers even when nothing is newer ("Pane is up to date"). It replaces the "Check for a Pane update" row, which was listed only after a check failed. Extension updates keep their own command, Check for Extension Updates.

**The download.** When any check finds a newer release, Pane downloads it at once in the background, without asking. It fetches the `.sha256`, then the package. It checks the package against the digest, unpacks it and stages it, as ADR 0048 and ADR 0049 say. Last, it writes a marker that names the staged version.

- While it downloads, nothing the user sees changes, except About, which shows the progress. After a check the user asked for, a toast shows the progress too.
- A failed download says nothing, like a failed background check, and is tried again at the next check.
- A staged update survives a restart. At start, Pane keeps a staging folder whose marker names a version newer than the one running, and removes anything else, as before.
- Pane downloads one update at a time. A newer release replaces an update already staged.
- No setting turns the download off; Raycast has none.
- A development build still checks only a stand-in that `PANE_RELEASES` names.

**Ready: the pinned row and the toast.** Once an update is staged, root search lists **Restart to Update Pane**. Its subtitle reads "Pane <version> is ready. Your extensions and settings are kept".

- **Where it is.** For a blank query it comes first, above the pinned home, under a section of its own ("Update"). It is drawn with the accent colour so that it stands out, and compact mode shows it too. For a typed query it is matched like any root result, for example by "update" or "restart".
- **Selection.** Because it is first, it is selected when the launcher opens on a blank query, so Enter restarts, as in Raycast. A restart costs a few seconds and loses nothing.
- **The toast.** "Pane <version> is ready", with Restart as its action, shown once: when the update is staged while the launcher is open, or otherwise the next time the launcher opens. It is never shown as a HUD over other applications while Pane is hidden.
- **About** shows the same state, with a "Restart to update" button.
- **While Pane is busy.** While Pane is installing, updating or acquiring an extension, the row is unavailable and says why, until that work ends.

**Restart.** Enter on the row, the toast's action, or About's button:

1. Swaps the program as ADR 0048 says. This includes rewriting the Windows Apps entry's `DisplayVersion`, `MajorVersion` and `MinorVersion`, and the macOS bundle's `Info.plist` version, so Settings → Apps and Finder name the new version.
2. Runs the new program with `--version` and expects the new version back. If it does not answer, Pane undoes the swap (the old program goes back under its own name), explains what happened, and the row stays.
3. Starts the new program with `--updated-from <old version> --wait-pid <this process's id>`. On Windows that is `pane.exe` at the install path; on macOS, the bundle through Launch Services (`open -n`); on Linux, the program or the AppImage file.
4. Waits up to 20 seconds for the new process to report that it has started. If it does not (it could not be started, or it exited), the old Pane undoes the swap, says "Pane <version> could not start, so Pane stays at <old version>", and keeps running. The row stays, so the user can try again.
5. Quits cleanly, as the tray's Exit does: it releases its hotkeys and the keyboard hook, removes the tray icon, and records the clean quit.

**The handoff.** The new process takes nothing the old one holds until the old process has exited: no hotkey, keyboard hook, tray icon, local channel, file index lock or clipboard listener. It waits up to 30 seconds. After that, it ends the old process, but only if that process still runs the old program from Pane's install folder, so that a reused process id is never ended. Then it starts as usual and shows the launcher with the toast "Updated to Pane <version>".

**If the new version fails after the old one has gone.** The renamed old program (`pane.exe.old`, `pane.old` or `<name>.old`) is kept until the new version has settled, meaning its window, hotkeys and tray icon are all up. Only a start after a settled run removes it, so a failed update can still be undone by hand. A new version that ends unexpectedly before it settles leaves the crash notice for its next start. Rolling back by itself after a failed start stays out of scope, as the specification says.

**What's New.** Pane records, in its data folder, the version each start runs. A start on a different version than the last one (found from `--updated-from`, or from that record, which also catches an update the setup installed) lists **What's New in Pane <version>**, pinned first like the restart row. It stays listed for 24 hours from that start, until the user opens it, or until they dismiss it from the Actions panel. Enter shows the release's notes in a detail view, rendered as Markdown, with an action that opens the release page. The notes are the GitHub release's body, taken from the listing the check kept, or else read once. A first start on a fresh data folder lists none. A data folder that has Pane's data but no record yet belongs to a Pane from before this flow, so its first start counts as an update and lists the row. When both rows apply, Restart to Update Pane comes first.

**Rejected or deferred.**

- **Installing without the user's Enter**, while Pane is idle or when it quits, as Raycast's macOS updater now does. Deferred while Pane's packages are unsigned. The `.sha256` comes from the same release, so it guards only against a damaged download. A program swapped and started without anyone pressing Enter would run whatever the repository published. Windows SmartScreen may also question a newly written unsigned program with nobody there to answer it. Revisit once packages are signed (ADR 0048's Trust). Then, install only while no command, service or install runs, as Raycast holds off while Dictation or AI agents work.
- **Running the new setup silently** instead of the swap. Unchanged from before (see [installer](../installer.md)): the setup would not start Pane again anyway (`RestartApplications=no`, `skipifsilent`), and a Pane installed from the zip has no setup.
- **A setting that turns checks or downloads off.** Raycast has none, and a development build already checks nothing by itself. It can be added if users ask.
- **A setting that hides What's New**, which Raycast has. Not now.
- **A system notification or a tray badge** for a ready update. Raycast shows neither.
- **A row offering the update before it is downloaded** ("Update Pane to <version>", whose Enter started the download). Raycast lists its item only once the update is ready, and a download the user has to start is the step this decision removes.
- **A watchdog that starts Pane again** when the new version fails after the old one has exited (Raycast added one in its macOS 2.8). Deferred, with automatic rollback.
- **A general single-instance guard**, where a second `pane` started while one runs shows the running one's launcher and exits. It would also stop double starts from Start or the setup's last page. It is a separate decision: the handoff here needs only the old process's id. Note that the glossary keeps the local channel for `pane-ext`, not as a single-instance channel.

**Consequences.**

- The superseded text is marked in ADR 0048, the glossary's Application update, and [installer](../installer.md#updating-pane-itself). The installer page describes 0.1.1's behaviour until the build lands.
- The smokes' update phases (#54–56) check that nothing is downloaded until the row is chosen, and that the new version is used the next time Pane starts. They change to check a download Pane makes by itself, the restart, and a relaunched Pane reporting the new version.
- The tests need a schedule they can shorten. A development build can be given a shorter interval, as it is given `PANE_RELEASES`.
- A running Pane now wakes every 4 hours for one request, which is negligible next to the idle measurements.
- Pane 0.1.1 and earlier still update the old way. The first update to a version with this flow needs one restart by hand.
