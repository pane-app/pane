# Native validation — ticket #88: popup, hover and press micro-transitions

**Status: deferred — a plan, not results.** The host machine that would run
the captures is in interactive use (the user games on it), so no native run
was performed for this ticket; native/visual validation is #84's pass for
the whole milestone. This file records exactly what to capture when a host
is available, so that pass can execute it without re-deriving the design.
Source constants alone do not prove smoothness; until these captures
exist, the only completed evidence for #88 is the test platform's
controlled-clock integration tests — the popup tests in
`crates/pane/tests/launcher_settings.rs` (entrance from the trigger,
resting alignment and width stability, the inert exit with the focus
already restored, reopen retargeting with no surviving overlay, filtering
animating nothing, reduced motion) and `crates/pane/tests/settings.rs`
(the footer menu's entrance and exit, reversal, inertness and reduced
motion), and the pointer-feedback tests in `crates/pane/tests/window.rs`
and `crates/pane/tests/launcher_settings.rs` (fading hover washes, the
press's stronger wash with unmoved geometry, immediate activation,
immediate keyboard selection, idle settling) — run by CI on Linux,
Windows and macOS at the `[verify]` tier.

Everything below uses a **debug build** of the pinned working tree (the
renderer pin `bcf3a0acd047c1873293069d0ed42085a38f699b` is recorded in
`crates/pane/Cargo.toml`), because the capture hooks are debug-only by
design (see `crates/pane/src/ui/motion.rs`):

- `PANE_MOTION_SCALE=<factor>` — stretches **every** transition timeline
  by that factor (clamped 0.25–16), now including the popup entrance
  (140 ms), the popup exit (100 ms) and the pointer fade (150 ms), because
  they all go through the same policy. At 8× a popup entrance becomes
  1.12 s and the exit 0.8 s, so a screenshot taken a known delay after a
  click samples a definite point of the curve instead of chasing a fast
  blur. A release build ignores it entirely.
- `PANE_TEST_REDUCE_MOTION=1` — runs with reduced motion engaged and skips
  the native preference read and watch, so the reduced presentation can
  be captured without touching the operator's own system settings. A
  release build ignores it entirely.

## Helper change needed first

The same one #86 and #87's plans call for: a copy of
`docs/evidence/settings-71/capture-pane.ps1` (the established, safe launch
helper) extended with `-MotionScale <float>`, `-ReduceMotion <switch>` and
a small `-KeyDelayMs` that can sample mid-transition frames — one helper
shared by the motion tickets, if #86's pass has not already written it.
#88 adds one need those plans did not have: the popups and the pointer
feedback are **mouse-driven** (clicks on the ellipsis, the select's
trigger, rows and the primary action, and a pointer that enters and
leaves a control mid-fade), so the helper needs to move and click at
pixel coordinates as well as send key tokens — SendInput-based moves and
clicks at computed coordinates, with the same safety rules (per-run
scratch `PANE_DATA_DIR` and scratch `LOCALAPPDATA`, `PANE_ARTIFACTS`
cleared for the child, foreground HWND confirmed before every input, only
the spawned PID closed, no deletion anywhere).

## What to capture

### The footer menu's popup

1. **Entrance** — click the ellipsis: the popup rises out of the strip
   (a 3 px shift down from its rest above the strip) over 140 ms, its
   surface, shadow and contents moving together, already faintly visible
   on the first frame. At 8× this is plainly visible; capture three
   frames across it.
2. **Exit** — Escape: the popup recedes into the strip over 100 ms and
   fades all the way out before unmounting; the button's glyph color
   returns at once. Also an outside click on a result row: the dismissal
   consumes the click (the row does not open).
3. **Reversal** — close and immediately click the ellipsis again: the
   popup turns around from where it is (no restart flash, no second
   overlay); while the exit ran, a click where the popup was reached
   nothing.
4. **The window edge** — the popup sits at the window's bottom-left
   corner by design; verify the entrance's 3 px overlap with the strip
   paints over the strip's own background without clipping, and that a
   window shorter than the popup's snap room still snaps the popup
   inside it at rest.

### The Settings select's popup

5. **Entrance** — click the opening-monitor trigger on the Launcher page:
   the dropdown enters from the trigger (a 3 px shift up from its rest
   below it) over 140 ms and settles left-aligned below the trigger at
   the width coordination #85 left (content-width, about the trigger's).
   The width must not change while it moves — that is this ticket's
   anchoring guarantee, worth one clip at 8×.
6. **Exit with a draft** — type a query, Escape: the exit paints the
   filtered list exactly as the user left it while the keyboard is
   already back on the trigger; a click on the fading popup commits
   nothing.
7. **Clipping near the edge** — resize the Settings window to its floor
   (560×400) and scroll the page so the trigger sits near the bottom:
   the popup snaps inside the window at rest, and the entrance's shift
   never moves the snap (the anchored element measures the resting
   size). The harness asserts this; a clip proves it visually.

### Pointer feedback

8. **Hover and press** — the ellipsis button, the footer's primary
   action, the select's trigger, the Settings rows and the Shortcuts
   cells: the hover wash fades in over 150 ms; a fast reversal (enter,
   leave before it settles) continues from the wash on screen; a press
   takes the stronger wash (the primary action's relaxes one rung) with
   the control's geometry and hit target exactly unmoved, and activation
   never waits on the fade.
9. **Keyboard beside the pointer** — with the pointer resting on one
   row, move the selection with the arrows: the active option's wash
   lands at once next to the fading pointer washes (nothing of the
   selection fades).
10. **Theme change mid-transition** — flip Appearance while a wash or a
    popup is in flight: the transition continues to the current tokens'
    endpoints.

### Reduced motion

11. `PANE_TEST_REDUCE_MOTION=1` — popups appear and vanish at their
    endpoints, washes snap, and nothing schedules frames. On Windows,
    also flip the system's "Animation effects" preference while a
    transition runs: the next drawn frame settles at once (the watch the
    motion policy wires).

## Idle and frame observations

Alongside the clips, record the idle behavior the harness asserts: a
settled control schedules no cosmetic frame (nothing animates while the
window sits still), and a popup's exit ends with the popup fully
unmounted — nothing of it remains to intercept a click. Keep
`PANE_MOTION_SCALE` out of any clip that measures timing.

One known non-idle to expect and not misread as a leak: a pointer parked
over content that reflowed under it — a popup unmounting over where the
pointer rests, a list scrolling beneath it. The renderer's style
transitions read the hover for the layout pass from the hover state a
real mouse move set, so until the pointer moves again a wash there can
sit mid-fade and keep the window asking for frames; the first real move
settles it. The test harness sends the pointer away before counting
frames for exactly this reason, and the fix for it belongs to the
renderer (the pinned fork's hover tracking), not to Pane's motion
policy — worth an observation in this pass, nothing more.
