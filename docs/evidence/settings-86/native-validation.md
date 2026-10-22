# Native validation — ticket #86: command open/back view transitions

**Status: deferred — a plan, not results.** The host machine that would run
the captures is in interactive use (the user games on it), so no native run
was performed for this ticket. This file records exactly what to capture
when a host is available, so the milestone's validation pass can execute it
without re-deriving the design. Source constants alone do not prove
smoothness (the ticket says so); until these captures exist, the only
completed evidence for #86 is the test platform's controlled-clock
integration tests in `crates/pane/tests/window.rs` (interruption,
retargeting, immediacy, cancellation, reduced motion, idle completion) run
by CI on Linux, Windows and macOS.

Everything below uses a **debug build** of the pinned working tree (the
renderer pin `bcf3a0acd047c1873293069d0ed42085a38f699b` is recorded in
`crates/pane/Cargo.toml`), because the two capture hooks are debug-only by
design:

- `PANE_MOTION_SCALE=<factor>` — stretches every view-transition timeline
  by that factor (clamped 0.25–16). At 8×, the 150 ms entrance becomes
  1.2 s and the 120 ms return 0.96 s, so a screenshot taken a known delay
  after a keystroke samples a definite point of the curve instead of
  chasing a fast blur. A release build ignores it entirely.
- `PANE_TEST_REDUCE_MOTION=1` — runs with reduced motion engaged and skips
  the native preference read and watch, so the reduced presentation can be
  captured without touching the operator's own system settings. A release
  build ignores it entirely.

## Helper change needed first

`docs/evidence/settings-71/capture-pane.ps1` (the established, safe launch
helper) passes only `PANE_DATA_DIR`, `PANE_EXTENSIONS_DIR`, `PANE_THEME`,
`PANE_MATERIAL`, `PANE_ARTIFACTS` and `LOCALAPPDATA` spawn-scoped to the
child. Copy it to this directory as `capture-motion.ps1` and add two
optional parameters (`-MotionScale <float>`, `-ReduceMotion <switch>`)
that set `PANE_MOTION_SCALE` / `PANE_TEST_REDUCE_MOTION` spawn-scoped and
record them in `pane-run.json`, plus a `-KeyDelayMs` small enough to
sample mid-arrival frames (the current default, 700 ms, always lands
settled). All the existing safety rules carry over unchanged: per-run
scratch `PANE_DATA_DIR` and scratch `LOCALAPPDATA`, `PANE_ARTIFACTS`
cleared for the child, foreground HWND confirmed before every key token,
only the spawned PID closed, no deletion anywhere.

Every run's `pane-run.json` already records OS, DPI, window bounds and
focus method; the plan's checks are pixel-based, so also record the
binary's commit (`git rev-parse HEAD`) in the run notes.

## Captures to take (Windows first — the available host)

All with the sample extensions the helper installs (root search lists
"Rust sample", so a command is always selected). Pixel checks run on the
**opaque** material (deterministic colors, as in #71); one glass run per
theme is for perceptual review only.

| ID | Setup | Keys / actions | What the frames must show |
| --- | --- | --- | --- |
| F1 | dark, opaque, scale 8, delay ≈ 100 ms | `rust`, `{ENTER}` then captures at ~100, 400, 700, 1000, 1600 ms after the Enter | The list region starts displaced ~3 logical px *below* rest and faint (~30% opacity floor) and settles to rest over ~1.2 s; the query field, heading and footer strips stay pixel-identical throughout (compare against the pre-Enter capture). |
| F2 | dark, opaque, scale 8, delay ≈ 100 ms | from the command screen, `{ESC}` then the same timed captures | The paired return: root's list starts ~3 px *above* rest and settles over ~0.96 s — visibly quicker than F1's entrance; chrome again identical. |
| F3 | light, opaque, scale 8 | repeat F1 | Same motion under the light theme. |
| F4 | dark, opaque, 150% DPI, scale 8 | repeat F1 | The 3 px shift stays 3 *logical* px (4.5 physical at 150%): motion is scale-invariant, not a growing jump. |
| F5 | dark, opaque, normal speed (no scale), screen recording (Game Bar or OBS, manual) | `rust`, `{ENTER}`, `{ESC}`, `rust`, `{ENTER}` | A normal-speed clip for perceptual review: brief, subtle, no bounce; the clip is evidence for smoothness, not for exact pixel claims. |
| R1 | dark, opaque, scale 8 | `rust`, `{ENTER}`, `{ESC}`, `{ENTER}` with captures between each (no extra delay) | The rapid open/back/open retargets from the presentation on screen: the third capture continues from the interrupted offset instead of restarting from the floor — no flash, no replay. |
| T1 | dark, opaque, scale 8 | in the command, type `he` with captures immediately after each letter | Typing does not animate: rows change with no offset and no fade, on the same frames that carry the (still in-flight) arrival. |
| N1 | dark, opaque, scale 8, reduce motion | `rust`, `{ENTER}` with the same timed captures | Reduced motion: every capture is already settled — offset 0, full opacity — indistinguishable from a settled baseline capture. |
| C1 | dark, opaque, scale 8 | open the form ("Greet someone") and the color custom view | Forms and custom views arrive with the same policy; their controls' focus ring is visible from the first frame. |
| C2 | dark, opaque, scale 8 | open the ellipsis menu and Settings while a list arrival is in flight | The shell (footer strip, menu popup, Settings window) is unaffected: the popup's bounds and the footer do not shift with the content. |

## How to check the frames without eyes on the pixels

- **Chrome stationarity**: diff the footer strip's row of pixels (the
  `status-*` strip) and the search header between each F1/F2 capture and
  the pre-navigation capture: they must be identical (the same one-pixel
  rounding tolerance the smoke checker's `--same` mode allows).
- **Direction and distance**: locate the first result row's text color in
  consecutive F1 frames (the smoke checker's `--locate` prints the region
  center): its y advances toward rest monotonically from ~3 px below; in
  F2 it decreases from ~3 px above. No frame overshoots past rest (no
  bounce).
- **The fade**: in F1's early frames the row text pixels blend toward the
  panel background (the color is closer to the background than in the
  settled frame), converging to the settled color by the last frame.
- **Completion**: the final timed capture is pixel-identical to a capture
  taken with no transition pending (a settled baseline), and no further
  frame changes after it.
- **Immediacy** (T1): the narrowed rows are present in the frame taken
  immediately after the keystroke, while the arrival offset is still
  non-zero — input never waits for the transition.

## Frame and resource observations

- **No ambient frames after settle**: after F1's final capture, sample the
  pane process's CPU (`Get-Process pane`) for 10 s at 1 s intervals — a
  settled launcher must stay at ~0% (the resource workload on Linux,
  `scripts/measure-linux.sh`, re-checks the same class of idle on Xvfb).
- **Hidden mid-transition**: launch with scale 8, navigate, hide the window
  (helper extension or manual) for several seconds, then show it: the
  first visible frame is already settled (progress is measured on a clock,
  not in delivered frames).
- **Reduced motion from the OS itself** (manual, on a host where changing
  the real setting is acceptable): with the app running, toggle Windows'
  "Animation effects" off — the in-flight transition (scale 8) settles on
  the next frame; toggle it on — transitions resume. This is the one check
  that exercises the real `UISettings.AnimationsEnabledChanged` watch
  rather than `PANE_TEST_REDUCE_MOTION`.

## macOS and Linux

Deferred with the milestone's pass, as in #71/#72: the same F1/F2/T1/N1
matrix on macOS (AppKit) and Linux (Xvfb, `scripts/smoke-linux.sh`
patterns). On those platforms today the reduced-motion fallback is the
documented static full-motion default (no native read is wired); the
captures there verify the transitions themselves, not detection. Linux
Wayland limitation is unchanged from the smokes' documentation.

## Limitations, stated plainly

- Nothing in this file has run; it is a plan. Until the captures exist,
  perceptual smoothness is unproven and only behavioral guarantees are
  claimed, by the integration tests.
- Screenshot sampling at 8× proves the shape and discipline of the motion
  (direction, distance, monotonic settle, chrome stationarity, idle), not
  the perceived feel of the real 150 ms; the normal-speed clip covers
  perception but resists programmatic checking.
- Windows-first, as every native Pane evidence directory so far; macOS and
  Linux remain open native legs for the milestone.
