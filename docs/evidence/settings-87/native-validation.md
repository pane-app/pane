# Native validation — ticket #87: Settings section and group transitions

**Status: deferred — a plan, not results.** The host machine that would run
the captures is in interactive use (the user games on it), so no native run
was performed for this ticket; native/visual validation is #84's pass for
the whole milestone. This file records exactly what to capture when a host
is available, so that pass can execute it without re-deriving the design.
Source constants alone do not prove smoothness; until these captures
exist, the only completed evidence for #87 is the test platform's
controlled-clock integration tests — the section-transition tests in
`crates/pane/tests/settings.rs` (shell stationarity, both directions,
immediacy of the selected-section state, rapid-switch retargeting,
reduced motion at the window's floor) and the disclosure tests in
`crates/pane/tests/shortcuts.rs` (one coordinated block, interactive from
the first frame, reversal retargeting, focus leaving a collapsing group,
retained page state, reduced motion) — run by CI on Linux, Windows and
macOS at the `[verify]` tier.

Everything below uses a **debug build** of the pinned working tree (the
renderer pin `bcf3a0acd047c1873293069d0ed42085a38f699b` is recorded in
`crates/pane/Cargo.toml`), because the two capture hooks are debug-only by
design (see `crates/pane/src/ui/motion.rs`):

- `PANE_MOTION_SCALE=<factor>` — stretches **every** transition timeline
  by that factor (clamped 0.25–16), now including the Settings section
  arrival (150 ms) and the disclosure span (180 ms), because all three go
  through the same tween. At 8×, a section arrival becomes 1.2 s and a
  disclosure 1.44 s, so a screenshot taken a known delay after a click
  samples a definite point of the curve instead of chasing a fast blur. A
  release build ignores it entirely.
- `PANE_TEST_REDUCE_MOTION=1` — runs with reduced motion engaged and skips
  the native preference read and watch, so the reduced presentation can be
  captured without touching the operator's own system settings. A release
  build ignores it entirely.

## Helper change needed first

The same one #86's plan calls for: a copy of
`docs/evidence/settings-71/capture-pane.ps1` (the established, safe launch
helper) extended with `-MotionScale <float>`, `-ReduceMotion <switch>` and
a small `-KeyDelayMs` that can sample mid-arrival frames — one helper
shared by the motion tickets, if #86's pass has not already written it.
All the existing safety rules carry over unchanged: per-run scratch
`PANE_DATA_DIR` and scratch `LOCALAPPDATA`, `PANE_ARTIFACTS` cleared for
the child, foreground HWND confirmed before every key token, only the
spawned PID closed, no deletion anywhere. #87 adds one need the launcher
tickets did not have: the helper must be able to open **Settings** and
click a **sidebar section** and a **group header**, which the established
key-token approach does by pixel coordinate — the sections and headers
are the topmost left column and the first rows of the Shortcuts page, so
fixed offsets from the window's origin are stable enough for a scripted
click; record the coordinates in `pane-run.json` alongside OS, DPI, window
bounds and the binary's commit.

## Captures to take (Windows first — the available host)

All with the sample extensions the helper installs (root search lists
"Rust sample"), plus the Shortcuts page's groups over them. Pixel checks
run on the **opaque** material (deterministic colors, as in #71); one
glass run per theme is for perceptual review only.

| ID | Setup | Keys / actions | What the frames must show |
| --- | --- | --- | --- |
| S1 | dark, opaque, scale 8, delay ≈ 100 ms | open Settings (Ctrl+,), click **Shortcuts** in the sidebar, captures at ~100, 400, 700, 1000, 1300 ms after the click | The page's content starts displaced ~3 logical px *below* rest and faint (~30% opacity floor) and settles over ~1.2 s; the sidebar (including the row that was clicked and the newly selected row), the titlebar and the window bounds stay pixel-identical throughout (compare against the pre-click capture). |
| S2 | dark, opaque, scale 8 | from Shortcuts, click **Appearance** (moving up), same timed captures | The paired arrival: the page's content starts ~3 px *above* rest and settles over the same 1.2 s span — both directions share it, unlike the launcher's faster return. |
| S3 | light, opaque, scale 8 | repeat S1 | Same motion under the light theme. |
| S4 | dark, opaque, 150% DPI, scale 8 | repeat S1 | The 3 px shift stays 3 *logical* px (4.5 physical at 150%): motion is scale-invariant, not a growing jump. |
| S5 | dark, opaque, scale 8 | click Shortcuts, then immediately click Extensions, then Shortcuts again (no extra delay) with captures between each | Rapid section switches retarget from the presentation on screen: the third capture continues from the interrupted offset instead of restarting — no flash, no replay — and the page drawn is always the section the user is on (no fading-out predecessor lingers). |
| S6 | dark, opaque, scale 8, window at the 560×400 floor, narrow | repeat S1 | The transition at the layout floor: the pages still switch and arrive, the scroll viewport does not resize, nothing clips that did not already. |
| G1 | dark, opaque, scale 8, delay ≈ 100 ms | Shortcuts page, click a collapsed group's header, captures at ~100, 500, 900, 1200, 1500 ms | One coordinated block: the group's rows mount at once (the page's real height — the scroll range — is the expanded one from the first frame) and arrive together as one block over the tiny shift and fade, while the header's chevron turns from pointing right to pointing down **on the same timeline** — no staggered row arrivals, no chevron snapping ahead of or behind the rows. |
| G2 | dark, opaque, scale 8 | click an expanded group's header, same timed captures | The collapse: the rows unmount at once (never drawn fading out) while the chevron turns back on the disclosure's timeline; the content above the group never moves. |
| G3 | dark, opaque, scale 8 | expand a group, then click its header again mid-arrival, captures between | The reversal retargets from the look on screen: the chevron continues from where it had turned, the rows unmount at once — no restart, no flash. |
| G4 | dark, opaque, scale 8 | expand a group and click a row's alias cell mid-arrival, type | The arriving commands are interactive from the first frame: the editor opens and takes the typing while the arrival is still in flight. |
| F1 | dark, opaque, scale 8 | open the inline editor in a group (click an alias cell), type an uncommitted edit, then click the group's header to collapse | Focus leaves the collapsing group for the controlling header: the editor closes without committing (the alias is unchanged, the record untouched), the hidden rows expose no hit targets — a click where the rows were does nothing — and Enter on the header re-expands the group. |
| N1 | dark, opaque, scale 8, reduce motion | repeat S1 and G1 with the same timed captures | Reduced motion: every capture is already settled — no offset, no fade — indistinguishable from a settled baseline capture. |
| N2 | dark, opaque, normal speed (no scale), screen recording (Game Bar or OBS, manual) | switch sections and expand/collapse groups | A normal-speed clip for perceptual review: brief, subtle, coordinated; the clip is evidence for smoothness, not for exact pixel claims. |

## How to check the frames without eyes on the pixels

- **Shell stationarity** (S1–S6): diff the sidebar column and the titlebar
  strip between each capture and the pre-click capture: identical (the
  same one-pixel rounding tolerance the smoke checker's `--same` mode
  allows). The newly selected sidebar row's highlight change is the one
  allowed difference in that column — it flips on the frame the click
  lands, before the arrival progresses.
- **Direction and distance**: locate the page's heading text in
  consecutive S1 frames (the smoke checker's `--locate` prints the region
  center): its y advances toward rest monotonically from ~3 px below; in
  S2 it decreases from ~3 px above. No frame overshoots rest.
- **Coordination** (G1): the chevron's pixel region rotates through
  quarter-turn steps while the row block's text pixels rise together —
  sample the row block's top row and the chevron region center in the
  same frames; neither reaches its endpoint before the other's last
  captured step.
- **Layout is real from the first frame** (G1): the page's scroll thumb
  (or, at the floor, the last row's presence) reflects the expanded height
  on the very first post-click capture — the arrival is paint only.
- **No ambient frames after settle**: after each capture series, sample
  the pane process's CPU (`Get-Process pane`) for 10 s at 1 s intervals —
  a settled window must stay at ~0%.
- **Reduced motion from the OS itself** (manual, where changing the real
  setting is acceptable): with the app running, toggle Windows' "Animation
  effects" off — the in-flight transition (scale 8) settles on the next
  frame; toggle it on — transitions resume. This exercises the real
  `UISettings.AnimationsEnabledChanged` watch rather than
  `PANE_TEST_REDUCE_MOTION`.

## macOS and Linux

Deferred with the milestone's pass, as in #71/#72/#86: the same S1/S2/G1/N1
matrix on macOS (AppKit) and Linux (Xvfb, `scripts/smoke-linux.sh`
patterns). On those platforms today the reduced-motion fallback is the
documented static full-motion default (no native read is wired); the
captures there verify the transitions themselves, not detection.

## Limitations, stated plainly

- Nothing in this file has run; it is a plan. Until the captures exist,
  perceptual smoothness is unproven and only behavioral guarantees are
  claimed, by the integration tests.
- Screenshot sampling at 8× proves the shape and discipline of the motion
  (direction, distance, monotonic settle, shell stationarity, layout
  truth, idle), not the perceived feel of the real 150/180 ms; the
  normal-speed clip covers perception but resists programmatic checking.
- The chevron–row coordination check samples two regions at capture
  cadence, which bounds (not proves) simultaneity; the stronger guarantee
  is structural — both derive from the one disclosure value, in code and
  in the integration tests.
- Windows-first, as every native Pane evidence directory so far; macOS and
  Linux remain open native legs for the milestone.
