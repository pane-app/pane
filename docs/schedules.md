# Scheduled work

Scheduled work lets a command run on a fixed interval without the user
asking. This page is for extension authors who want to schedule a
command. The design is recorded in
[ADR 0024](adr/0024-the-host-runs-scheduled-extension-work-by-its-own-clock.md)
(proposed).

A command declares the schedule in its package's manifest. Every
interval, Pane runs the command's action of the item the schedule names
and shows the answer on the command's screen. It does this while the
package's code may run: the package is enabled, not [paused](pausing.md)
and not [waiting](dependencies.md#waiting-for-a-required-dependency) for
a required dependency.

- Disabling the package stops the schedule.
- Enabling it starts the schedule again.
- A restart schedules again whatever the manifest declares.

## Where it lives

### The manifest declaration

A `schedule` in the command's entry in `pane.json` names an interval and
the item whose action runs:

```json
"schedule": { "everySeconds": 900, "item": "check" }
```

There is one schedule kind only: a fixed interval. The interval is at
least 1 second and at most 30 days, and the item id is at most 256
characters. These limits are `MIN_SCHEDULE_SECONDS`,
`MAX_SCHEDULE_SECONDS` and `MAX_SCHEDULE_ITEM` in
[`packages.rs`](../crates/pane-core/src/packages.rs). Anything else, or a
schedule without an `item`, is refused when the package is previewed or
installed, before anything is installed.

A command that searches as the user types (`"search": true`) or computes
root results (`"rootResults": true`) may be scheduled as well. The
schedule runs its action, not those interfaces.

### The host scheduler

The scheduler is in the core, in
[`launcher/schedules.rs`](../crates/pane-core/src/launcher/schedules.rs).
It is a thread of Pane's own that wakes when a run is due and starts each
run on a thread of its own, so neither the window nor the scheduler waits
for a guest.

It is driven by the launcher's clock, the same one clipboard history
follows ([`crate::clipboard::Clock`](../crates/pane-core/src/clipboard.rs)).
That is the system's clock, or the one tests and development builds give
through `Launcher::with_clock`; release builds have no way to replace it.
The clock tells the scheduler when it is set other than by time passing.

The scheduler also looks again whenever a package's
[generation](generations.md) changes, since every path that begins or ends
one tells the extension data's change hooks.

### The guest needs nothing new

A scheduled run is the command's action of the item the schedule names,
exactly as running that item from its list is. Pane asks for the command's
tree, then hands the item's action's callback to `handle-event` (see
[list-tree.md](list-tree.md)). The answer text is the result, an error it
answers with is an expected error, and a trap is a crash of the package.

The item is usually one the command lists, so the user can run it too.

### The author examples

The examples are [`guests/sample-schedule`](../guests/sample-schedule)
(Rust), with the same command in
[JavaScript](../guests/sample-schedule-js) and
[TypeScript](../guests/sample-schedule-ts). Their packages are
[`guests/packages/sample-schedule`](../guests/packages/sample-schedule)
and its `-js`/`-ts` copies.
[Author example and tests](#author-example-and-tests) describes what the
**Counting** command does.

## Behavior

### Lazy activation

Nothing of a package runs because it is installed. Its schedule activates
the command only when a run is due, and starts the guest instance then, as
opening the command would. No other package is activated by it.

A command unavailable on this system is never scheduled, like an action
the user cannot invoke.

### When the schedule runs

The schedule runs while the package is enabled, not paused and not
[waiting](dependencies.md#waiting-for-a-required-dependency) for a
required dependency. It starts from the moment the code may run: Pane
starts, the package is installed or enabled, or its code is replaced by a
reload or an update. A code replacement restarts the interval with the new
code.

A disable, an uninstall, a pause after a failure, or a code replacement
ends the schedule. Enabling the package again starts it from a full
interval.

Waiting skips the ticks that fall due, and they are not replayed. When
what the package waits for returns, the schedule starts from a full
interval. A run already going when the package began to wait finishes,
since waiting ends no [generation](generations.md).

### The generation owns the run

Each run belongs to the package's [generation](generations.md) that was
current when the scheduler asked for it. A disable, reload, update,
uninstall or pause during the run stops it: the runtime stops the call,
and its instance goes. The late answer is discarded and never shown. The
sample's "Run slowly" never saves "finished" when it is stopped.

### One run at a time

Ticks that fall due while a run has not answered are coalesced. Pane asks
for no second run of the command meanwhile, and the next run starts at the
next tick after the first one answers.

A run that never ends (a guest waiting on a clock) holds only its own
command's schedule, as any call does ([pausing](pausing.md)).

### The answer shows on the command's screen

When a run answers while the command's screen is on display, and its
generation has not ended, the answer shows the way an action's answer
does. The text shows as the result, and an error the extension answered
with shows as an error.

The run happens whether or not the command is open, and opening the
command shows its effect (the sample's count, kept in its content).

### Crashes

A scheduled run that traps is a crash of the package, as it would be if
the user ran the item by hand. The third crash within five minutes
[pauses](pausing.md) the package, which ends the schedule with the
generation. Retry starts it again.

An error the extension answers with never pauses it, however often it
happens.

### Across restarts

The schedule comes from the manifest, not from a record. A restart
schedules again whatever the installed copy declares, for packages still
enabled. Work that fell due while Pane was not running is not replayed:
the first run after a start is one full interval after it.

### Refused manifests

Pane refuses a manifest whose schedule is impossible, with the reason, and
nothing is installed or scheduled. A schedule is impossible when it has:

- `everySeconds` below 1 or above 2,592,000;
- no `item`;
- an `item` longer than 256 characters;
- an unknown field.

## Limits

- The scheduled run is the action of an item the manifest names. This is
  the minimal choice the ticket allows, provisional pending the user's
  confirmation. A scheduled command cannot be asked for its view instead,
  or run work no item answers, and one schedule per command is all the
  manifest offers.
- The interval bounds (1 second to 30 days) and the restart behavior are
  provisional, as the specification's "scheduling intervals are not yet
  selected". The restart behavior is that work which fell due while Pane
  was stopped, or while the package could not run, is not replayed.
- The clock seam is the one clipboard history introduced
  (`Launcher::with_clock`). Its provisional home is `clipboard.rs`.
  Release builds cannot replace the system's clock, so scheduled work runs
  by the system's time there.
- A guest waiting on a clock (the sample's "Run slowly") is never stopped
  until its generation ends, as for any call. A scheduled command whose
  item waits forever holds the runtime like any such call.
- Only one kind of schedule exists: a fixed interval from when the code
  may run. Calendars, one-shot delays and minimum-battery policies are
  future work, as is a per-command way to turn a schedule off without
  disabling the package.
- Like every Pane record on the data folder, schedules are not coordinated
  between two Pane processes on it: both would run a scheduled command.
- Nothing here is platform-specific (it lives in `pane-core`). It has run
  on Linux, and runs in `cargo xtask ci` on Windows, macOS and Linux.

### Timers a guest sets itself

A guest can still set its own timers: `after` (one firing) and `every`
(one every interval). They are registered at run time as
[owned registrations](generations.md#owned-registrations) (#158), not
declared in the manifest, and have the same bounds (1 second to 30 days).

Each firing is a call into the component's `events` export. It belongs to
the generation like a scheduled run: it is stopped when the generation
ends, its trap counts towards pausing, and none of a waiting package's
code runs. Firings that fall due while one is pending or the package waits
are coalesced into one, delivered when they can be.

Timers are bounded and undone with the code that made them. A package can
have 64; one beyond that is refused with the limit named. ADR 0024 had
rejected timers on the guest's side;
[ADR 0041](adr/0041-extensions-compose-through-capabilities-that-pane-brokers.md)
records why they were added back as owned registrations.

## Author example and tests

### The samples

[`guests/sample-schedule`](../guests/sample-schedule) (Rust) has the
**Counting** command, whose package declares
`"schedule": { "everySeconds": 60, "item": "count" }`. The command counts
its runs in its content and answers the new count. Its items stand for the
ways a run can end:

- "Run now" is the item the schedule runs.
- "Run slowly" saves "started", waits ten seconds, then saves "finished".
  A disable or reload while it runs stops it.
- "Answer an error" answers an error.
- "Crash" counts the run, then traps.

[`guests/sample-schedule-js`](../guests/sample-schedule-js) and
[`guests/sample-schedule-ts`](../guests/sample-schedule-ts) are the same
command in JavaScript and TypeScript. `cargo xtask js-guests` builds them
into `guests/prebuilt`, as it does the other JS/TS samples.

### The checks

[`crates/pane-core/tests/schedules.rs`](../crates/pane-core/tests/schedules.rs)
runs the same checks in all three languages, with a clock the tests set.
It checks that:

- installation alone activates nothing until a run is due, and no other
  package's code runs;
- the answer, and an error the extension answers with, show on the
  command's screen;
- disabling stops a run still pending: its "finished" is never saved, its
  answer is not shown over the disable's own outcome, and ticks that pass
  meanwhile run nothing;
- enabling starts the schedule again from a full interval;
- ticks that fall due while a run is pending start one run, not many, and
  coalesce into one after it answers;
- a restart schedules again whatever the manifest declares, from a full
  interval, without replaying work that fell due while Pane was stopped;
- a package disabled before a restart is not scheduled after it;
- reloading stops a pending run, and the replacement's schedule runs
  again;
- uninstalling stops the schedule;
- a scheduled run that traps counts as a crash, so three within five
  minutes pause the package and stop the schedule, and Retry restarts it;
- a scheduled run that computes without yielding (the compute limit
  shortened, as #18's unresponsive tests do) is stopped by Pane, counted
  as a crash and pauses the package the same way;
- an impossible schedule, including an over-long `item`, is refused
  before anything is installed.

The tests wait for the scheduler deterministically, with
`Launcher::wait_for_schedules` (as clipboard expiry's tests use
`wait_for_clipboard_expiry`). The clock moves only when a test moves it,
and the test waits until every look and run it caused has settled. Where a
test must see an effect inside the guest, such as a save landing while a
run is still pending, it polls the package's data with a deadline. One
check gives a wrongly started second run a moment to show itself.

## Tracking

Added for [#47](https://github.com/pane-app/pane/issues/47): US14, US45,
US57, US58, US79; T02, T09, T17; contributions to G3, not a claim that it
passes.
