# Continuing services

Added for [#48](https://github.com/pane-app/pane/issues/48) (US14, US45,
US57, US79; T02, T09, T17; contributions to G3, not a claim that it
passes). A command can declare, in its package's manifest, that it runs a
continuing service: Pane runs the service's cycles while the package's
code may run — it is enabled, not [paused](pausing.md) and not
[waiting](dependencies.md#waiting-for-a-required-dependency) for a required
dependency — without the
user asking and at no interval the manifest declares, since each cycle
answers the status to show and how long to wait before the next. A user
starts a service by installing or enabling its package, sees its status
on the command's screen, and stops it through extension management:
disabling the package stops the service; enabling it starts it again; a
restart starts it again where the package is enabled. The architecture is
recorded in [ADR 0025](adr/0025-a-continuing-service-cycles-at-its-own-cadence.md)
(proposed).

## Where it lives

- **The manifest declaration**, in the command's entry in `pane.json`:
  `"service": true`. One service per command, the minimal shape; a package
  whose several commands each declare one runs several services, exactly
  as several commands may each declare a [schedule](schedules.md). A
  command that searches as the user types (`"search": true`), computes
  root results (`"rootResults": true`) or is
  [scheduled](schedules.md) may run a service as well: they are separate
  activation models, and none runs another's code.
- **The guest's cycle**, the `run-cycle` export of the interface
  `pane:extension/service` ([`wit/service.wit`](../wit/service.wit)):
  Pane calls it with the command's id in `pane.json` (so one component can
  serve several commands' services), and it answers one `cycle` record —
  the status to show and `next-seconds`, how long to wait before the next
  cycle. This is a new guest-facing surface, the smallest that can carry
  the cadence: an ordinary action's answer is a string shown to the
  user, with no room for "when next", and Pane refuses to pace a service
  at an interval of its own choosing, which would make it a schedule. A
  command whose manifest entry sets `"service": true` exports the
  interface beside `command`; a component that does not is refused when
  the package is previewed or installed, before anything is installed.
  JavaScript and TypeScript commands set `"pane": { "service": true }` in
  their `package.json` so the build links it, and export `runCycle` as
  `service` (typed `Service` and `Cycle` in
  [`guests/js/pane.d.ts`](../guests/js/pane.d.ts)).
- **The host runner**, in the core
  ([`launcher/services.rs`](../crates/pane-core/src/launcher/services.rs)):
  a thread of Pane's own, which wakes when a cycle is due and starts each
  on a thread of its own, so neither the window nor the runner waits for a
  guest — the same shape as the scheduler's, and driven by the same
  seams: the launcher's clock, the one clipboard history follows
  ([`crate::clipboard::Clock`](../crates/pane-core/src/clipboard.rs)),
  which tells it when the clock is set other than by time passing, and the
  extension data's change hooks, which tell it whenever a package's
  [generation](generations.md) changes.
- **The author examples** are
  [`guests/sample-service`](../guests/sample-service) (Rust) with the same
  command in [JavaScript](../guests/sample-service-js) and
  [TypeScript](../guests/sample-service-ts), packages
  [`guests/packages/sample-service`](../guests/packages/sample-service)
  and its `-js`/`-ts` copies: the **Watching** command keeps a count of
  events in its content and watches them, and counts its cycles, in its
  content for all time and in its instance for this run. Its items stand
  for the ways a cycle can end ("Add an event" gives the service something
  to see; "Wait on the next cycle" waits ten seconds, so a disable or
  reload while it runs stops it; "Fail the next cycle" answers an error;
  "Crash the next cycle" traps; "Stop responding on the next cycle"
  computes without yielding).

## Behavior

- **At once, not on an interval.** The service begins the moment its code
  may run — Pane starts, the package is installed or enabled, or its code
  is replaced by a reload or an update — and its first cycle runs then,
  because a continuing service's declaration is a request to run, not a
  request to be woken later (a schedule, by contrast, waits a full
  interval first). Installing a package starts its service, since an
  installed package is enabled: the declaration, not the act of
  installing, is what makes the code run, and nothing else of the package
  — and nothing of any other package — is activated by it.
- **The cadence is the service's own.** Each cycle answers `next-seconds`,
  and Pane waits exactly that before the next cycle: a watcher may check
  often, a monitor rarely, and the same service may change its mind cycle
  by cycle. The bounds are 1 second to 30 days, as a schedule's, and
  anything outside them is clamped, not refused (the bounds are
  provisional, like scheduled work's). Time that passes while a cycle runs
  is not replayed: the next cycle runs one cadence after the answer lands.
- **The instance is the service's task.** Each cycle starts the command's
  instance if it has none, and the instance — whatever the service keeps
  in it, its task's or subscription's state — lives for the whole
  generation, finding its state where the last cycle left it. When the
  generation ends the instance goes with it (see
  [generations](generations.md) for what stopping costs), so the task
  Pane stops on disable is exactly the guest's in-memory state; enabling
  the package again starts a fresh one. Data the service saves in its
  settings or content is kept, as for any package.
- **The generation owns the cycle.** Each cycle belongs to the package's
  generation current when the runner asked for it: a disable, reload,
  update, uninstall or pause that happens while it runs stops it (the
  runtime stops the call, and its instance and task go), and its late
  answer is discarded, never shown and never paced from. The sample's
  "Wait on the next cycle" never saves "finished" when it is stopped.
  Waiting is not one of these: a package that
  [waits](dependencies.md#waiting-for-a-required-dependency) for a required
  dependency runs no cycles meanwhile, a cycle already going finishes
  (waiting ends no [generation](generations.md) and stops no instance), and
  the first cycle after what it waited for returns runs at once, in the
  instance the service still has.
- **At most one cycle at a time.** No second cycle of a service is asked
  for before the current one answers; cadences that pass meanwhile are
  served by the next cycle, at the cadence its answer gives from when it
  lands. A cycle that never ends (a guest waiting on a clock) holds the
  runtime like any such call ([pausing](pausing.md)), and its service
  holds only itself.
- **The status, on the command's screen.** When a cycle answers and the
  command's screen is the one on display, its status shows as an action's
  answer does: the text as the result, an error the service answered with
  as an error. While the command is not open the cycle still runs, and
  opening the command shows its effect (the sample's counts, kept in its
  content and its instance). The status never blocks the window: cycles
  run on their own threads, and the runner never waits for a guest.
- **Failures follow the existing policy.** A cycle that traps, or that
  computes without finishing and Pane stops as unresponsive, is a crash of
  the package, like any call: the third within five minutes
  [pauses](pausing.md) the package, which ends the service with the
  generation, and Retry starts it again. An error the service answers
  with, however often, never pauses it: Pane shows it and runs the next
  cycle after a fixed wait (1 second, provisional), so one broken cycle
  never ends a service the user enabled — the same wait a crash gets,
  which is what lets the pause policy count repeated ones.
- **Across restarts.** The service comes from the manifest, not from a
  record, so a restart starts again whatever the installed copy declares
  for packages still enabled, its first cycle at once, its task beginning
  again and its saved counts carrying on. A package disabled before the
  restart is not served after it.
- **Expected errors before anything is installed.** A manifest whose
  command declares `"service": true` but whose component does not export
  `pane:extension/service@0.1.0` with the functions Pane calls is refused
  when the package is previewed or installed, with the reason, and
  nothing is installed or run.

## Limits

- One service per command, declared by a boolean: the minimal shape the
  ticket allows. A service cannot be turned off without disabling the
  package (as a schedule cannot), and the manifest offers no per-command
  service settings.
- The guest-facing surface is new (`run-cycle`), chosen over reusing an
  item's action because the cadence must come from the guest. It is
  deliberately minimal: one function, one record, no event stream, no
  long-lived call. A long-lived call would hold the shared runtime thread
  behind it (the runtime serves one guest call at a time), which is why
  Pane cycles the service instead of letting it run inside one call.
- The cadence bounds (1 second to 30 days), the wait after a failed cycle
  (1 second), and the clamping rather than refusing of out-of-bounds
  answers are provisional, as scheduled work's bounds are.
- A service whose component is momentarily absent — its package's code is
  being replaced — runs when the replacement is in place, rather than
  failing a cycle against the old copy. A component missing for good
  leaves its service unrun and unreported; the command's own load failure
  is what tells the user.
- Waiting inside a cycle (the sample's "Wait on the next cycle") holds
  every other extension's calls behind it, as any call's wait does, until
  its generation ends; a service that must wait for long should answer a
  short cycle and ask Pane to call it again.
- Like every Pane record on the data folder, services are not coordinated
  between two Pane processes on it: both would run a package's service.
- Nothing here is platform-specific (it lives in `pane-core`); it has run
  on Linux, and runs in `cargo xtask ci` on Windows, macOS and Linux.

## Author example and tests

- [`guests/sample-service`](../guests/sample-service) (Rust): the
  **Watching** command, whose package declares `"service": true`. Its
  cycle answers `Watching: <events> events (cycle <total>, <this run>
  this run)` and asks for the next cycle in a second; "this run" is the
  task's state, kept in the instance and so beginning again whenever the
  code starts afresh. "Add an event" is what the service watches; "Wait
  on the next cycle" notes "started" in its settings, waits ten seconds,
  then notes "finished"; "Fail the next cycle" answers an error; "Crash
  the next cycle" counts, then traps; "Stop responding on the next cycle"
  computes without waiting; "Ask for a 0-second cadence" and "Ask for a
  31-day cadence" answer cadences beyond Pane's bounds, which it clamps.
- [`guests/sample-service-js`](../guests/sample-service-js) and
  [`guests/sample-service-ts`](../guests/sample-service-ts): the same
  command in JavaScript and TypeScript, built into `guests/prebuilt` by
  `cargo xtask js-guests` as the other JS/TS samples are.
- [`crates/pane-core/tests/services.rs`](../crates/pane-core/tests/services.rs):
  the same checks in all three languages, with a clock the tests set:
  installing the package starts its service and activates nothing else,
  and the cadence is the service's own (half a cadence runs nothing, five
  at once run one cycle); the status of a cycle shows on the command's
  screen and reports what the service watches; an error a cycle answers
  with shows as one while the service carries on, never pausing it;
  disabling stops a cycle still pending (its "finished" is never saved,
  its answer is not shown over the disable's own outcome, the cadence
  that passes meanwhile runs nothing, and nothing of the package is left
  running), and enabling starts the service again with a fresh task; a
  cycle in flight prevents a second one; a cycle never blocks the
  launcher; a restart starts the service again where the package is
  enabled, its saved count carrying on and its task beginning again, and
  a package disabled before the restart is not served after it;
  reloading stops a pending cycle and the replacement serves again;
  uninstalling stops the service and releases the package's instances; a
  cycle that traps, or that stops responding (the compute limit
  shortened, as #18's unresponsive tests do), is a crash, so three
  within five minutes pause the package and stop the service, and Retry
  restarts it; a cycle answering 0 seconds or 31 days runs again at the
  clamped 1-second minimum and 30-day maximum; and a manifest declaring
  a service the component does not export is refused before anything is
  installed. The runner itself is
  waited for deterministically (`Launcher::wait_for_services`, as
  scheduled work's `wait_for_schedules`): the clock moves only when a
  test moves it, and the test waits until every look and cycle it caused
  has settled. Where a test must see an effect inside the guest — a save
  landing while a cycle is still pending — it polls the package's data
  with a deadline.
