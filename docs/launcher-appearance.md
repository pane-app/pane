# Launcher appearance

This page covers how Pane's launcher looks: the theme and material the
user picks, how the glass material behaves on each system, and the
window's geometry and visual rules. It ends with how the look was
validated.

The look comes from [#64](https://github.com/pane-app/pane/issues/64),
which applies the
[retained authored reference and reviewed native proof](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/evidence/ui-prototype/README.md)
to the working launcher. It keeps the
[presentation ownership](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/launcher-presentation.md)
from #62: the app supplies identities and behavior, and shared UI owns
colors, type, geometry, row chrome, glyphs and surfaces.

## Choosing the appearance

The user chooses Pane's appearance on the Settings window's Appearance
page ([#73](https://github.com/pane-app/pane/issues/73)). There are two
choices:

- the theme: system, light or dark;
- the material: glass or solid.

Each shows a live preview. Each takes effect in the launcher and the
Settings window the moment it is chosen, with no restart.

### Where the choice is kept

The choice is kept in `settings.json` in Pane's data folder. The file is
versioned, validated and written atomically, and it sits beside the
extensions' own records, never inside them.

A fresh start reloads the last choice a save actually wrote. A save that
fails is reported, and the page falls back to what was saved. A record
that cannot be read is never replaced: its data stays for diagnosis.

### The system theme

The *system* theme follows the operating system's appearance. The
windows re-render when the OS switches between light and dark: GPUI
delivers the platform's notification on Windows (the `ImmersiveColorSet`
change), macOS, X11 and Wayland.

On macOS the platform's own titlebar and window edges follow the theme
as well. They are forced light or dark where the preference forces one,
and the system's own where the preference follows it.

### Development overrides

Development overrides remain, for driving a specific appearance through
a test or a smoke run:

```powershell
$env:PANE_THEME = 'dark'       # system, dark or light
$env:PANE_MATERIAL = 'opaque'  # glass or opaque
cargo run -p pane --locked
```

An explicit `PANE_THEME`/`PANE_MATERIAL` wins for that process. The
Settings window's Appearance page says it is in force, offers no choices
while it is (nothing chosen there applies), and never writes it back as
the saved preference. Unknown or missing values override nothing.

The product default, with no record and no override, is the reference's
dark/glass.

### Glass on each system

The retained `PANE_MATERIAL=glass` path requests desktop composition on
Windows/macOS and normalizes to opaque on Linux. Where that happens, the
Appearance page says so under the material choice.

On Linux, that normalization is the intentional opaque launcher, pinned
by a policy test. Its tiered CI evidence and the outstanding native
X11/Wayland validation are recorded in
[the validation report](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/launcher-ui-validation.md).

On macOS, the request reaches the fork's under-window vibrancy material
(see [docs/gpui-fork.md](gpui-fork.md)). No suppression check runs there,
and the page's caveat that a glass request is not proof of blur still
applies.

On Windows, the material's construction reads the OS build, the
[transparency setting](https://learn.microsoft.com/en-us/uwp/api/windows.ui.viewmanagement.uisettings.advancedeffectsenabled)
and the
[high-contrast setting](https://learn.microsoft.com/en-us/uwp/api/windows.ui.viewmanagement.accessibilitysettings.highcontrast).
Pane selects an opaque window and solid panel for any of these:

- a build below 17763;
- disabled transparency;
- enabled high contrast;
- a failed settings/version query.

Nothing changes system settings. Opaque validation does not require
transparency to be enabled.

### What the checks cannot tell

These checks reveal suppression preferences. They cannot show successful
compositor output, because GPUI exposes no reliable visible-blur query. A
driver/compositor failure after the checks may leave the tint without
blur. The page says a glass request is not proof of blur, and choosing
the solid surface gives a deterministic panel everywhere.

Native glass evidence and remaining Windows conditions are recorded in
[the validation report](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/launcher-ui-validation.md)
and
[the Settings appearance validation plan](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/evidence/settings-73/native-validation.md).

## Visual behavior

### Geometry

The initial window is 760 by 460 logical pixels. Shared geometry provides:

- the panel radius;
- a 64-pixel query region;
- rows of at least 44 pixels;
- 28-pixel tiles;
- a footer of at least 50 pixels.

### Corners

The reference's 18-pixel panel radius shows differently on each system:

- On Linux the window's corners are transparent, so the desktop shows
  through the curve.
- On macOS the corners are platform-rounded. The authored curve stands
  over the window's vibrancy, which the fork's under-window material now
  requests (see [docs/gpui-fork.md](gpui-fork.md)). Native macOS material
  validation is outstanding in
  [the archived launcher UI validation](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/launcher-ui-validation.md).
- On Windows the panel fills the window to its edges, and the Desktop
  Window Manager rounds the window's own corners
  (`pane::prefer_rounded_window_corners`). Neither the acrylic frost nor
  the opaque surface shows as a plate behind the corners.

### Taskbar, Dock and window switchers

The launcher window is never on the taskbar or in the window switcher, as
Raycast's is not ([#458](https://github.com/pane-app/pane/issues/458)).
Pane is reached by its hotkey and its tray or menu-bar entry. There is no
setting for it. The Settings window is an ordinary window and keeps its
taskbar button. HUDs are pop-ups, which no system lists.

- On Windows the launcher is a tool window (`WS_EX_TOOLWINDOW`, without
  `WS_EX_APPWINDOW`): no taskbar button and no Alt+Tab entry. It opens
  hidden and gets the style before its first show, since Windows decides
  on the button as a window is shown (`pane::open_launcher_window`). A
  test reads the style back on CI's Windows runner, first shown, after a
  dismiss and after the tray summons it again.
- On macOS Pane is an accessory application: no Dock icon, no Cmd+Tab
  entry and no menu bar of its own. `main.rs` sets the activation policy
  before launching finishes, not `LSUIElement` in the bundle's
  Info.plist, so a development build behaves as the installed app does.
  Settings brings Pane to the front as it opens.
- On Linux under X11 the launcher asks the window manager to skip the
  taskbar and the pager (EWMH's `_NET_WM_STATE_SKIP_TASKBAR` and
  `_NET_WM_STATE_SKIP_PAGER`), again before every show, since a window
  manager may forget the state of a window it withdrew. A window manager
  without EWMH ignores it.
- On Wayland there is no protocol for it: whether the launcher is listed
  is up to the compositor.

### What the launcher shows

The actual query, command rows, selection, invocation and extension views
still run through the existing launcher. Nothing is fabricated to fill
the reference layout: no pinned content, invented application kinds or
extra controls.

### Type, icons and input

Geist is embedded with its [OFL license](../crates/pane/assets/fonts/OFL.txt).
Dark follows the supplied reference. Light derives neutrals from the same
roles and geometry.

Known stable command IDs map to the reference's icon presentation;
unknown commands use the neutral tile.

The existing editable input keeps query/form editing, IME composition,
focus and accessibility wiring. The magnifier's surrounding chrome and
non-search headings provide drag regions. Decorative surfaces register
no input handlers.

On a pushed view (a command's search, a designed List such as Switch
Windows, the update results) the search bar shows a back button in the
magnifier's place, as Raycast's does (#469). It is the split views' back
button (Clipboard History's, Search Files'): a 32px rounded square with
the left arrow, its fill washing on hover, centred in the magnifier's
44px hit target so the field's text does not move. Its tooltip names the
back key ("Back (Escape)"). A click goes back one level, whatever the
field holds, and the keyboard stays in the search field. Root search
shows the magnifier.

A pushed view with no search bar (a designed view whose tree names no
List or Grid: a form, a detail, a canvas; and the forms Pane itself
asks, such as the alias form) has a top bar in its place, as Raycast's
does (#482). The top bar is the search bar's row: the same 64px, rule
(or frosted pill) and loading bar, the same back button in the same
place, and the view's navigation title in the row title's 14/500 where
the query's text starts (the open command's title when the tree names
none). Nothing in the row moves between a search view and a top-bar
view, and the content starts at the same height. The title is the row's
drag region. The bar takes no focus: the keyboard stays where the view
put it, a form's first field say. The core's own screens (a package's
preview, a confirmation, the details screens) keep their heading.

### Status lines, forms and extension drawing

Unavailable reasons wrap and stay visible and non-invokable. Host forms
use the theme while keeping validation, tab traversal and submission.

Custom extension drawing colors stay as the extension authored them;
only host focus chrome takes the theme.

Long status messages wrap and grow the footer to a bounded fraction of
the window. They scroll independently, so either end is reachable while
results keep their space.

### The panel

The panel keeps a fine border and inset highlight. It deliberately omits
the prototype's original full-panel outer shadow stack, which obscured
the desktop.

The retained glass starting tints are 70% dark and 80% light. They are
not universal contrast guarantees.

## Validation

The existing window and command-search integration suites exercise real
Rust, JavaScript and TypeScript fixtures. The carried long-footer test
adds 380-by-420 and 640-by-200 layouts, horizontal containment, wrapping,
bounded growth and wheel access to both ends.

These tests use GPUI's simulated platform. They do not establish native
IME or assistive-technology operation.

### Windows checks on 2026-10-02

- `cargo fmt -p pane --check`: pass.
- `cargo check -p pane --tests --locked -j 1`: pass, no warnings.
- `cargo test -p pane --test window --test command_search --locked -j 1`:
  50 window + 1 command-search tests pass, including the carried footer
  regression.
  [Raw results](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/evidence/ui-64/window-search-tests.log).
- Source audit: no core imports in shared UI; extension drawing colors
  remain unchanged; the original unavailable-reason element ID is
  retained.

### Integration and other platforms

The build uses the unchanged baseline renderer pin. The parent's
integrated checks combine these visuals with #63's maintained fork and
#65's native material policy.

A copied executable is supplied to the parent for coordinated opaque
captures (SHA-256
`B4EEABBA65B91E29CB7F9D86783306787977280E9D4C0E2448EF51405639E053`).
Native capture results are recorded separately at that integration
boundary.

Other platforms remain outstanding. The prototype screenshots are
historical evidence, not new results.
