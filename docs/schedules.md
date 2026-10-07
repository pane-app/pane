# Scheduled work

Added for [#47](https://github.com/hoangvu12/pane/issues/47) (US14, US45,
US57, US58, US79; T02, T09, T17; contributions to G3, not a claim that it
passes). A command can declare, in its package's manifest, that it runs on
a schedule: Pane runs the command's action of the item the schedule names
every interval, while the package's code may run — it is enabled and not
[paused](pausing.md) — without the user asking, and shows the answer on the
command's screen. Disabling the package stops the schedule; enabling it
starts it again; a restart schedules again whatever the manifest declares.
The architecture is recorded in [ADR 0024](adr/0024-the-host-runs-scheduled-extension-work-by-its-own-clock.md)
(proposed).

## Where it lives

- **The manifest declaration**, in the command's entry in `pane.json`: a
  `schedule` naming an interval and the item whose action runs, such as
  `"schedule": { "everySeconds": 900, "item": "check" }`. One schedule kind
  only: a fixed interval. The interval is at least 1 second and at most 30
  days, and the item id at most 256 characters (`MIN_SCHEDULE_SECONDS`,
  `MAX_SCHEDULE_SECONDS` and `MAX_SCHEDULE_ITEM` in
  [`packages.rs`](../crates/pane-core/src/packages.rs)); anything else, or
  a schedule without an `item`, is refused when the package is previewed
  or installed, before anything is installed. A command that searches as
  the user types (`"search": true`) or computes root results
  (`"rootResults": true`) may be scheduled as well: the schedule runs its
  action, not those interfaces.
- **The host scheduler**, in the core
  ([`launcher/schedules.rs`](../crates/pane-core/src/launcher/schedules.rs)):
  a thread of Pane's own, which wakes when a run is due and starts each on
  a thread of its own, so neither the window nor the scheduler waits for a
  guest. It is driven by the launcher's clock, the same one clipboard
  history follows ([`crate::clipboard::Clock`](../crates/pane-core/src/clipboard.rs)):
  the system's clock, or the one tests and development builds give through
  `Launcher::with_clock` (release builds have no way to replace it), which
  tells it when the clock is set other than by time passing. It looks
  again whenever a package's [generation](generations.md) changes, since
  every path that begins or ends one tells the extension data's change
  hooks.
- **The guest needs nothing new.** The scheduled run is the command's
  action of the item the schedule names, exactly as running that item from
  its list is (Pane asks for the command's tree, then hands the item's
  action's callback to `handle-event`, see [list-tree.md](list-tree.md)):
  the answer text is the result, an error
  it answers with is an expected error, and a trap is a crash of the
  package. The item is usually one the command lists, so the user can run
  it too.
- **The author examples** are
  [`guests/sample-schedule`](../guests/sample-schedule) (Rust) with the
  same command in
  [JavaScript](../guests/sample-schedule-js) and
  [TypeScript](../guests/sample-schedule-ts), packages
  [`guests/packages/sample-schedule`](../guests/packages/sample-schedule)
  and its `-js`/`-ts` copies: the **Counting** command counts its runs in
  its content and answers the new count, and its items stand for the ways
  a run can end ("Run slowly" waits ten seconds, so a disable or reload
  while it runs stops it; "Answer an error" answers an error; "Crash"
  traps).

## Behavior

- **Lazy.** Nothing of a package runs because it is installed: its
  schedule activates the command only when a run is due, starting the
  guest instance then (as opening the command would), and no other package
  is activated by it. A command unavailable on this system is never
  scheduled, like an action the user cannot invoke.
- **While the code may run.** The schedule runs while the package is
  enabled and not paused, from the moment its code may run: Pane starts,
  the package is installed or enabled, or its code is replaced by a reload
  or an update (the interval restarts with the new code). A disable, an
  uninstall, a pause after a failure, or a code replacement ends it, and
  enabling the package again starts it from a full interval.
- **The generation owns the run.** Each run belongs to the package's
  [generation](generations.md) current when the scheduler asked for it:
  a disable, reload, update, uninstall or pause that happens while it
  runs stops it (the runtime stops the call, and its instance goes), and
  its late answer is discarded, never shown. The sample's "Run slowly"
  never saves "finished" when it is stopped.
- **At most one run at a time.** Ticks that fall due while a run has not
  answered are coalesced: no second run of the command is asked for
  meanwhile, and the next run starts at the next tick after it answers.
  A run that never ends (a guest waiting on a clock) holds only its own
  command's schedule, as any call does ([pausing](pausing.md)).
- **The answer, on the command's screen.** When the run answers, the
  command's screen is the one on display, and its generation has not
  ended, the answer shows as an action's answer does: the text as the
  result, an error the extension answered with as an error. While the
  command is not open the run still happens, and opening the command
  shows its effect (the sample's count, kept in its content).
- **Crashes count like any action's.** A scheduled run that traps is a
  crash of the package, like running the item by hand: the third crash
  within five minutes [pauses](pausing.md) the package, which ends the
  schedule with the generation; Retry starts it again. An error the
  extension answers with, however often, never pauses it.
- **Across restarts.** The schedule comes from the manifest, not from a
  record, so a restart schedules again whatever the installed copy
  declares for packages still enabled. Work that fell due while Pane was
  not running is not replayed: the first run after a start is one full
  interval after it.
- **Expected errors before anything is installed.** A manifest whose
  schedule is impossible (`everySeconds` below 1 or above 2,592,000, no
  `item`, an `item` longer than 256 characters, or an unknown field) is
  refused with the reason, and nothing is installed or scheduled.

## Limits

- The scheduled run is the action of an item the manifest names: the
  minimal choice the ticket allows, provisional pending the user's
  confirmation. A scheduled command cannot be asked for its view instead,
  or run work no item answers, and one schedule per command is all the
  manifest offers.
- The interval bounds (1 second to 30 days) and the restart behavior (no
  replay of work that fell due while Pane was stopped, or while the
  package could not run) are provisional, as the specification's
  "scheduling intervals are not yet selected".
- The clock seam is the one clipboard history introduced
  (`Launcher::with_clock`); its provisional home is `clipboard.rs`, and
  release builds cannot replace the system's clock, so scheduled work runs
  by the system's time there.
- A guest waiting on a clock (the sample's "Run slowly") is never stopped
  until its generation ends, as for any call; a scheduled command whose
  item waits forever holds the runtime like any such call.
- Only one kind of schedule exists: a fixed interval from when the code
  may run. Calendars, one-shot delays and minimum-battery policies are
  future work, as is a per-command way to turn a schedule off without
  disabling the package.
- Like every Pane record on the data folder, schedules are not coordinated
  between two Pane processes on it: both would run a scheduled command.
- Nothing here is platform-specific (it lives in `pane-core`); it has run
  on Linux, and runs in `cargo xtask ci` on Windows, macOS and Linux.

## Author example and tests

- [`guests/sample-schedule`](../guests/sample-schedule) (Rust): the
  **Counting** command, whose package declares
  `"schedule": { "everySeconds": 60, "item": "count" }`. Its "Run now"
  item is what the schedule runs; "Run slowly" waits ten seconds after
  saving "started", then saves "finished"; "Answer an error" answers an
  error; "Crash" counts the run, then traps.
- [`guests/sample-schedule-js`](../guests/sample-schedule-js) and
  [`guests/sample-schedule-ts`](../guests/sample-schedule-ts): the same
  command in JavaScript and TypeScript, built into
  `guests/prebuilt` by `cargo xtask js-guests` as the other JS/TS samples
  are.
- [`crates/pane-core/tests/schedules.rs`](../crates/pane-core/tests/schedules.rs):
  the same checks in all three languages, with a clock the tests set:
  installation alone activates nothing until
  a run is due, and no other package's code runs; the answer and an error
  the extension answers with show on the command's screen; disabling
  stops a run still pending (its "finished" is never saved, its answer is
  not shown over the disable's own outcome, and ticks that pass meanwhile
  run nothing), and enabling starts the schedule again from a full
  interval; ticks that fall due while a run is pending start one run, not
  many, and coalesce into one after it answers; a restart schedules again
  whatever the manifest declares, from a full interval, without replaying
  work that fell due while Pane was stopped; a package disabled before a
  restart is not scheduled after it; reloading stops a pending run and the
  replacement's schedule runs again; uninstalling stops the schedule; a
  scheduled run that traps counts as a crash, so three within five
  minutes pause the package and stop the schedule, and Retry restarts it;
  a scheduled run that computes without yielding (the compute limit
  shortened, as #18's unresponsive tests do) is stopped by Pane, counted
  as a crash and pauses it the same way; and an impossible schedule,
  including an over-long `item`, is refused before anything is installed.
  The scheduler itself is waited for deterministically
  (`Launcher::wait_for_schedules`, as clipboard expiry's
  `wait_for_clipboard_expiry`): the clock moves only when a test moves it,
  and the test waits until every look and run it caused has settled. Where
  a test must see an effect inside the guest — a save landing while a run
  is still pending — it polls the package's data with a deadline, and one
  check gives a wrongly started second run a moment to show itself.
