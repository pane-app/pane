# Launcher appearance

[#64](https://github.com/pane-app/pane/issues/64) applies the
[retained authored reference and reviewed native proof](evidence/ui-prototype/README.md)
to the working launcher. The [presentation ownership](launcher-presentation.md)
from #62 is kept: the app supplies identities and behavior; shared UI owns
colors, type, geometry, row chrome, glyphs and surfaces.

## Choosing the appearance

Pane's appearance is the user's choice, made on the Settings window's
Appearance page ([#73](https://github.com/pane-app/pane/issues/73)):
the theme (system, light or dark) and the material (glass or solid), each
shown with a live preview and each taking effect in the launcher and the
Settings window the moment it is chosen — no restart. The choice is kept
in `settings.json` in Pane's data folder (versioned, validated, written
atomically, beside — never inside — the extensions' own records), a fresh
start reloads the last choice a save actually wrote, and a save that
fails is reported while the page falls back to what was saved. A record
that cannot be read is never replaced: its data stays for diagnosis.

The *system* theme follows the operating system's appearance, and the
windows re-render when the OS switches between light and dark — GPUI
delivers the platform's notification on Windows (the `ImmersiveColorSet`
change), macOS, X11 and Wayland. On macOS the platform's own titlebar and
window edges follow the theme as well: forced light or dark where the
preference forces one, the system's own where the preference follows it.

Development overrides remain, for driving a specific appearance through
a test or a smoke run:

```powershell
$env:PANE_THEME = 'dark'       # system, dark or light
$env:PANE_MATERIAL = 'opaque'  # glass or opaque
cargo run -p pane --locked
```

An explicit `PANE_THEME`/`PANE_MATERIAL` wins for that process: the
Settings window's Appearance page says it is in force, offers no choices
while it is (nothing chosen there applies), and never writes it back as
the saved preference. Unknown or missing values override nothing. The
product default — no record, no override — is the reference's
dark/glass.

The retained `PANE_MATERIAL=glass` path requests desktop composition on
Windows/macOS and normalizes to opaque on Linux; the Appearance page says
so under the material choice where it happens. On Linux that normalization
is the intentional opaque launcher, pinned by a policy test, with its
tiered CI evidence and the outstanding native X11/Wayland validation
recorded in [the validation report](launcher-ui-validation.md). On macOS the
request reaches the fork's under-window vibrancy material (see [docs/gpui-fork.md](gpui-fork.md));
no suppression check runs there, and the page's caveat that a glass request
is not proof of blur still applies. On Windows, the material's
construction reads the OS build, [transparency setting](https://learn.microsoft.com/en-us/uwp/api/windows.ui.viewmanagement.uisettings.advancedeffectsenabled)
and [high-contrast setting](https://learn.microsoft.com/en-us/uwp/api/windows.ui.viewmanagement.accessibilitysettings.highcontrast).
Builds below 17763, disabled transparency, enabled high contrast, or a
failed settings/version query select an opaque window and solid panel.
Nothing changes system settings. Opaque validation does not require
transparency to be enabled.

These checks reveal suppression preferences, not successful compositor
output. GPUI exposes no reliable visible-blur query. A driver/compositor
failure after the checks may leave the tint without blur; the page says a
glass request is not proof of blur, and choosing the solid surface gives
a deterministic panel everywhere. Native glass evidence and remaining
Windows conditions are recorded in [the validation report](launcher-ui-validation.md)
and [the Settings appearance validation plan](evidence/settings-73/native-validation.md).

## Visual behavior

The initial window is 760 by 460 logical pixels. Shared geometry provides
the panel radius, a 64-pixel query region, minimum 44-pixel rows,
28-pixel tiles and a minimum 50-pixel footer. The reference's 18-pixel
panel radius shows where the window's corners are transparent (Linux's
desktop shows through the curve) or platform-rounded (macOS's authored
curve stands over the window's vibrancy, which the fork's under-window
material now requests — see [docs/gpui-fork.md](gpui-fork.md); native macOS
material validation is outstanding in
[docs/launcher-ui-validation.md](launcher-ui-validation.md)); on Windows the panel
fills the window to its edges and the Desktop Window Manager rounds the
window's own corners (`pane::prefer_rounded_window_corners`), so neither
the acrylic frost nor the opaque surface shows as a plate behind the
corners. The actual query, command rows,
selection, invocation and extension views still run through the existing
launcher. No pinned content, invented application kinds or extra controls
are fabricated to fill the reference layout.

Geist is embedded with its [OFL license](../crates/pane/assets/fonts/OFL.txt).
Dark follows the supplied reference; light derives neutrals from the same
roles and geometry. Known stable command IDs map to the reference's icon
presentation; unknown commands use the neutral tile. The existing editable
input retains query/form editing, IME composition, focus and accessibility
wiring. The magnifier's surrounding chrome and non-search headings provide
drag regions; decorative surfaces register no input handlers.

Unavailable reasons wrap and remain visible and non-invokable. Host forms
use the theme while keeping validation, tab traversal and submission. Custom
extension drawing colors remain authored by the extension; only host focus
chrome takes the theme. Long status messages wrap, grow the footer to a
bounded fraction of the window, and scroll independently so either end is
reachable while results retain space.

The panel keeps a fine border and inset highlight. It deliberately omits
the prototype's original full-panel outer shadow stack, which obscured the
desktop. The retained glass starting tints are 70% dark and 80% light; those
are not universal contrast guarantees.

## Validation

The existing window and command-search integration suites exercise real
Rust, JavaScript and TypeScript fixtures. The carried long-footer test adds
380-by-420 and 640-by-200 layouts, horizontal containment, wrapping, bounded
growth and wheel access to both ends. These tests use GPUI's simulated
platform; they do not establish native IME or assistive-technology operation.

Windows checks on 2026-10-02:

- `cargo fmt -p pane --check`: pass.
- `cargo check -p pane --tests --locked -j 1`: pass, no warnings.
- `cargo test -p pane --test window --test command_search --locked -j 1`:
  **50 window + 1 command-search tests pass**, including the carried footer
  regression. [Raw results](evidence/ui-64/window-search-tests.log).
- Source audit: no core imports in shared UI; extension drawing colors
  remain unchanged; the original unavailable-reason element ID is retained.

The build uses the unchanged baseline renderer pin; the parent's integrated
checks combine these visuals with #63's maintained fork and #65's native
material policy. A copied executable is supplied to the parent for coordinated
opaque captures (SHA-256
`B4EEABBA65B91E29CB7F9D86783306787977280E9D4C0E2448EF51405639E053`).
Native capture results are recorded separately at that integration boundary.
Other platforms remain outstanding; prototype screenshots are historical
evidence, not new results.
