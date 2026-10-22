# Native validation — Settings appearance (#73)

**Status: plan only.** The host that recorded the earlier native evidence
([`settings-72`](../settings-72/native-validation.md)) was unavailable for
this ticket, so no native captures were made: everything below is the
capture plan the next native run on this branch should follow, and this
file must say "passed/failed, evidence" per row once that run happens.
The automated window-harness results that *were* run are on CI (this
branch's Check legs); they exercise the Appearance page through GPUI's
test platform, which is not native IME, assistive technology, or
compositor behavior.

## What this ticket ships

The Appearance page of the Settings window ([#73](https://github.com/hoangvu12/pane/issues/73)):
system/light/dark theme and glass/solid material, a live preview, one
observable host-settings entity both windows follow, and the
versioned/validated/atomic `settings.json` record in Pane's data folder.
The record's own rules (missing fields default, unknown values fail,
unreadable records are never replaced) are unit-tested in
`pane-core`/`pane`; the window-level behavior (change-to-render,
two-window synchronization, restart, save failure, corrupt record,
overrides) is tested in `crates/pane/tests/settings.rs` through real
keystrokes, clicks, the accessibility tree and the painted quads.

## What the harness cannot observe (why native evidence is required)

- **The OS appearance-change notification.** A theme set to *system* must
  re-render both windows when the operating system switches between light
  and dark. GPUI CE wires `Window::observe_window_appearance` to the real
  platform notifications (Windows `WM_SETTINGCHANGE`/`ImmersiveColorSet`,
  macOS appearance changes, X11 and Wayland), and Pane registers it for
  both windows — but the test platform's appearance simulation
  (`TestWindow::simulate_appearance_change`) is crate-private in the
  pinned revision, so the in-window tests cover the choice's
  change-to-render and the entity's re-resolution, not the platform
  notification itself. A native run must flip the OS appearance with both
  windows open and record both repainting.
- **Compositor blur.** A glass request is not proof of visible blur; GPUI
  exposes no query for it. Native captures must show what the glass
  request actually produced behind the window on each platform.
- **The macOS window chrome following the theme** (the
  `NSApplication.appearance` call): the platform's own titlebar and edges
  must match a forced light/dark choice and follow the system again when
  the preference is *system*.
- **Perceived readability of both palettes on both materials** at normal
  and scaled sizes: dark/light text on the tint and the solid panel, over
  a bright and a dark desktop. The theme's contrast-honesty note in
  `ui/theme.rs` is a stated limitation, not a certified ratio.

## Capture plan (Windows first, as before)

Modeled on the established
[`settings-72/capture-settings.ps1`](../settings-72/) pattern: scratch
`PANE_DATA_DIR`, spawn-scoped environment, real focus confirmation,
guarded clicks, close only the spawned process. Rows:

| Check | Evidence to capture |
| --- | --- |
| The Appearance page lists System/Light/Dark and Glass/Solid, with the current choice marked and the preview drawn | screenshot of the page as opened |
| Choosing Light repaints the launcher window and the Settings window (both visible side by side) without a restart | before/after screenshots |
| Choosing Solid changes the panel from tint to solid; choosing Glass back | screenshots; on Linux the page's "Glass is unavailable here" note instead |
| The *system* choice follows the OS: flip Windows' light/dark in Settings with both windows open | screenshots of both windows before/after the OS flip |
| A restart (close Pane, start again over the same data folder) reloads the last saved choice | screenshot of the reopened window |
| A read-only or sabotaged `settings.json` shows the page's status and leaves the file untouched | screenshot + the file's content before/after |
| `PANE_THEME=light` makes both windows render light with the override notice on the page, and the record is not created or changed | screenshot + directory listing |
| The macOS-specific chrome-following and traffic-light coexistence | outstanding (no macOS host yet, as for every earlier milestone) |
| Linux opaque normalization and the note's wording | outstanding; X11 evidence does not prove Wayland |

## Platform material limitations (documented, not invented)

- **Linux:** GPUI CE exposes no compositor frost Pane can request, so a
  glass preference normalizes to the opaque window and solid panel, and
  the page says so (`ui::material::glass_fallback_reason`). This is the
  standing policy from the earlier milestone, unchanged.
- **Windows:** glass is suppressed on builds below 17763, when the OS's
  transparency preference is off, or when high contrast is on; the page
  names the possibility. These are suppression preferences, not compositor
  success: the acrylic request may still fail after them.
- **macOS:** vibrancy is requested; the same not-proof-of-blur caveat
  applies.
- **All:** no blur slider, tint slider, accent or density controls exist;
  the Appearance page is exactly theme + material + preview, per the
  specification's bounded scope.
