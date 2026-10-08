# 0024: The host runs scheduled extension work by its own clock

Status: proposed (for [#47](https://github.com/pane-app/pane/issues/47);
the specification's Implementation Decision 13 accepts scheduled work as
an activation model and leaves intervals and the details open)

## Context

The specification wants installed extensions to do work on a schedule
without the user asking (US45, "lazy commands/events, scheduled work and
explicit continuing background services"), while installation alone never
keeps an executable instance alive (US14) and disabling stops
host-managed work (US57). The activation model is otherwise settled:
commands open lazily, every call into a package belongs to a
[generation](../generations.md) that a disable, reload, update or pause
ends, and attributable failures [pause](../pausing.md) the package.
Ticket #47 asks for one schedule kind with a bounded policy, not a
general scheduler.

A WASI 0.3 guest cannot keep itself alive: nothing runs it between calls.
Timer-like work therefore has to be driven by the host, or the package
would need an explicit background service (US45's third model, ticket
#48). The host also already has a clock seam for time-driven host work:
clipboard history expiry is driven by the launcher's clock
([ADR 0023](0023-host-expires-clipboard-history-by-its-own-clock.md)),
which tests and development builds replace with a manual one so time
passes only when they move it.

## Decision

The package manifest declares the schedule, and Pane's host runs it:

- A command's `pane.json` entry may declare
  `"schedule": { "everySeconds": <seconds>, "item": "<item id>" }`: the
  command's action of that item runs every interval. One schedule kind,
  a fixed interval; the bounds (1 second to 30 days) are provisional
  pending the user's choice of scheduling intervals. An impossible
  declaration is refused before anything is installed.
- The scheduler is host-side, in the launcher: a thread of Pane's own
  that wakes when a run is due (and whenever a package's generation
  changes), starting each run on a thread of its own so the window never
  waits for a guest. It follows the launcher's clock — the system's, or
  the manual one of tests and development builds — so scheduling is
  checked through public behavior without real time passing.
- The schedule belongs to the package's generation: it runs while the
  code may run (enabled and not paused), and ends when it may not. Each
  run is asked for with the generation current when it is due, so an end
  of that generation stops a pending run and its late answer is
  discarded, exactly as for a user action. At most one run of a command
  is asked for at a time; ticks that fall due meanwhile are coalesced.
- The scheduled run is an ordinary guest action, so its failures follow
  the established policy for free: a trap is a crash of the package (the
  third within five minutes pauses it, which ends the schedule), and an
  error the extension answers with is an expected error.
- The answer is shown on the command's screen while that screen is on
  display, as an action's answer is; the run happens whether or not it
  is.

Deliberately not chosen: guest-side timers (a guest cannot run between
calls, and a host-driven "wake the guest" API would be a second,
parallel scheduling model); OS timer integration (no cross-platform
parity, and nothing needs Pane to be woken from sleep — the schedule
simply runs again when Pane runs); replay of missed work (the
specification's T02 distinguishes "declared background work" from
"everything ran"; a restart schedules again what the manifest declares,
from a full interval, which keeps that distinction honest); and a
persisted record of the next run (the manifest is the declaration, so
the schedule survives restarts by being read again).

## Consequences

- The host owns no scheduling state beyond each schedule's current
  phase, kept in memory: a restart or a disable-and-enable restarts the
  interval, and no schedule survives Pane being stopped.
- A scheduled command's activation is observable exactly like a user's:
  the guest instance starts when the run is due, and nothing else of the
  package runs. Tests assert this through the public launcher interface
  with a manual clock.
- Authors write the work as the action of a listed item, which the user
  can also run by hand; there is no separate "background task" entry
  point. This is the minimal contract the ticket allows and is
  provisional pending user confirmation.
- The clock seam stays where clipboard history put it
  (`Launcher::with_clock`); scheduled work sharing it means one
  development seam for all clock-driven host work. Release builds keep
  the system's clock.
- More than one Pane process on the same data folder would each run the
  same schedule, like every uncoordinated record on it.
