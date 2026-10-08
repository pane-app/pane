# Launcher UI implementation and validation

Work follows [specification #61](https://github.com/pane-app/pane/issues/61).
Application baseline: `748d71e`. Implementation branch: `feat/launcher-ui-spec-61`.

## Integration order

| Ticket | Scope | Prerequisites |
| --- | --- | --- |
| [#62](https://github.com/pane-app/pane/issues/62) | Preserve prototype evidence and separate presentation ownership | None |
| [#63](https://github.com/pane-app/pane/issues/63) | Reproducible GPUI CE fork and Windows alpha regression | None |
| [#64](https://github.com/pane-app/pane/issues/64) | Shared dark/light visuals and responsive layouts | #62 |
| [#65](https://github.com/pane-app/pane/issues/65) | Windows native material and validation | #63, #64 |
| [#66](https://github.com/pane-app/pane/issues/66) | macOS native material and validation | #63, #64 |
| [#67](https://github.com/pane-app/pane/issues/67) | Linux opaque appearance and validation | #63, #64 |
| [#68](https://github.com/pane-app/pane/issues/68) | Combined revision validation | #65, #66, #67 |

## Evidence rules

Application behavior tests, renderer output regressions, and native compositor
captures establish different properties. A headless test or successful build
does not establish desktop blur, native input-method operation, or assistive
technology support. A background luminance response alone proves transparency,
not blur; compositor evidence must include softened external edges.

Record the tested application and fork revisions, OS/build, display backend,
scaling, requested appearance, relevant transparency settings and foreground
conditions with each native result. Preserve prototype results separately from
production validation. Do not alter global transparency settings automatically.

## Environment availability

At implementation start on 2026-10-02, the connected development device is
Windows. No macOS or Linux desktop is connected; WSL is not installed. Existing
CI includes Windows, macOS and Linux jobs, but CI build/test results do not replace
the required native material and interaction evidence. Unavailable cases remain
not run and keep final validation open.

The user subsequently scoped execution to Windows first, with a final CI run on
every OS after integration. Native macOS/Linux validation is deferred; it is not
a prerequisite for completing this Windows-focused implementation pass.

## Current result

Implementation and validation are in progress.

### Presentation prefactor (#62)

Integrated as `038a7a8`, with implementation `1254dc6` and preserved prototype
evidence `43aec59`. See [presentation ownership](launcher-presentation.md) and
[prototype recovery and provenance](evidence/ui-prototype/README.md).

- Windows: 49 launcher-window tests and one command-search test pass; checking
  test targets and formatting also pass. The first parallel full test build
  exhausted host memory; scoped serial builds succeeded.
- Native Windows at 96 DPI: startup, typing `rust`, Enter to open its command,
  Enter to invoke Say hello, and Escape passed. The capture shows
  [the Rust guest's answer](evidence/ui-windows/refactor/action.png).
  [Capture metadata](evidence/ui-windows/refactor/pane-run.json) records OS,
  binary SHA256, confirmed foreground input and cleanup of the spawned process.
- The metadata names `b0793c2`, the pre-amend implementation commit whose binary
  was captured. The final `1254dc6` amendment only normalized a saved evidence
  log's final newline; the application code and binary are the same.
- This check used the original appearance before styling. Its theme/material
  environment values were ignored by that build and are not material evidence.

### Shared appearance (#64)

Integrated as `5ca1f06`, implementation `70fd205`. The Windows application suites
pass 50 window tests plus one command-search test. The additional footer test
covers 380x420 and 640x200 layouts, wrapping, bounded height and both scroll
directions. Formatting and checking test targets pass.

Native Windows 11 25H2 build 26200.8737 at 96 DPI, opaque mode, binary SHA256
`B4EEABBA65B91E29CB7F9D86783306787977280E9D4C0E2448EF51405639E053`:

- [Dark root](evidence/ui-windows/opaque-dark/00-initial-window.png), query,
  command invocation and Escape pass. Guarded native header dragging moves
  the window; narrow resize, restore, deactivation and reactivation pass.
  [Run metadata](evidence/ui-windows/opaque-dark/pane-run.json).
- [Light root](evidence/ui-windows/opaque-light-form/00-initial-window.png),
  form editing, [validation](evidence/ui-windows/opaque-light-form/04-after-TAB-ENTER-window.png)
  and [successful submission](evidence/ui-windows/opaque-light-form/06-after-TAB-ENTER-window.png)
  pass. Choice glyphs, focus caret and error/success text remain readable.
  [Run metadata](evidence/ui-windows/opaque-light-form/pane-run.json).
- At 380x420 outer physical pixels, the
  [error wraps](evidence/ui-windows/opaque-narrow/03-after-DOWN-2-ENTER-window.png)
  and the [selected unavailable row](evidence/ui-windows/opaque-narrow/04-after-DOWN-5-window.png)
  retains its full explanation inside the viewport.
  [Run metadata](evidence/ui-windows/opaque-narrow/pane-run.json).

These captures were visually reviewed and their exact binary hash, target
foreground, un-aborted key input and process cleanup checked. Two earlier
attempts were covered by an existing prototype window; the helper sent no keys,
and neither their images nor their input attempts count as evidence. Positioning
only the spawned window near the display's upper-left corner enabled a guarded
focus click. No original prototype window was closed or moved.

These #64 captures use the old renderer pin in opaque mode. Final glass evidence
must use the combined fork and application revision. Native IME, screen-reader
operation, scaled displays and OS transparency-disabled operation remain not run.

### Combined Windows material (#65)

Application source `9c6bfa3` (production code from `e3a3d8a`), fork
`2b9e644e3f89a38eacebc713fb0d1807c618c76c`, binary SHA256
`9E2FABAEBBB9DABAD1A87E898C358AAAD8EF4154FB7D2E30091CEE71F731DBD1`:

The revision identifiers in capture metadata precede the required commit
sign-off normalization. The durable tag `evidence/launcher-ui-windows-20261002`
points to `9df21872b7bcf1e85421bb05ea3e9088a7a5e019`, whose source tree is exactly
the captured revision's `07341bfee05b8549488664cfcbcd9029fd177cc3`. Only commit
messages/parent identifiers changed. The later review fix centralizes smoke
color names and removes a redundant local binding when returning a result row;
it changes no rendered behavior. Final Clippy (`--all-targets -- -D warnings`)
passes. The all-OS CI result is attached to [PR #69](https://github.com/pane-app/pane/pull/69).

- `cargo build --locked -j1 -p pane` passes. The combined application passes
  50 window tests and one command-search test, including the footer regression.
- Windows 11 25H2, build 26200.8737, 96 DPI, transparency enabled. Every retained
  capture confirms the spawned window's foreground ownership, the diagnostic
  backdrop immediately behind it when used, no aborted keys, and cleanup of the
  spawned process. Original prototype windows were retained.
- [Dark glass](evidence/ui-windows/combined/glass-dark-lightbackdrop/00-initial-window.png)
  and [light glass](evidence/ui-windows/combined/glass-light-lightbackdrop/00-initial-window.png)
  visibly soften the known sharp external squares/bands. The diagnostic image
  is painted by a separate helper-owned native window, never inside Pane.
  Sharp pattern sources are saved with the
  [light form](evidence/ui-windows/combined/glass-light-form/external-pattern.png)
  and [narrow run](evidence/ui-windows/combined/glass-narrow/external-pattern.png).
- Comparing bright/dark backdrops in the empty right-hand list region
  `(630,120)-(730,300)` yields mean absolute RGB differences of **13.818944**
  for dark glass and **16.858611** for light glass. Both opaque controls yield
  **0**. These numbers establish background response, not blur by themselves;
  the reviewed softened edges supply the blur evidence. They are host-specific,
  not pass thresholds. [Measurements](evidence/ui-windows/combined/backdrop-response.json)
  can be reproduced with `python docs/evidence/ui-windows/combined/measure-response.py`
  (Pillow required).
- Dark glass retains query, command/action invocation, Escape, actual guarded
  header dragging, narrow resize/restore, and deactivation/reactivation.
  [Metadata and window checks](evidence/ui-windows/combined/glass-dark-lightbackdrop/pane-run.json).
- Light glass retains editable form focus, validation and
  [successful submission](evidence/ui-windows/combined/glass-light-form/06-after-TAB-ENTER-window.png).
  The [380x420 glass view](evidence/ui-windows/combined/glass-narrow/04-after-DOWN-5-window.png)
  keeps unavailable explanations and wrapped error text readable.
- The integrated dark/opaque [package preview](evidence/ui-windows/combined/integrated-preview/00-initial-window.png)
  passes the updated smoke checker (106 metadata pixels), validating its Windows
  sampling region against the real UI rather than a synthetic fixture alone.

Visual review found no new clipped controls or obscuring panel layer. The dark
muted subtitles/footer have the weakest contrast over brighter patches but were
decipherable in these captures; this is not a universal contrast guarantee.
The 70% dark/80% light tints, fine border and inner highlight remain. Removing
full-panel outer shadows was independently necessary in the prototype; the
source-over correction alone is not credited with establishing useful glass.

Startup chooses an opaque window and solid panel when Windows reports disabled
transparency, high contrast, an unsupported build, or cannot answer the required
queries. Those suppressed configurations were not exercised natively here, and
no system setting was changed. Standard 96-DPI display coverage only; scaled
displays, native IME and screen-reader operation remain not run. Native macOS
and Linux validation remains deferred under the user's Windows-first scope.

## Repeat the Windows checks

Build the pinned application, then run a fresh process for each theme/material:

```powershell
cargo build --locked -j1 -p pane
./scripts/capture-pane-windows.ps1 -Binary ./target/debug/pane.exe `
  -ApplicationRevision (git rev-parse HEAD) -OutputDir ./.scratch/ui-capture `
  -Theme dark -Material glass -Backdrop -BackdropPattern light `
  -PositionTopLeft -ClickToFocus -ExerciseWindow -Keys @('rust','{ENTER}','{ENTER}','{ESC}')
```

Repeat `light`/`dark` themes, both backdrop patterns, and `opaque` controls.
Use `-WindowWidthPixels 380 -WindowHeightPixels 420` for the narrow case.
The helper's click/drag options use real pointer input only inside its verified
spawned window; do not move the pointer during those checks. It records binary
hash, requested appearance, OS/settings/DPI, focus and cleanup in `pane-run.json`.
Do not count a failed-focus or covered-window capture as visual/input evidence.
