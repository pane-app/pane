# Pausing a broken extension

Added for [#16](https://github.com/pane-app/pane/issues/16) (US77, US78,
US79, US80, T17, G3; contributions, not a claim that the whole scenario or
gate passes). When one installed extension keeps failing, Pane pauses it,
says so, and offers Retry, while the rest of Pane keeps running. No command
line is needed. The model builds on [generations](generations.md): pausing
ends the package's generation.

## What pauses a package

Only a failure Pane can attribute to one package: its code, in the
package's current generation, failed on its own.

| Failure | Paused | Why |
| --- | --- | --- |
| A component could not be loaded or instantiated, or a reload's new code trapped as it started (a [startup failure](../CONTEXT.md)) | At once | Starting it again would fail the same way. |
| A guest call trapped (a crash): opening a command, an action, a form, a custom view's event, drawing or destructor, root results, indexed results, a query sent to a command through its alias or as a fallback, an operation another package called, a scheduled run or a continuing service's cycle | On the **3rd crash within 5 minutes** | A broken command stops failing soon; one bad input does not stop an extension that otherwise works. |
| A guest call computed for 5 seconds without finishing and Pane stopped it (it **stopped responding**, [below](#when-an-extension-stops-responding), #18) | Counted as a crash, in the same count: the 3rd failure within 5 minutes pauses | Wasmtime was running that package's code, so the failure is known to be its own (provisional). |

These are explicit choices, not measurements (`CRASHES_BEFORE_PAUSE` and
`CRASH_WINDOW` in
[`launcher/pausing.rs`](../crates/pane-core/src/launcher/pausing.rs)):

- Calls that answer between crashes **do not** start the count again: a
  user opens the command (which answers) before each crashing action, so a
  reset on any answer would never pause it. Crashes more than 5 minutes
  apart are not counted together, so rare crashes of a long-running Pane
  never add up to a pause.
- The count is kept in memory for the package's current generation: a
  restart, disable, enable, reload, update or Retry starts it afresh. Only
  the pause itself is recorded. The window is tested with explicit times
  (`Pauses::crashed` takes the time of each crash).

What is **not** a failure of the package:

- An **error the extension answers with** ("sign in first", a refused form,
  a refused view): an expected outcome of code that runs. It never pauses
  it, however often it happens.
- A **call stopped** because its generation, or that of a caller in its
  chain, ended (disable, reload, update, uninstall). A package serving an
  operation for a stopped caller loses its instance and restarts afresh on
  its next call, as after a crash ([generations](generations.md#what-stopping-costs));
  that is not counted.
- A **crash of an operation's target** counts for the target only; its
  caller answered.
- A crash reported after its package was disabled, reloaded, updated or
  uninstalled: the package's generation is checked with the launcher's
  state locked, the same lock those end it under, so such a report never
  counts.
- In JavaScript and TypeScript, **anything a handler throws** is an error
  it answers with: the build wraps the exported handlers
  ([`guests/js/adapt.js`](../guests/js/adapt.js)), so a thrown `Error`
  from `submitForm` rejects the form as a whole rather than trapping.
- A failure of the **runtime itself**, with no attributable package, pauses
  nothing: a reload or Retry that cannot start the package because Pane's
  runtime is unavailable (it could not start, or its thread has stopped)
  says so, and a Retry leaves the pause as it was. Recovering from a crash
  of the runtime thread is described [below](#when-the-extension-runtime-itself-crashes)
  (#17), and so is a runtime thread that stops responding
  ([#18](https://github.com/pane-app/pane/issues/18)).
- A **slow call**: a guest waiting (on a clock, a helper, another
  extension) is not computing and is never stopped for it; a native helper
  that runs past its 30-second limit is ended, and the command gets an
  error it handles ([helpers](helpers.md#running-and-stopping)).

## What a paused package does

- Its generation ends (`End::Paused`): its pending calls stop, its
  instances and views are dropped, and its code can no longer read or save
  data or call operations. A command of it that is open closes.
- Its commands stay in root search, listed and selectable, saying "<title>
  is paused after an error; retry it in Settings › Extensions"; activating one
  shows that and runs nothing. A global hotkey assigned to one stays
  registered (it is the user's choice); pressing it shows the same reason.
  It computes no root results and supplies no indexed ones. Its commands'
  alias and fallback rows ([aliases](aliases.md)) stay listed with the same
  reason and send nothing. Another
  package calling its operations is answered `unavailable` with the same
  reason. Root search tells a paused command from one this system does not
  support, and from one whose package
  [waits](dependencies.md#waiting-for-a-required-dependency) for a required
  dependency, by its reason's kind (`Unavailable::Paused`,
  `Unavailable::OnThisSystem` and `Unavailable::Waiting`).
- The status line (the launcher's toast) says "<title> crashed 3 times
  within 5 minutes and is paused" ("stopped responding 3 times", or
  "crashed or stopped responding 3 times", when the count holds stopped
  calls; or "could not start and is paused"),
  that its saved data is kept, and that Retry and why are in Manage
  extensions.
- **Settings › Extensions** lists it as "Enabled · Paused after crashing"
  ("Paused after not responding", "Paused after crashing or not
  responding", or "Enabled · Failed to start"), with a **Retry <title>** row (**Retry
  starting <title>** after a failure to start) and a **Why <title> is
  paused** row. That opens the details: how it failed, its source and
  version, what is kept, and the full diagnostics (the last crash's message
  and backtrace), with Retry. The diagnostics of a reload that fails to
  start also go to standard error and [Pane's log](#when-pane-itself-ends-its-log-and-the-crash-notice).
- Its settings, content, cache and credentials are kept. Clearing its cache,
  uninstalling it and the rest of Settings › Extensions work, since none of them
  runs it.
- Other packages keep running. A package that requires the paused one
  [waits](dependencies.md#waiting-for-a-required-dependency) for it, coming
  back on Retry, rather than having its calls refused; waiting never
  counts towards pausing it.

Paused is **not** disabled: disabled is the user's choice and adds nothing
to root search; paused is Pane's, and says why.

## Persistence and how a pause ends

The pause is recorded in `installed.json` beside the package's record, with
the cause, the details, the package's version and its managed copy
(`"paused": { "after": "crashes", "why": …, "version": …, "code": "3" }`).
After a restart the package is still paused if its managed copy and version
are still those that failed: its commands are explained and nothing of it
runs, so a known broken package is not started again blindly.

The record is written by a thread of its own, in the order the pauses and
their ends happen, so neither the runtime thread (where a crash is noticed)
nor the window waits for the file; `Launcher::records_written` resolves
once what happened so far is written. A Pane stopped in the moment between a
pause and its record forgets that pause.

| Action | Effect |
| --- | --- |
| **Retry** | Starts the same code in a new generation, asking each available command for its view (as a reload does; see [current decisions](current-decisions.md) on this exception to lazy activation). If that fails to start, it is paused again; if Pane's runtime is unavailable, it stays paused as it was. The crash count starts afresh. |
| Reload or update | New code, which has not failed: the pause and its record go. |
| Disable or enable | Ends the pause too (as a disable ended a startup failure before): the package starts afresh. |
| Uninstall | The record goes with the package. An uninstall that cannot be recorded leaves it installed and still paused. |

A reload whose new code fails to start ([#11](https://github.com/pane-app/pane/issues/11))
is now one such pause: the Retry and diagnostics it offered are these, and
it holds across a restart. The earlier code is still not restored.

## When the extension runtime itself crashes

Added for [#17](https://github.com/pane-app/pane/issues/17) (US77, US78,
US80, T18, T19, G3; contributions, not a claim that the whole scenario or
gate passes). The [extension runtime](../CONTEXT.md) runs every
extension; it is a thread in Pane's process today (the ticket speaks of
killing the runtime process; whether it becomes one is pending the user's
decision, see [current decisions](current-decisions.md) Q39). A **runtime crash** is a panic of
that thread: a fault in Pane's host code or in Wasmtime, not a guest trap
(a trap is caught and counts as a crash of its package, above). Pane cannot
tell which extension, if any, caused it, so:

- **No extension is named or paused**, and no crash is counted towards
  pausing one. The status line says "Pane's extension runtime stopped
  unexpectedly and was started again; what was running was stopped and is
  not run again. Saved data is kept; details are in Settings › Extensions."
- **Every call the thread held stops**, running or queued, and answers
  "Extension runtime unavailable: it stopped before answering and was
  started again (or was not restarted, and why); Pane does not run this
  again by itself". None is sent again: an action that saved before its
  answer was lost stays done once, and running it again is the user's
  choice. Its guest instances, custom views and streams go with the
  thread; a custom view on screen closes, returning to its command; a
  command's list and a form stay open (their next call starts a fresh
  instance).
- **The native helpers it ran are ended** and reaped before anything
  else happens, since all of them were started by its guests
  ([helpers](helpers.md)); development builds run outside the runtime and
  are not affected.
- **Saved data is kept**: Pane writes extension data itself, never through
  the runtime thread's state.
- **Navigation and management keep working**: root search, Manage
  extensions, enabling and disabling, clearing a cache, uninstalling,
  deleting retained data and the hotkey and alias screens run no
  extension. Installing, updating and reloading check components on a
  checker thread of their own, which answers a check that panics and
  carries on.
- **Restarting is suppressed after a repeat.** Pane starts a fresh runtime
  thread (with a fresh engine) after a crash, unless the runtime crashed
  within 5 minutes before (`CRASH_WINDOW` in
  [`runtime/supervisor.rs`](../crates/pane-core/src/runtime/supervisor.rs),
  the same window as for pausing a package):
  then it stays stopped, and every extension call answers "it stopped after
  crashing and runs nothing until you restart it in Settings › Extensions".
- **Settings › Extensions** then starts with **Restart the extension runtime**
  (when Pane did not restart it) and **Why the extension runtime stopped**,
  whose screen says what happened and what Pane did, and shows the panic
  message ("Diagnostics"; the backtrace, if enabled, goes to standard error
  and Pane's log with the rest of the report). Restarting forgets earlier crashes, so the
  next one restarts it again by itself. The window redraws by itself when
  the crash is reported.

Faults are injected to check this, since no extension can crash the
runtime thread: `Runtime::inject(Fault::Crash)` panics the thread wherever
it waits (for the next request, or for a guest's clock, helper or
operation), `Fault::CrashBeforeAnswer { item }` panics it once the action
`item` has run, before its answer is sent (a lost response after a
completed side effect; other calls, such as root search's, are not
affected). The native smokes set `PANE_TEST_RUNTIME_FAULTS` to a file whose
appearance injects one (`crash`, or `crash-before-answer:count`). All of
this exists in debug builds only (`cfg(any(test, debug_assertions))`, which
the tests and smokes use): a release build has no fault types, hook or
environment variable.

Recovery relies on the panic **unwinding** to a `catch_unwind` on the
runtime thread: Pane refuses to build with `panic = "abort"`
(`compile_error!` in `runtime/supervisor.rs`).

**Taking over a lock.** The runtime's own locks ignore poisoning (one
`lock` helper in `runtime.rs`); what they guard is replaced as a whole, so
it is never half written. The launcher's state is different: the runtime
thread holds it while it notes a package's failure, and could panic half
way through pausing it. When the launcher finds its state poisoned, it
clears the poison and first resets it: every claim of a change in progress
is dropped (the panicked thread's would never be released; a change still
running elsewhere finishes, but another change to the same package is no
longer refused meanwhile), the listed pauses are made to agree with the
packages whose code is stopped (one stopped but not yet listed is listed as
paused, with Retry), and root search is shown afresh, closing any command,
form or custom view, with "Pane recovered from an internal error". The
installed packages, their records and extension data are not rebuilt:
they change only after their change is written.

Limits: a failure that ends Pane's whole process is not recovered: an
abort, a fault in native code, the system killing Pane, or **a panic in a
destructor while the thread unwinds from the first panic** (Rust then
aborts the process). The runtime is a thread in Pane's process today, not
a process of its own. The crash history is in memory only. A crash that loses the answer of an operation call loses the
caller's answer too; neither is sent again. Since #18 every guest yields
at each epoch tick, so an injected crash no longer waits for a computing
guest.

## When an extension stops responding

Added for [#18](https://github.com/pane-app/pane/issues/18) (US77, US78,
US80, T18, T19, G3; contributions, not a claim that the whole scenario or
gate passes). The runtime serves one guest call at a time, so a call that
never finishes holds every other extension's calls behind it. Pane bounds
the ways a call fails to finish that it can tell apart (a guest computing,
a native helper that does not exit, the runtime thread itself stuck) and
**never blames a healthy extension**: only a guest's own computing counts
against it ([`runtime/deadlines.rs`](../crates/pane-core/src/runtime/deadlines.rs);
every value is an explicit, **provisional** choice). Some ways are not
bounded yet: a guest waiting on a clock, one looping on Pane's host calls,
and a host call that never returns (see the limits below):

| What does not finish | Limit | What Pane does | Whose failure |
| --- | --- | --- | --- |
| A guest **computing without waiting** (a busy loop in Rust, JavaScript or TypeScript): an **unresponsive call** | 5 seconds of the guest's own computing in one call, in all (`COMPUTE_LIMIT`) | Stops the call where the guest yields and drops its instance, as for a stopped call; the answer is "The extension stopped responding: it computed for 5 seconds without finishing, so Pane stopped it" | The package's own: Wasmtime was running its code. Counted with its crashes (3 within 5 minutes pause it) |
| A guest **waiting on a native helper** that does not exit | None since #136 (#18's provisional 30 seconds is gone, see [helpers](helpers.md#limits)) | Nothing: other extensions' calls are served meanwhile; the command's own timeout, the end of its call or of its generation, or Pane quitting ends the helper | — |
| The **runtime thread itself** making no progress (a **runtime hang**): inside one poll of its work, outside any host call, with its heartbeat still | Says "not responding yet" after 10 seconds (`WARN_AFTER`), gives up after 30 seconds (`UNRESPONSIVE_LIMIT`) | Gives up on the thread, as on a [runtime crash](#when-the-extension-runtime-itself-crashes) | Unknown: no extension is named or paused |
| A guest **waiting** on anything else (a clock, a save, an operation of another extension, a web request) | None | Nothing: waiting is not computing, and the call ends when its generation does | — |
| A **slow host call** (Pane reading or saving a value, listing applications, answering a folder's listing, reading or changing clipboard history) | None | Nothing: its time is Pane's, not the guest's, and the thread inside it is not stuck | — |

How it works:

- **Epochs.** Each runtime thread has a ticker thread advancing the
  engine's epoch every 10 ms while a call is in flight on it (see the next
  point; `Config::epoch_interruption`), and every store yields to the
  runtime thread at each one (an epoch-deadline
  callback). So the runtime thread keeps looking at the call's generation
  and injected faults however busy a guest is: disabling, reloading,
  updating or pausing a package stops its computing guest within a tick
  ([generations](generations.md)). Epoch interruption was chosen over fuel
  because it measures time, not instructions, and costs a check per loop
  and function rather than a count per instruction. The ticker ends with
  its thread.
- **The timers sleep while no call runs** (#190). Every request sent to a
  runtime thread is **in flight** there from when it is sent until it has
  been served: any guest call (a command, root and indexed results, a
  search, an action, a scheduled run, a service's cycle, setup and
  preferences; an operation is served inside its caller's call), a custom
  view's destructor, and host work that waits on guests
  (`Runtime::running`, `view_count`). Guest code runs only meanwhile.
  While nothing is in flight, the ticker waits without a timeout and does
  not tick; the watchdog waits too while the thread is also outside any
  poll of its work, so a thread working with nothing in flight (an
  injected fault, a nudge to drop stopped instances) is still watched. A
  request being sent wakes both, the thread starting a poll wakes the
  watchdog, and the thread's end wakes both, for the ticker's last tick.
  Each checks and waits under the lock that the count and the poll change
  under, so a call that starts as a timer goes to wait still wakes it, and
  no computing guest is left unticked. So while Pane is quiet, neither
  thread wakes at all. While anything is in flight, both behave
  exactly as below: a tick every 10 ms, a look every 100 ms, the compute
  limit and the give-up. The watchdog does not count the time it waited:
  it starts its count afresh as it wakes, and a "not responding yet" it
  had said is withdrawn as it goes to wait, since the thread was then seen
  outside any poll. Between a service's cycles, or a schedule's runs, both
  wait.
- **The meter** counts a call's compute time as the runtime thread's CPU
  time while it polls the call (`CLOCK_THREAD_CPUTIME_ID` on Linux and
  macOS, `GetThreadTimes` on Windows; wall time inside those polls where
  neither is available), **less the time inside Pane's host calls**. Every
  host import marks its entry and exit in one place (`Watch::host`, and
  `hosted` for a host call's future: saving data, running a helper,
  sending a web request and receiving its response; a host call inside
  another is counted once). So a slow save, a slow system call or a machine
  too loaded to run the thread never counts against the guest. A guest
  that waits is not polled at all. The meter is **per call and
  cumulative**: awaiting between parts of a computation does not reset
  it; only the call finishing does. Serving the operations a guest calls
  counts for their targets, not for it. **Starting an instance is never
  counted**: a slow start is not a failure to start; a start that never
  finishes holds the runtime thread until its package is disabled,
  reloaded, updated or uninstalled, which stops it. A custom view's
  destructor is metered like a call.
- **Saving** is written by a writer thread: the runtime thread changes the
  value in memory and awaits the write, so it never waits on the file
  system. **Helpers** are found and started on a thread of their own.
  [Clipboard history](clipboard-history.md) (#35, #36) is the exception:
  a command's change to it (turning it on, pausing, excluding a program,
  setting the retention, deleting items, clearing, turning it off and
  clearing) is written by the runtime thread inside its marked host call,
  so the write is never charged to the guest nor given up on, but the
  thread waits for it. Its expiry runs on a thread of its own.
- **The watchdog.** Each runtime thread has a heartbeat, bumped at each
  poll of its work, each epoch yield of a guest and as each host call
  starts and ends. A watchdog thread looks every 100 ms while a request
  is in flight or the thread is inside a poll (it sleeps otherwise, see
  above). A thread waiting
  for work, or awaiting a guest's host work, is not polled, so never quiet;
  one inside a host call is never given up on; one computing a guest
  beats at every tick (and is stopped by the meter). A thread inside one
  poll, outside any host call, whose heartbeat stays still for 10 seconds
  is said to be **not responding yet** in the status line ("Pane's
  extension runtime is not responding yet. Pane starts it again if it
  stays stuck; saved data is kept."), and what the status line said before
  comes back if it carries on. After 30 seconds, Pane gives up on it,
  exactly as last seen: a thread that left its poll or beat meanwhile
  (checked under the lock it leaves its poll under) is left alone.
  Compiling a component is exempt. Only time the watchdog itself saw
  counts: the time between two of its looks counts at most one second
  (`LOOK_GAP`), so a whole process stopped meanwhile (by a debugger,
  SIGSTOP or Ctrl-Z, or a computer asleep on a system whose clock counts
  sleep), whose runtime thread did not run either, is not given up on as
  it resumes. Giving up: every call the thread held
  (running or queued, and `Runtime::running` or `view_count` asked of it)
  answers "Extension runtime unavailable: it stopped responding before
  answering and was started again; Pane does not run this again by
  itself", its native helpers are ended, a fresh thread serves the next
  call (unless the runtime already failed within 5 minutes before: hangs
  and crashes share the restart window), and the status line and Manage
  extensions ("Why the extension runtime stopped", "Restarted after not
  responding") say that the runtime made no progress, that it was not
  running an extension's code nor inside one of Pane's host calls, and
  that which code held it is not known; its last known work (waiting,
  handling a request, starting a guest instance or running a guest call)
  is in the diagnostics. No extension is named or paused.
- **The fence.** A thread cannot be ended from outside: the stuck one is
  abandoned, and its fence closes. Stopped code is one check that every
  host interface goes through (`runtime::code_stopped`, through
  `GuestState::stopped` and `PackageData::stopped`): data, applications,
  operations, helpers, the folder listing, web requests and clipboard
  history (its reads too) all
  refuse code whose generation ended or whose thread's fence closed. A
  save, or a change of clipboard history, checks and stages its change
  while holding the fence open, and closing waits for it, so no save
  lands after the give-up. A guest the thread still runs traps at its
  next tick (the ticker's last tick makes sure it reaches one), the thread
  runs nothing more once it returns, and only then frees its instances
  and memory (`Runtime::abandoned_threads` counts those still stuck). The
  launcher closes a custom view it held, as after a crash. A management
  action that waits for the runtime (uninstalling, clearing a cache)
  waits for it at most 30 seconds.
- **Locks.** A fresh thread must never wait on something an abandoned one
  holds, or its failure would use up the restart window for the first
  one's. What runtime threads share is held only briefly and never across
  blocking work: the extension data lock while a value is read or staged
  (a cache or uninstall removal reads its file without it), clipboard
  history's lock while it is read or changed in memory (its file is written
  after, one write at a time, so a change can wait for a write in
  progress, the clipboard listener's or one made just before a give-up),
  the helpers'
  lock while a run is registered (a process starts without it), the
  runtime's own state. What remains: the runtime thread reports a
  package's failure to the launcher, which takes the launcher's lock
  briefly; a thread stuck inside that would have stopped the launcher too.
- Other active extensions: their calls wait behind the running call,
  behind a computing guest for up to the compute limit, and behind one
  waiting or looping on host calls until it ends or its generation does
  (see the limits below); their instances and open views are kept. After a runtime hang, every extension's instances and views go with
  the abandoned thread, as after a crash.

**Provisional, pending user confirmation:** the limits (5 seconds of a
guest's computing, 10 then 30 seconds of a thread
making no progress) and the 10 ms tick; an unresponsive call counts as a
crash towards pausing (not a pause at once, and not free); a start is
never metered; the operation error kind for a target that stops
responding is `crashed` (the WIT has no kind of its own); a helper past
its limit fails with `failed`, not `refused`; compiling is exempt from the
watchdog; the stuck thread is abandoned, since the runtime stays a thread
in Pane's process ([Q39](current-decisions.md)).

Faults and limits for tests and smokes (debug builds only, like #17's):
`Runtime::inject(Fault::Hang)` blocks the runtime thread wherever it next
checks for faults (waiting for a request, or between a guest's yields),
as a thread stuck outside any host call would, until `Fault::Release`;
`Fault::SlowHostCall(d)` makes the next host call compute for `d` itself.
`Runtime::set_limits` shortens the limits; the fault file takes `hang`,
`release` and `limits:<compute>,<warn>,<unresponsive>` (seconds).

Limits:

- Where a system has no per-thread CPU clock, the meter counts wall time
  inside the guest's polls, so there a loaded machine counts against the
  guest.
- A runtime hang abandons a thread that keeps its memory until it returns;
  a thread stuck for good keeps it until Pane quits. A host call that
  never returns is never given up on (it is Pane's own work, marked as
  such), and neither is a compile that never ends; the runtime then stays
  stuck until Pane restarts.
- A call waiting on another extension's operation, or on a clock, has no
  time limit; a user cannot cancel a running action yet.
- A guest looping on Pane's host calls (saving a setting over and over,
  say) is charged only its own computing between them, so the compute
  limit may take very long to reach, and each call's heartbeat keeps the
  watchdog from giving up; the same holds for one waiting on a clock
  again and again. Meanwhile every other extension's calls wait, and no
  extension is named. A host call that never returns (a hung disk, or on
  Windows a clipboard owner that does not answer while `copy` puts an
  item back on the clipboard) is never given up on, and nothing says so.
  Not in #18; a proposed follow-up is a wall-clock notice naming the
  running package ("… is taking long"), with no blame or pause, user
  cancellation of a running call, and `copy` run off the runtime thread
  with a timeout.
- After a give-up, the abandoned thread keeps what it held until it
  returns: its guests' web requests keep their connection slots (four
  per package), so a package whose requests it held has fewer
  connections, or none, meanwhile; and a thread stuck spinning in native
  code keeps a processor core busy until it returns or Pane quits.
- Stopping a computing guest drops its instance and what it keeps in
  memory, like any stopped call.

## When Pane itself ends: its log and the crash notice

A crash that ends Pane's process (an abort, a fault in native code, a panic
outside the extension runtime's thread) is not recovered. Since #133 it at
least leaves a trace on this computer, and the next start says so. Nothing
is sent anywhere: there is no telemetry, no crash upload and no minidump.

- **One diagnostic path.** Every message Pane writes to standard error, in
  `pane-core` and in `pane`, goes through `pane_core::diagnostics::report`
  (the `diagnostic!` macro): it still writes to standard error, so a
  developer running Pane from a terminal sees no change, and it also
  writes to Pane's log. On Windows a release build is a windowed program
  whose standard error goes nowhere, so the log is where its runtime crash
  details, pause reasons and failures to save are kept. An extension's own
  output is not included. A unit test fails if a source file of either
  crate writes to standard error by itself.
- **Where.** `pane.log` in a logs folder in the system's place for logs:
  `%LOCALAPPDATA%\Pane\logs` on Windows, `~/Library/Logs/Pane` on macOS
  (where Console shows it), `$XDG_STATE_HOME/pane/logs` (by default
  `~/.local/state/pane/logs`) on Linux, and `logs` inside `PANE_DATA_DIR`
  when that is set (tests and smokes). The folder is readable by the user
  only (mode 0700; on Windows a protected DACL for the user and SYSTEM).
- **Size and rotation.** One log is appended to across starts. Past 2 MiB it
  becomes `pane.1.log`, the older files shift, and at most 5 older files are
  kept (6 in all, about 12 MiB at most). If `pane.log` cannot be moved (on
  Windows another program may hold it open), no older file is shifted or
  removed: Pane keeps appending to it and tries again once it has grown by
  another 2 MiB. Each line starts with its UTC time
  (`2026-10-08T05:06:07.089Z`); each run starts with a line naming Pane's
  version and its process. A message of several lines (a panic's
  backtrace) keeps them, indented.
- **Rate limit.** One site — a message's fixed text, its format string,
  before its values — writes at most 10 lines a minute. What it says beyond
  that is held back, and the first line of its next minute is preceded by
  one saying how many similar lines were left out ("5 similar lines were
  left out: …"). A clean quit writes those counts too.
- **Redaction.** At the log's writer, before anything is written: the home
  folder's path becomes `~`, the user's name `<user>` and the computer's
  name `<computer>`, each matched without regard to case (and with either
  slash), only as a whole word or path component (a user called "admin"
  leaves "administrator" alone), and only when it is at least 3 characters
  long. A user or computer name that identifies nobody and is also a word
  Pane's messages use ("admin", "administrator", "localhost", "pane",
  "root", "user") is not replaced; a home folder named after one still
  becomes `~`. Other paths are
  kept, since a diagnosis needs them. Pane's own messages do not carry
  extension data values, local credentials, clipboard history text, query
  text or found file names: a web image that shows its fallback is named by
  its address (`host:port`) only, since the rest of an extension's URL can
  carry what the user typed, and any other URL its error names (escaped, or
  one a redirect led to) is written as `<url>`. Standard error still gets
  the message as it
  was.
- **Panics.** A panic hook writes the panic's thread, location, message and
  any captured backtrace (`RUST_BACKTRACE`) to the log before the default
  handling, which still prints it. A recovered [runtime crash](#when-the-extension-runtime-itself-crashes)
  is logged like any other message.
- **The marker.** At start Pane writes `running-<process id>.json` in the
  logs folder, holding its process id, when the process started and Pane's
  version. A clean quit removes it: the tray's or menu bar's Quit, closing
  the launcher's window, and the system ending the session (GPUI's quit
  hooks run for `WM_ENDSESSION` on Windows and the termination notification
  on macOS; SIGTERM, SIGINT and SIGHUP on Linux and macOS). A clean quit
  also writes the clipboard history that waits in a batch (#192,
  [clipboard history](clipboard-history.md#ownership-and-deletion)). For
  the signals, the handler only writes a byte to a pipe; the thread
  `pane-signals`, woken by it, runs the clean quit on a thread of its own,
  waits for it at most 2 seconds, removes the marker whether it ended or
  not, and ends Pane with the signal's own default action, as it always
  ended. A signal received meanwhile (the session's end sends SIGTERM and
  SIGHUP together) changes nothing. Should the pipe or that thread not
  exist, the handler removes the marker itself and ends Pane at once,
  writing nothing of the batch. The signals run only that clean quit, not
  the app's other quit hooks (the tray, the hotkeys, the runtime's
  helpers). At the next start each marker found is
  checked against the system's process table (on Windows the process's
  exit code and creation time, on Linux `/proc/<id>/stat`, on macOS the
  process's BSD information): a process that no longer runs, or a process
  with the same id that started at another time, means that run ended
  unexpectedly, and its marker goes; a process that still runs, started
  when the marker says, is another Pane on the same folder, and its marker
  is left alone. Where the system does not say when a process started, a
  running process counts as the marker's. Each start writes its own.
- **The notice.** After an unexpected end, root search lists one root
  result, **Pane quit unexpectedly last time**, after the application
  update's rows, whose action opens the logs folder with the system's file
  manager, and the status line says it too; its Actions panel has
  **Dismiss Notice**. Settings' About page shows the same notice in its
  **Log** row, beside the diagnostics, with **Open log folder**, and
  **Copy diagnostics** now includes the log's folder; the data and log
  folders it copies are redacted as the log is (the home folder as `~`),
  so the report can go into a public bug report. The notice goes when
  the user dismisses it, opens the folder, or Pane next quits cleanly and
  starts again.

Checked by unit tests in `pane-core` (`diagnostics`: redaction, rotation,
the rate limit, the marker's decisions over a fake process table, a panic in
the log, the one diagnostic path; `clipboard/history` and
`launcher/icon_loads`: a clipboard item and a query never reach the log),
by `crates/pane/tests/crash_record.rs` (a start after a crash lists the
notice, dismissing it removes it, and the About page shows it with the log's
folder in the diagnostics) and by `crates/pane/tests/tray.rs` (the tray's
Quit leaves no marker).

## Author example and tests

- The Rust settings sample's **Count** adds one to a count in its content
  and answers it: after a runtime crash lost its answer, the count shows
  it ran once and was not run again.
- The settings samples' **Crash** item
  ([Rust](../guests/sample-settings/src/lib.rs),
  [JavaScript](../guests/sample-settings-js/src/index.js),
  [TypeScript](../guests/sample-settings-ts/src/index.ts)) crashes on
  purpose: a Rust panic traps; in JavaScript and TypeScript, resolving an
  action with something other than a string does (throwing is an error the
  extension answers with).
- [`crates/pane-core/tests/pausing.rs`](../crates/pane-core/tests/pausing.rs):
  three crashes pause each language's sample, with its data kept, the pause
  held after a restart and Retry starting it; errors it answers with never
  pause it; an `Error` thrown from a form is an error in each language; the
  crash count starts afresh after a restart; another package keeps running;
  disabling, enabling or reloading ends the pause; a pause recorded for
  another version does not hold; a Retry without a runtime keeps the pause;
  a component that cannot load is paused at once; an uninstall that cannot
  be recorded keeps the pause; a reload that fails to start is paused across
  a restart; a root result provider that keeps crashing is paused and asked
  no more. [`aliases.rs`](../crates/pane-core/tests/aliases.rs): a command
  whose query ("crash") traps three times is paused, in Rust, JavaScript and
  TypeScript, and its alias row then explains the pause and runs nothing. Unit tests in `launcher/pausing.rs` cover the crash window with
  explicit times and crashes of code disabled meanwhile.
- [`crates/pane-core/tests/runtime_crash.rs`](../crates/pane-core/tests/runtime_crash.rs)
  (#17): with the settings and helper samples running, a crash ends the
  waiting helper (Pane lists none and its heartbeat stops), names no
  extension, pauses nothing, keeps saved data and restarts the runtime;
  Settings › Extensions, disable and uninstall work; a second crash soon after
  stops it until **Restart the extension runtime**; the settings sample's
  **Count**, whose answer a crash lost after it saved, is not run again.
  [`runtime.rs`](../crates/pane-core/src/runtime.rs) checks that a
  restarted thread never reuses a view id; `runtime/supervisor.rs` tests
  the restart window with explicit times.
  [`crates/pane/tests/runtime_crash.rs`](../crates/pane/tests/runtime_crash.rs):
  in the window, a crash closes the open custom view and redraws with the
  explanation; the details and Restart rows render and work. The native
  smokes' runtime-crash phase (frames 200 to 209) does the same with real
  key events.
- The settings samples' **Stop responding** (#18; Rust, JavaScript and
  TypeScript alike) saves `busy` as "started", then computes without
  waiting for up to a minute (bounded, so it ends even without Pane)
  before saving "finished".
- [`crates/pane-core/tests/unresponsive.rs`](../crates/pane-core/tests/unresponsive.rs)
  (#18, with limits shortened through `Runtime::set_limits`): in each
  language, while Stop responding computes, Settings › Extensions opens and
  the calculator (another extension) answers as soon as the call is
  stopped; the call is stopped after the compute limit and says why, never
  saves "finished" and is not run again; the third time pauses the package
  ("stopped responding 3 times within 5 minutes"), with its data kept,
  details and Retry. A runtime thread made to hang is said to be not
  responding yet, then given up on: nothing is paused or named, Manage
  extensions says "Restarted after not responding", a fresh thread runs
  the next call and the calculator, and the stuck thread, released, saves
  nothing and ends; one released before the give-up carries on, its call
  answers and the status line is put back.
  [`helpers.rs`](../crates/pane-core/tests/helpers.rs) ends a waiting
  helper when its thread is given up on. Unit tests in `runtime.rs` stop a
  computing guest (another command's open view kept), stop it on disable,
  never stop nor blame a guest whose host call computes for three times
  the compute limit (`Fault::SlowHostCall`), a clipboard history call's
  too, replace a hung thread, answer
  `running` and `view_count` on a give-up, and never restore the obsolete
  generation of a package reloaded during a hang; `deadlines.rs` tests the
  watchdog's verdicts, the give-up race and a meter that charges neither a
  host call nor time the thread did not run; `extension_data.rs` that no
  save or clipboard history change lands after the fence closes and
  fenced code reads no clipboard history; `http.rs` that fenced code sends
  nothing; and `launcher/pausing.rs` counting unresponsive calls with
  crashes.
  [`crates/pane/tests/unresponsive.rs`](../crates/pane/tests/unresponsive.rs):
  in the window, keys are answered while the guest computes, and the error,
  the pause toast, the paused command's reason and Retry render. The
  native smokes' unresponsive phase (frames 240 to 248, data folder
  `unresponsive-data`) does the same with real key events.
- [`crates/pane-core/tests/operations.rs`](../crates/pane-core/tests/operations.rs):
  a target that keeps crashing is paused and its caller is not; a target
  stopped with its caller again and again is not paused.
- [`crates/pane-core/tests/hotkeys.rs`](../crates/pane-core/tests/hotkeys.rs):
  a paused command's hotkey stays registered and explains the pause.
- [`crates/pane/tests/install.rs`](../crates/pane/tests/install.rs): in the
  window, three crashes show the toast and the paused command's reason, and
  Retry in the extension list starts it again; the details of a reload that
  failed to start are rendered on their own screen.

## Limits

- The toast is the launcher's status line, replaced by the next action's
  outcome; the lasting status is in Settings › Extensions. When a crash of an
  operation's target pauses it, the caller's own answer (which reports the
  crash) is shown instead of the toast.
- A guest computing without waiting is stopped after 5 seconds of
  computing and counted like a crash (#18); one that waits forever on
  another extension's operation or a clock is not stopped until its
  generation ends.
- The crash count is not kept across restarts; a package that crashes
  twice per session is never paused.
- `pane_js.py`'s generated entry, which applies the JS adapter, is not part
  of the prebuilt components' input digest (the adapter itself is): a change
  to that template alone needs `cargo xtask js-guests` by hand.
- Nothing here is platform-specific (it lives in `pane-core`); it has run on
  Linux, and runs in `cargo xtask ci` on Windows, macOS and Linux.
