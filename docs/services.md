# Continuing services

A continuing service is extension code that Pane keeps running in the
background, without the user asking, for as long as the package's code
may run. This page is for extension authors who want to write one. The
design is recorded in
[ADR 0025](adr/0025-a-continuing-service-cycles-at-its-own-cadence.md)
(proposed).

A command declares the service in its package's manifest. Pane then runs
the service in cycles while the package is enabled, not
[paused](pausing.md) and not
[waiting](dependencies.md#waiting-for-a-required-dependency) for a
required dependency. The manifest declares no interval. Each cycle answers
with the status to show and how long to wait before the next one.

For the user, a service works like this:

- Installing or enabling the package starts the service.
- The service's status shows on the command's screen.
- Disabling the package in extension management stops the service, and
  enabling it starts the service again.
- A restart starts the service again where the package is enabled.

## Where it lives

### The manifest declaration

A command declares a service in its entry in `pane.json` with
`"service": true`. Each command can declare one service, the minimal
shape. A package whose commands each declare one runs several services,
just as several commands may each declare a [schedule](schedules.md).

A command that runs a service may also search as the user types
(`"search": true`), compute root results (`"rootResults": true`) or be
[scheduled](schedules.md). These are separate activation models, and none
runs another's code.

### The guest's cycle

The guest side is the `run-cycle` export of the interface
`pane:extension/service` ([`wit/service.wit`](../wit/service.wit)). Pane
calls it with the command's id in `pane.json`, so one component can serve
several commands' services. It answers one `cycle` record: the status to
show, and `next-seconds`, how long to wait before the next cycle.

This is a new guest-facing surface, the smallest that can carry the
cadence. An ordinary action answers with a string shown to the user and
has no room for "when next". Pane will not pace a service at an interval
of its own choosing either, since that would make it a schedule.

A command whose manifest entry sets `"service": true` exports the
interface beside `command`. A component that does not is refused when the
package is previewed or installed, before anything is installed.

JavaScript and TypeScript commands set `"pane": { "service": true }` in
their `package.json` so the build links the interface, and export
`runCycle` as `service`. The types `Service` and `Cycle` are in
[`guests/js/pane.d.ts`](../guests/js/pane.d.ts).

### The host runner

The runner is in the core, in
[`launcher/services.rs`](../crates/pane-core/src/launcher/services.rs).
It is a thread of Pane's own that wakes when a cycle is due and starts
each cycle on a thread of its own, so neither the window nor the runner
waits for a guest. The scheduler has the same shape and is driven by the
same two seams:

- The launcher's clock, the one clipboard history follows
  ([`crate::clipboard::Clock`](../crates/pane-core/src/clipboard.rs)). It
  tells the runner when the clock is set other than by time passing.
- The extension data's change hooks. They tell the runner whenever a
  package's [generation](generations.md) changes.

### The author examples

The examples are [`guests/sample-service`](../guests/sample-service)
(Rust), with the same command in [JavaScript](../guests/sample-service-js)
and [TypeScript](../guests/sample-service-ts). Their packages are
[`guests/packages/sample-service`](../guests/packages/sample-service) and
its `-js`/`-ts` copies. [Author example and tests](#author-example-and-tests)
describes what the **Watching** command does.

## Behavior

### It starts at once

The service begins the moment its code may run: when Pane starts, when the
package is installed or enabled, or when its code is replaced by a reload
or an update. Its first cycle runs then. The declaration is a request to
run now, where a schedule waits a full interval first.

Installing a package starts its service, since an installed package is
enabled. What makes the code run is the declaration. Installing activates
nothing else of the package, and nothing of any other package.

### The service sets its own cadence

Each cycle answers `next-seconds`, and Pane waits exactly that long before
the next cycle. A watcher may check often and a monitor rarely, and the
same service may change its mind from one cycle to the next.

The bounds are 1 second to 30 days, the same as a schedule's. Anything
outside them is clamped, not refused. Like scheduled work's bounds, these
are provisional.

Time that passes while a cycle runs is not replayed. The next cycle runs
one cadence after the answer lands.

### The instance holds the service's task

Each cycle starts the command's instance if it has none. The instance
holds whatever the service keeps in it, such as its task's or
subscription's state. It lives for the whole generation, so each cycle
finds the state where the last one left it.

When the generation ends, the instance goes with it ([generations](generations.md)
describes what stopping costs). The task Pane stops on disable is exactly
the guest's in-memory state, and enabling the package again starts a fresh
one.

A reload or an update that replaces the code can hand the task over
instead, when the component opts in to the
[state handoff](generations.md#the-state-handoff) (ADR 0041, #159). Pane
asks the old instance for a snapshot while it is idle, and the new
instance's first cycle finds the restored state. The service sample's
"this run" count carries on where it left off.

Data the service saves in its settings or content is kept, as for any
package.

### The generation owns each cycle

Each cycle belongs to the package's generation that was current when the
runner asked for it. A disable, reload, update, uninstall or pause during
the cycle stops it: the runtime stops the call, and the instance and task
go. The late answer is discarded, never shown and never paced from. The
sample's "Wait on the next cycle" never saves "finished" when it is
stopped.

Waiting for a dependency does not stop a cycle. A package that
[waits](dependencies.md#waiting-for-a-required-dependency) for a required
dependency runs no cycles meanwhile, but a cycle already going finishes,
because waiting ends no [generation](generations.md) and stops no
instance. When what it waited for returns, the first cycle runs at once,
in the instance the service still has.

### One cycle at a time

Pane never asks for a second cycle of a service before the current one
answers. Cadences that pass meanwhile are served by the next cycle, at the
cadence its answer gives, counted from when it lands.

A cycle that never ends (a guest waiting on a clock) holds the runtime
like any such call ([pausing](pausing.md)), and its service holds only
itself.

### The status shows on the command's screen

When a cycle answers while the command's screen is on display, its status
shows the way an action's answer does. Text shows as the result, and an
error the service answered with shows as an error.

The cycle runs whether or not the command is open, and opening the command
shows its effect (the sample's counts, kept in its content and its
instance). The status never blocks the window: cycles run on their own
threads, and the runner never waits for a guest.

### Failures

Failures follow the existing policy. A cycle that traps, or that computes
without finishing so that Pane stops it as unresponsive, is a crash of the
package, like any call. The third crash within five minutes
[pauses](pausing.md) the package, which ends the service with the
generation. Retry starts it again.

An error the service answers with never pauses it, however often it
happens. Pane shows the error and runs the next cycle after a fixed wait
of 1 second (provisional), so one broken cycle never ends a service the
user enabled. A crash gets the same wait, which is what lets the pause
policy count repeated ones.

### Across restarts

The service comes from the manifest, not from a record. A restart starts
it again for every package still enabled, from whatever the installed copy
declares. The first cycle runs at once, the task begins again, and saved
counts carry on. A package disabled before the restart is not served after
it.

### Refused manifests

A manifest is refused when a command declares `"service": true` but its
component does not export `pane:extension/service@0.2.0` with the
functions Pane calls. The refusal comes when the package is previewed or
installed, with the reason, and nothing is installed or run.

## Limits

- Each command has one service, declared by a boolean: the minimal shape
  the ticket allows. A service cannot be turned off without disabling the
  package (a schedule cannot either), and the manifest offers no
  per-command service settings.
- The guest-facing surface (`run-cycle`) is new, because the cadence must
  come from the guest and an item's action cannot carry it. It is
  deliberately minimal: one function, one record, no event stream and no
  long-lived call. The runtime serves one guest call at a time on a shared
  thread, so a long-lived call would hold that thread. Pane cycles the
  service instead. ADR 0025 records the reasoning.
- The cadence bounds (1 second to 30 days), the 1-second wait after a
  failed cycle, and clamping out-of-bounds answers instead of refusing them
  are provisional, as scheduled work's bounds are.
- While a package's code is being replaced, its component is momentarily
  absent. The service runs once the replacement is in place, without
  failing a cycle against the old copy. A component missing for good
  leaves its service unrun and unreported; the command's own load failure
  is what tells the user.
- Waiting inside a cycle (the sample's "Wait on the next cycle") holds
  every other extension's calls behind it until its generation ends, as
  any call's wait does. A service that must wait for long should answer a
  short cycle and ask Pane to call it again.
- Like every Pane record on the data folder, services are not coordinated
  between two Pane processes on it: both would run a package's service.
- Nothing here is platform-specific (it lives in `pane-core`). It has run
  on Linux, and runs in `cargo xtask ci` on Windows, macOS and Linux.

## Author example and tests

### The samples

[`guests/sample-service`](../guests/sample-service) (Rust) has the
**Watching** command, whose package declares `"service": true`. The
command keeps a count of events in its content and watches them. It also
counts its cycles: in its content for all time, and in its instance for
this run.

Each cycle answers `Watching: <events> events (cycle <total>, <this run> this run)`
and asks for the next cycle in a second. "This run" is the task's state.
It is kept in the instance, so it begins again whenever the code starts
afresh. A reload or an update is the exception: the sample opts in to
handing its state over (its component exports the lifecycle interface, as
do the JavaScript and TypeScript service samples).

Its items stand for the ways a cycle can end:

- "Add an event" is what the service watches. It gives the service
  something to see.
- "Wait on the next cycle" notes "started" in its settings, waits ten
  seconds, then notes "finished". A disable or reload while it waits stops
  it.
- "Fail the next cycle" answers an error.
- "Crash the next cycle" counts, then traps.
- "Stop responding on the next cycle" computes without yielding.
- "Ask for a 0-second cadence" and "Ask for a 31-day cadence" answer
  cadences beyond Pane's bounds, which Pane clamps.

[`guests/sample-service-js`](../guests/sample-service-js) and
[`guests/sample-service-ts`](../guests/sample-service-ts) are the same
command in JavaScript and TypeScript. `cargo xtask js-guests` builds them
into `guests/prebuilt`, as it does the other JS/TS samples.

### The checks

[`crates/pane-core/tests/services.rs`](../crates/pane-core/tests/services.rs)
runs the same checks in all three languages, with a clock the tests set.
It checks that:

- installing the package starts its service and activates nothing else;
- the cadence is the service's own: half a cadence runs nothing, and five
  at once run one cycle;
- the status of a cycle shows on the command's screen and reports what
  the service watches;
- an error a cycle answers with shows as one while the service carries on,
  never pausing it;
- disabling stops a cycle still pending: its "finished" is never saved,
  its answer is not shown over the disable's own outcome, the cadence that
  passes meanwhile runs nothing, and nothing of the package is left
  running;
- enabling starts the service again with a fresh task;
- a cycle in flight prevents a second one;
- a cycle never blocks the launcher;
- a restart starts the service again where the package is enabled, its
  saved count carrying on and its task beginning again, and a package
  disabled before the restart is not served after it;
- reloading stops a pending cycle, and the replacement serves again;
- uninstalling stops the service and releases the package's instances;
- a cycle that traps, or that stops responding (the compute limit
  shortened, as #18's unresponsive tests do), is a crash, so three within
  five minutes pause the package and stop the service, and Retry restarts
  it;
- a cycle answering 0 seconds or 31 days runs again at the clamped
  1-second minimum and 30-day maximum;
- a manifest declaring a service the component does not export is refused
  before anything is installed.

The tests wait for the runner deterministically, with
`Launcher::wait_for_services` (as scheduled work's tests use
`wait_for_schedules`). The clock moves only when a test moves it, and the
test waits until every look and cycle it caused has settled. Where a test
must see an effect inside the guest, such as a save landing while a cycle
is still pending, it polls the package's data with a deadline.

## Tracking

Added for [#48](https://github.com/pane-app/pane/issues/48): US14, US45,
US57, US79; T02, T09, T17; contributions to G3, not a claim that it
passes.
