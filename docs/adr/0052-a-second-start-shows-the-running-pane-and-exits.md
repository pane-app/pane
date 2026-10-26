# A second start shows the running Pane and exits

Accepted 2026-10-25 by the user's standing rule that Pane does what Raycast does ("do like Raycast"). It settles the "general single-instance guard" that [ADR 0050](0050-pane-downloads-its-updates-by-itself-and-restarts-into-them.md) deferred ([#542](https://github.com/pane-app/pane/issues/542)).

**Why.** Starting `pane` while Pane ran (from Start, the setup's last page, or a second double-click) started a second Pane: two launchers, two keyboard hooks, two tray icons and two sets of hotkeys fighting for the same keys. Raycast runs one instance. Opening it again brings up its window.

**What.** Pane holds an **instance lock** on its data folder for as long as it runs, and listens for later starts beside it. A start that finds the lock held is a **second start**:

- A plain second start asks the running Pane to show its launcher, as the tray's Open Pane does, and exits. A second `pane --install <source>` asks it to show that package for installation too, with a folder made absolute first.
- A second start with `--hidden` or `--launch-at-login` exits once the running Pane answers, asking nothing: neither shows anything of its own.
- A Pane that is quitting stops answering, but holds the lock until the system has ended it, which can take a few seconds (the smokes saw it on Linux). A start meanwhile keeps trying the lock and runs as the one Pane once the lock is free, so quitting Pane and starting it again at once works.
- If the lock stays held and nobody answers for 10 seconds, the start runs anyway, without the lock, and says why in its log. A Pane that never answers is broken or stuck, and the user who started Pane gets one.
- `pane --version` and the selected-text worker exit before the lock is looked at.
- The new process of a restart into an update (ADR 0050, ADR 0051) waits for the old process by its id first, as before, and only then takes the lock the old one let go of.
- A second start writes nothing: it exits before the crash record, the log and What's New's record. The running Pane's log says it was started again, so a start that exited is never silent. A start that runs without the lock writes why to its log as soon as the log is open.

On macOS, opening `Pane.app` while it runs never starts a second process (Launch Services activates the one running), so Pane shows its launcher when the system says it was opened again. Starting the bundle's program directly is a second start like any other.

**How.**

- Windows: the named mutex `Local\pane-instance-<key>` (`Local\` is the sign-in session), owned by Pane's main thread, and the named pipe `\\.\pipe\pane-instance-<SID>-<session>-<key>`, open to the user alone as the local channel's pipe is. The second start lets the running Pane take the foreground it was given (`AllowSetForegroundWindow`).
- macOS and Linux: an exclusive `flock` on `instance-<key>.lock` and the Unix-domain socket `instance-<key>`, in the local channel's folder of the user's own (`pane` in `$XDG_RUNTIME_DIR`, else `pane-<uid>` in the temporary folder).
- `<key>` is a hash of the data folder's absolute path. A Pane with a data folder of its own (`PANE_DATA_DIR`, as the tests, the smokes and a development Pane beside an installed one use) runs beside the others. Two Panes never share one data folder.
- The system lets go of the lock when Pane ends, however it ends, so a crash never blocks the next start.
- The second start sends one JSON line (`{"request":"show"}`, with an `install`; an empty line for a quiet start) and waits for `ok`, trying the lock and the endpoint again for up to 10 seconds, since the running Pane may itself still be starting or quitting. A request a later Pane added is read as a plain show.

**Rejected.**

- **The local channel** for this. The glossary keeps it for `pane-ext`, a test or development Pane may move it with `PANE_CHANNEL`, and its requests carry a version an older Pane refuses. The instance endpoint shares only its platform code.
- **One Pane per user** rather than per data folder. The installed Pane's data folder is the user's, so this is the same for users, while the smokes and a development Pane keep running beside an installed one.
- **Telling the running Pane by finding its window.** GPUI's window takes no message of Pane's own, and macOS and Linux need a listener anyway.

**Consequences.**

- [Building and running Pane](../building.md#one-pane-at-a-time) and the [installer's concurrent Panes](../installer.md#concurrent-panes) say so. The glossary adds Instance lock and Second start.
- The smokes start Pane many times in sequence. Each start comes after the one before was stopped, though not always after the system finished ending it, so a start may wait a moment for the lock. They don't start Pane twice at once.
- The guard's tests (`pane_core::instance`) cover the lock, the request and its answer, a quiet start, a lock let go of or abandoned, a start that takes the lock a quitting Pane lets go of, and a holder that never answers. On macOS and Linux they also cover a socket file a killed Pane left (replaced by the next Pane), a held lock with only such a file (the start runs after the limit), and a socket that queues the connection but never takes it (connected is not answered; the start runs after the limit, and the asking connection times out too).
