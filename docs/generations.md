# Generations: stopping pending calls

Added for [#14](https://github.com/pane-app/pane/issues/14) (US35, US54,
US57, US64, T06, T09, G2, G3; contributions, not a claim that the whole
scenario or gate passes). Disabling, reloading or updating an extension
stops the calls into it that are still pending, and their late results never
reach the screen or the extension's data. This page is the ownership and
cancellation model later lifecycle work builds on: native helpers (#15),
pausing a broken extension (#16), recovering from a guest that stops
responding (#18), uninstall (#40), cancelling a pending search (#29, #30)
and the state handoff across a replacement (#159).

## The model

A **generation** ([glossary](../CONTEXT.md)) is one run of an installed
package's code: it begins when the package is installed, enabled or Pane
starts, and ends when the package is disabled, [paused](pausing.md) after it
failed, or its code is replaced by a reload or an update (a reload's or update's new code runs in a new
generation). A disabled package stays in an ended generation until it is
enabled again. Commands built into Pane have no generation: they run as long
as Pane. A package that
[waits](dependencies.md#waiting-for-a-required-dependency) for a required
dependency that cannot serve it keeps its generation: waiting is not one
of the ways a generation ends. What Pane would start for a waiting package
— its commands' views and runs, scheduled work, services and results — is
not started, but a call or cycle already running finishes, an open screen
stays and its calls answer as calls do.

Every guest call has two owners, with different powers:

| Owner | What it decides | Examples |
| --- | --- | --- |
| The package's **generation**, current when the call was asked for | Whether the call may run at all. When it ends, the call is **stopped**. | Opening a command, running an action, submitting a form, opening a custom view, a view event, computing root results, serving an operation |
| The **screen, view or search** the call answers | Whether its answer is **shown**. Leaving it only **discards** the answer; the call is not stopped. | Back from a command that is opening; a newer query; a newer view event's frame on screen |

- The launcher takes the generation when the user acts (pressing Enter,
  submitting), not when the returned future first runs, so a call asked for
  before a disable is never started for the package's new state.
- An **operation call** belongs to the target package's generation and,
  through the call chain, to every caller's: when any generation in the
  chain ends, the calls from it inward stop. A target stopped while serving
  a call answers its caller at once, with `disabled` or, for a reload or
  update, `unavailable` ("Package b was reloaded or updated while serving
  the call; call it again"); the caller carries on. A caller stopped while
  it waits stops the operation it waits for too.
- Navigation does not stop calls: an action may be doing what the user
  asked, such as saving, and leaving its screen is not a request to stop
  it. The one exception, since [#29](files.md#cancelling-a-pending-search),
  is root search's own calls for computed results: the search is a
  stopping owner of them, so a newer query or leaving root search cancels
  those pending (`CallError::Cancelled`), exactly as an ended generation
  stops them, and it is not a failure for pausing.

## What stopping does

The runtime serves guest calls on its own thread, many at once: while one
waits on something outside its guest, the others run, and each instance's
own calls run one after another (#136). When a generation ends, it runs its
undo list, newest first (what it set up: its instances, its helpers' and
system programs' runs, its web requests and web images, a command search
in progress, its schedules, services and command hotkeys, which the
end marks for the scheduler, the services thread and the next hotkey sync
to drop or carry over to the next generation, and what the package's code
registered at run time — its [owned
registrations](#owned-registrations), dropped with their handles), and:

1. **Queued calls** of it (asked for but not started) are not started; they
   answer "The extension is disabled" or "The extension was reloaded or
   updated while this was running; try again", and the launcher shows
   nothing of them.
2. **A call waiting inside the guest** (on a clock, an operation call, any
   async import) is stopped the next time the runtime thread looks, which
   is immediately, since it is idle while the guest waits: the call's
   future is dropped and so is the guest instance, with everything its
   Wasmtime store holds (the abandoned task, its host tasks, streams,
   futures, custom views and resources). The instance must go: Wasmtime 49
   keeps a task whose host future was dropped in the store and would resume
   it on the next call, and it has no host-side `task.cancel` for an export
   the host called. So nothing after the guest's `await` runs, and a
   package serving an operation for a stopped caller loses its instance too
   (the next call starts it afresh, as after a crash).
3. **A result that completes anyway** (the guest returned in the same turn
   the generation ended) is discarded the same way.
4. **Host imports refuse the stopped code**, so it starts no host work of
   any kind: reading or saving any kind of extension data ("this code of
   the extension was replaced by a reload or an update; its settings are
   kept unchanged", or "the extension is disabled; …"), calling operations
   (`refused`), listing or opening applications, and starting a native
   helper ([helpers](helpers.md)). This matters only for
   code still running because it did not yield (below): code stopped at an
   `await` never runs again.
5. **Other packages keep running**, with their instances and open views.
6. Component checks (install, update, reload) run on a checker thread of
   their own, one at a time, so a reload's check does not wait behind the
   call it is about to stop.

The launcher then shows what the disable, reload or update says ("Disabled
<title>", "Reloaded <title>", "Updated <title> to <version>"); a command,
form or view of the package that was open has closed — and on a reload or
an update Pane opens the command again on the new code, as [the state
handoff](#the-state-handoff) describes — and no answer of the stopped code
appears, on the old screen or on the new code's.

## What stopping costs

Stopping a call drops its whole instance, never just the call: Wasmtime 49
cannot cancel a guest task the host called, and a dropped call's task would
resume in the store. So:

- **The instance's in-memory state is lost** with it: an open view of the
  same instance, caches kept in guest memory, and so on. For disable,
  reload and update that is intended, since the code stops anyway — a
  reload or an update can hand a bounded snapshot of it to the new code
  instead ([the state handoff](#the-state-handoff)). Later
  owners that stop calls while the package keeps running pay this cost
  too: cancelling a search's pending provider calls (#29) drops the
  provider's instance, losing what it keeps in memory between queries (the
  Files extension keeps nothing there).
  A command cancelling its own [native helper](helpers.md) run (#15) needs
  no such owner: the guest drops the run, Wasmtime cancels the host task
  and the process ends, while the instance stays. Such owners may prefer
  discarding answers to
  stopping calls, or waiting for component-model cancellation of a task
  the host called (`task.cancel` from the host) in a later Wasmtime.
- **A package serving an operation for a stopped caller restarts fresh**
  on its next call, although it was not disabled, as after a crash. It is
  not a failure of that package: [pausing](pausing.md) (#16) counts only
  crashes and failures to start, never a stopped call.

## What stopping cannot do yet

- ~~A guest computing without yielding cannot be preempted.~~ Since
  [#18](https://github.com/pane-app/pane/issues/18) every guest yields to
  the runtime thread at each epoch tick (10 ms, `Config::epoch_interruption`
  and an epoch-deadline callback), so a generation that ends stops a
  computing guest within a tick (one starting too), and a call whose guest
  computes for 5 seconds in all (its own computing only) without finishing
  is stopped as unresponsive
  ([pausing](pausing.md#when-an-extension-stops-responding)). What such
  code did before the stop is kept; nothing after it runs. The ticker, and
  the watchdog that gives up on a stuck runtime thread, run only while a
  call is in flight on the thread: while none is, no guest code runs and
  both sleep, and a call wakes them as it is sent, before its guest runs
  ([pausing](pausing.md#when-an-extension-stops-responding), #190). A
  computing guest's call is in flight, so a generation that ends still
  stops it within a tick.
- **No time limit on waiting, and no user cancellation** of an action: a
  call waiting on a clock or another extension's operation ends when the
  guest answers, or when a generation in its chain ends; only computing
  (5 seconds of the guest's own) is limited. A native helper's run has had
  no limit of Pane's own since #136.
## Owned registrations

An extension can register things at run time, each an **owned
registration** ([glossary](../CONTEXT.md)) the guest holds as a Wasm
resource: a dynamic root item (a row of root search under one of its
commands), a timer, a folder watcher, or a run-time provision of a
capability its `pane.json` marks `atRunTime`. Each lasts until the first
of three: the guest dropping the handle, the instance holding it going
away, or the generation ending — every one of the ways above — so
nothing an extension registered outlives its code, and an author writes
no cleanup code. Each records its undo on the generation's list; ending
it another way takes the entry off. A handle whose generation ended is
refused, as host imports already refuse stopped code.

Timer firings and watcher changes are delivered as guest calls, so they
belong to the generation and follow every rule on this page: a call in
flight is stopped by an ended generation, a trap counts towards
[pausing](pausing.md), and a package that
[waits](dependencies.md#waiting-for-a-required-dependency) runs none of
its code, its timers' and watchers' events held as one, delivered when it
can run again.

A package whose `pane.json` declares `"activate": "<component>"` has that
entry point called when its code may run and it is not waiting — at
install, enable, start, reload, update, Retry and on coming back from
waiting — and again when the instance that ran it is dropped while the
generation continues unpaused, so its registrations are made without
waiting for the user (an opt-in exception to lazy activation, ADR 0005
as amended by ADR 0041). A trap in it is a crash; during a reload's
start, a startup failure. Without `activate`, a package's code first runs
when the user asks for one of its commands, and its registrations live
only as long as the instance that made them.

- **External side effects are not undone**: what the guest did before the
  stop (a file written through WASI, a request sent) stays done; only what
  it would have done afterwards is prevented. Data it saved before the stop
  is kept (the sample's "started").
- **Native helper processes** ([helpers](helpers.md), #15) belong to their
  package's generation and are ended with it, by a thread that watches the
  generation itself, so even while the runtime thread is busy; they also
  end with the call that started them. Processes a helper starts itself are
  not stopped. Nothing in this model assumes one operating system: it lives
  in the runtime and the launcher, with no platform adapter.
- **Background work** the host runs without the user asking —
  [scheduled work](schedules.md) since #47 and a [continuing
  service](services.md) since #48 — belongs to a generation the same way:
  a disable, pause, uninstall or code replacement stops a run still
  pending and its late answer is discarded. Other kinds of background
  work (timers, subscriptions a guest keeps by itself) are not part of
  the extension API yet; when they are, they belong the same way.
- Measured cleanup is what the runtime reports (`Runtime::running`,
  `Runtime::view_count`); memory returned to the operating system after a
  dropped store is not measured.

## The state handoff

A package's replacement — a Reload, an Update (made by the user or by
Pane itself) or a development-mode reload — can hand what the old code
kept in memory to the new code: the **state handoff** (ADR 0004's
"explicit extension support", now defined). A component opts in by
exporting `snapshot` and `restore` in the lifecycle interface (`wit/
registrations.wit`; the component a `pane.json` names under `activate`
exports the same interface), which installing detects without running
guest code. A JavaScript or TypeScript package sets `"snapshot": true`
in the `"pane"` options of its `package.json` so that the build links
them, as for services.

- **When.** Only once the replacement has passed its checks and is about
to be installed: a replacement or build that fails changes nothing and
takes no snapshot. Never after a crash, a pause, a failure to start,
Retry, a disable followed by an enable, a runtime crash or hang, or a
restart of Pane.
- **Taking the snapshot.** Each running instance of the package that
exports `snapshot` and is idle (no call pending in it) is asked for one
before the old generation ends; an instance busy with a call is stopped
as today and gives none. The answer must come within 1 second, so a
handoff never delays a replacement by more than that, and is limited to
1 MiB, the limit on operation JSON; a late or larger one is dropped and
the replacement goes ahead. Snapshots are kept in memory only, never
written to disk.
- **Restoring.** The new code's instance of the same component file
restores the snapshot on its first start, before any other call into it,
so a continuing service's first cycle finds the restored state too. An
error from `restore` discards the state and is not a failure: the
extension starts fresh. A trap in `restore` is a crash; during a
reload's start it is a startup failure, so the package is paused and
the state is lost. A snapshot whose component the new code no longer has
is discarded. Development mode's diagnostics report a dropped,
oversized, late or rejected snapshot; elsewhere it is silent.
- **The screen reopens.** For every package, opted in or not: when one of
the package's command screens was on display at the replacement, Pane
opens that command again on the new code with its original launch
record — only its root view, not the views or forms it had pushed, which
an author restores from the snapshot if they want them back. The setup
gate applies if the new code adds required preferences. An automatic
update never applies while a screen is on display, so this concerns
Reload, development-mode reload and an update the user made.

The handoff sample, in [Rust](../guests/sample-handoff/src/lib.rs),
[JavaScript](../guests/sample-handoff-js/src/index.js) and
[TypeScript](../guests/sample-handoff-ts/src/index.ts), keeps a counter
and a draft in memory; the SDKs' helpers (`pane_extension::state`,
`@pane-app/extension/state`) serialise a value — serde in Rust, JSON in
JavaScript and TypeScript — while the bytes stay the author's to
version.

## Examples and tests

- The operations samples ([Rust](../guests/sample-operations/src/lib.rs),
  [JavaScript](../guests/sample-operations-js/src/index.js),
  [TypeScript](../guests/sample-operations-ts/src/index.ts)) publish
  `wait`, which saves `waiting` as "started", waits ten seconds and saves
  "finished"; their "Wait in another extension" item calls it in another
  package.
- The settings samples' **Save after waiting**
  ([Rust](../guests/sample-settings/src/lib.rs),
  [JavaScript](../guests/sample-settings-js/src/index.js),
  [TypeScript](../guests/sample-settings-ts/src/index.ts)) save "started",
  wait ten seconds with `wasi:clocks/monotonic-clock.wait-for`, then save
  "finished". Disabling or reloading the package meanwhile ends the call at
  once and "finished" is never saved.
- [`crates/pane-core/tests/stopping.rs`](../crates/pane-core/tests/stopping.rs)
  drives them in all three languages through the launcher: disabling,
  reloading and updating while the call waits (it ends in well under the
  wait, its answer is not shown, "finished" is not saved, and the new code
  runs), three disable and reload cycles each leaving only the expected
  instances running and no views, and a call queued behind a stopped one
  that belonged to the replaced code.
- [`crates/pane-core/tests/operations.rs`](../crates/pane-core/tests/operations.rs)
  stops a chain from both ends with the operations fixture's `wait`:
  disabling or reloading the target while it serves (the caller is told at
  once), and disabling the caller (the target's instance goes, the target
  itself stays enabled and serves the next call); and with the samples'
  `wait` from Rust to JavaScript, JavaScript to TypeScript and TypeScript
  to Rust, from both ends. The fixture's spinning items compute without
  yielding while the test disables or reloads `a`: what they try
  afterwards (saving, calling `b`) is refused, and their answer, or their
  error, completing in the same turn is discarded.
- [`crates/pane-core/tests/waiting.rs`](../crates/pane-core/tests/waiting.rs)
  drives a call that is already running when the package that made it
  begins to [wait](dependencies.md#waiting-for-a-required-dependency): it
  finishes with its answer, since waiting ends no generation and stops no
  instance.
- [`crates/pane-core/tests/handoff.rs`](../crates/pane-core/tests/handoff.rs)
  drives the state handoff in all three languages through the launcher:
  the counter and draft kept across a reload and a user's update, the
  screen reopened with its launch record for a package that does not opt
  in (a pushed view not reopened), nothing handed over after a crash, a
  disable followed by an enable or a restart, and the fixture's edges:
  a busy instance, the deadline, the size limit, and a `restore` that
  errs or traps.
- Runtime tests (`runtime.rs`): stopping a call whose instance holds a
  stream open to the host (stdout), the pending future of that write and
  an open custom view releases them all (the faulty fixture's `hold`); and
  a stale call of an ended generation, served after the next generation
  opened a view at the same component, is refused without touching it.
- These run in `cargo xtask ci`, which CI runs on Windows, macOS and Linux;
  when this was written they had run on Linux only. The earlier 20 sequential calls in one QuickJS
  instance are not concurrency evidence; the overlapping calls here are
  queued calls behind a pending one, and a chain's calls, still served one
  at a time.
