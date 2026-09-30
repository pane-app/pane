# 0025: A continuing service cycles at its own cadence

Status: proposed (for [#48](https://github.com/hoangvu12/pane/issues/48);
the specification's Implementation Decision 13 accepts "explicit ongoing
services" as an activation model and leaves their shape open)

## Context

The specification wants extensions to run work without the user asking in
three ways (US45): lazily (commands, when used), on a schedule
([#47](https://github.com/hoangvu12/pane/issues/47), at an interval the
manifest declares), and as explicit continuing services, which "run while
the package's code may run" rather than on an interval. Installation
alone never keeps an executable instance alive (US14), and disabling
stops host-managed work (US57); attributable failures
[pause](../pausing.md) the package (US79). Ticket #48 asks for one
service with a bounded policy, not a general service framework.

Two shapes were available for how a guest expresses a service:

- **A long-lived call.** The service would be one `run-action`-like call
  Pane holds open while the code may run. Pane's runtime serves every
  guest call on one shared thread, one at a time (a deliberate
  architecture: the epoch-yield machinery, the watchdog and crash
  recovery all rest on it), so one held call would hold every other
  extension's calls behind it for as long as the service lives. A
  service is exactly the thing that lives long. Rejected.
- **A host-driven cycle.** Pane calls the guest, the guest runs a slice
  of the service's work and answers, and Pane calls it again later. The
  instance — the service's task state — lives between cycles for the
  whole generation, exactly as any command's instance does. Chosen.

A cycle needs one thing an ordinary action's answer cannot carry: when
to run the next cycle. #47's schedule gets its time from the manifest;
if Pane picked the interval itself, a service would just be a schedule
with the declaration moved, and "runs while the code may run rather
than on an interval" would be false. So the cadence comes from the
guest, cycle by cycle.

## Decision

- A command's `pane.json` entry may set `"service": true`: while the
  package's code may run (enabled and not paused), Pane runs the
  component's `run-cycle` export in a cycle. One service per command —
  the minimal bounded shape, mirroring the schedule's per-command slot;
  a package whose several commands declare one runs several, as several
  schedules. A component that does not export the interface is refused
  at preview and install.
- The guest-facing surface is a new, minimal interface,
  `pane:extension/service` ([`wit/service.wit`](../../wit/service.wit)):
  `run-cycle(command) -> result<cycle, string>`, where `cycle` is the
  status to show and `next-seconds` to wait before the next cycle
  (bounds 1 second to 30 days, clamped, provisional as #47's are). No
  event stream, no long-lived call, no cancellation API of its own.
- The host runner is a thread of Pane's own in the launcher
  (`launcher/services.rs`), shaped like the scheduler's and driven by
  the same seams: the launcher's clock (`Launcher::with_clock`) and the
  extension data's generation-change hooks. Each cycle runs on a thread
  of its own, so the window never waits. Each cycle is asked for with
  the generation current at dispatch, so an ended generation stops a
  pending cycle and discards its late answer, exactly as #47's runs
  are.
- The service begins the moment the code may run — Pane starts, the
  package is installed or enabled, or its code is replaced — and its
  first cycle runs at once, unlike a schedule's full first interval: a
  service's declaration is a request to run, not a request to be woken
  later. It ends when the code may not: disabled, uninstalled, paused,
  or replaced, which begins a new one. Work is never replayed for time
  the code could not run.
- Each cycle is an ordinary guest call, so the recovery policy is
  free: a trap or a stopped-responding cycle is a crash (the third
  within five minutes pauses the package, ending the service); an
  answered error is an expected error, however often. After a failed
  cycle the next runs one second later (provisional), so a service the
  user enabled is never ended by one broken cycle — and so repeated
  crashes can reach the pause policy at all.
- The status of a cycle shows on the command's screen while that screen
  is on display, as a scheduled run's answer does; the cycle runs
  whether or not it is.

Deliberately not chosen: a long-lived call or an event-stream import
(both would need the runtime to serve a guest while it waits, or a
second call-serving model, contradicting the one-thread runtime);
host-chosen polling intervals (a service in name only); a per-package
service slot (a service answers on a command's screen, as a schedule
runs a command's item, so the command's entry declares it); and
turning a service off without disabling the package (a later, explicit
control — the same one a schedule lacks).

## Consequences

- The host owns no service state beyond each entry's phase, kept in
  memory: a restart starts services again from the manifest, and no
  service survives Pane being stopped. At most one cycle of a service
  is in flight, and a service whose entry went with its generation
  (replaced code) ignores the dead code's late report.
- The instance is the service's task: its in-memory state lives for the
  generation and is dropped with it (the cost
  [generations](../generations.md) already prices), so "the service's
  task stops on disable" is the instance drop that already exists, and
  the guest's saved data (settings, content) is kept as for any
  package.
- Authors write each cycle as a short slice of work that answers when
  to run again; a cycle that waits inside the guest holds every other
  extension's calls for as long as it waits, as any call does today.
- The clock seam stays where clipboard history and scheduled work put
  it; tests drive a service's cadence by moving the manual clock and
  wait for the runner to settle (`Launcher::wait_for_services`), as
  they do for schedules.
- A cycle that runs while its package's code is being replaced finds
  the old component gone: the runner waits for the replacement (every
  path that records one pokes it) rather than failing a cycle against
  dead code.
- More than one Pane process on the same data folder would each run the
  same service, like every uncoordinated record on it.
