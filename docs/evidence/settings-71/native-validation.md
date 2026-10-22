# Native validation — ticket #71: the footer's selected action

Windows captures of the idle footer's new presentation, recorded with the
`capture-pane.ps1` helper in this directory (adapted unchanged in behavior
from the ui-prototype helper; the same safety rules apply — per-run scratch
data, spawn-scoped environment, focus confirmed by real foreground HWND
before any key, only the spawned PID closed). Every run's metadata sits
beside its PNGs as `<name>.pane-run.json` (theme, material, keys, focus
method, DPI, window bounds).

All captures are of a debug build (three sample commands in root search, so
a command is always selected) at 96 DPI on Windows. `focus: False` in the
no-key runs' metadata is the helper refusing to send keys while another
window held the foreground; those runs send no keys, so the initial capture
is unaffected. The two key-driven runs activated the window with the
helper's guarded click (`-ClickToFocus`, recorded as `guarded-click`).

## What the captures show

| File | Shows |
| --- | --- |
| `dark-glass-root.png` | Root search, dark theme, glass material (the default): the idle strip's right-aligned action button — "Open command" with the Enter keycap — and the far left of the strip empty, reserved for the app menu (#72). |
| `light-glass-root.png` | The same, light theme. |
| `dark-opaque-root.png` / `light-opaque-root.png` | The same state on the deterministic opaque material (dark and light), which the pixel checks below were run against. |
| `dark-opaque-command.png` | After Enter opens the Rust sample: the command's screen, the button now reading "Run item" — the label follows the action's identity, not the row's title. |
| `dark-opaque-form.png` / `dark-glass-form.png` | After Enter, four Downs and Enter open the sample's form ("Greet someone"): the button reading "Submit" beside the form's own submit control. |
| `dark-opaque-narrow-root.png` | A 380×420 window (outer, physical pixels): the button and its keycap still fit inside the strip at the narrow width. |

Each key-driven run sent `{ENTER}`, `{DOWN}` ×4, `{ENTER}` and captured
after every token; the curated files above are the first and last of those
frames (the full sequence is recorded in `keysRequested`).

## How this was checked without eyes on the pixels

The captures were verified programmatically against the theme's opaque
colors, not by visual inspection alone:

- In every capture, the footer's bottom strip contains the label's text
  color (#EDEDEF dark, #202126 light) only in its right half — 35–182
  pixels depending on label and material — and nothing but background in
  its left half: the button is right-aligned and the far-left space is
  empty.
- The old idle instruction text (#8E8F94, the muted hint role) is absent
  from the footer in all of them: the hints are gone, not merely moved.
- A horizontal profile through `dark-opaque-root.png`'s strip center reads
  the intended structure left to right: the footer wash (#131416) for
  ~598 px, the button's selected-row wash (#27282a) for ~142 px carrying
  the label glyphs, the keycap's tile background (#38393b) as a ~21 px box
  with the return-arrow glyph, then the strip's 16 px right padding.
- The exact geometry (button height 28 px, vertically centered in the
  50 px strip; keycap 20 px; truncating label) is asserted on the test
  platform in `crates/pane/tests/window.rs`
  (`the_footer_button_runs_the_selected_action_like_enter`), which draws
  the same element tree these captures show on the desktop.

Limitations, stated plainly: this is Windows-only evidence at one DPI; the
glass captures composite over whatever the desktop showed, so their exact
colors vary (the checks above ran on the opaque captures); and these are
presentation captures — the behavioral acceptance (pointer/key equivalence,
disabled states, long-status visibility) is covered by the window
integration tests, not by this file.
