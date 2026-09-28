# macOS native baseline (#7)

Recorded 2026-09-28 from GitHub Actions run
[36366760796](https://github.com/wasimysaid/pane/actions/runs/36366760796) on the
fork `wasimysaid/pane`, commit `572d629`.

## Tested combination

| | |
| --- | --- |
| OS | macOS 15.7.9 (build 24G830), from the smoke's `sw_vers` |
| Architecture | arm64 (Apple silicon) |
| Machine | GitHub-hosted runner, image `macos-15-arm64` version 20260907.0337.1 |
| Session | the runner's logged-in GUI session: WindowServer, Finder, Dock, real display surface |
| Rust | 1.98.1 (`rust-toolchain.toml`) |

**Not tested, so not claimed:** Intel (x86_64) Macs, macOS 14 or earlier and 26
or later, a physical Mac with a real display, Retina scaling other than the
runner's, and any signed or notarized build.

## Fresh checkout build and checks

The CI job checks out the repository and runs only the documented commands (the
README's macOS prerequisite is the Xcode Command Line Tools, preinstalled on the
runner):

```sh
rustup toolchain install
cargo xtask ci          # guests, prebuilt JS/TS check, fmt, clippy, all tests
cargo build --locked -p pane
```

Result: `cargo xtask ci` passed (13 window, 9 launcher-model, 1 runtime-cache
and 25 sample-contract tests), and the launcher built. No Windows machine or
PowerShell was involved. The Rust guests are built natively; the JS and TS
samples use the committed prebuilt components. Rebuilding those from source on
macOS (`cargo xtask js-guests`) has **not** been run. The Cargo cache
(`Swatinem/rust-cache`) was warm, so this is a fresh checkout but not a cold
`target/`.

## Native GUI smoke

```sh
cargo build -p pane
scripts/smoke-macos.sh smoke            # needs Python 3 with Pillow
```

The script launches `target/debug/pane`, brings it to the front and sends real
key events through System Events (`osascript`), which needs the Accessibility
permission for the calling terminal (GitHub's macOS runners grant it). For each
of the Rust, JavaScript and TypeScript sample commands it presses Return to open
it, Down and Return to run "Wait briefly" (an async WASI 0.3 clock import inside
the guest), then Escape.

It fails if Pane exits. Three checks also run against the `screencapture`
screenshots (`scripts/check_screenshot.py`):

- The root screen draws text in Pane's hint color.
- Each result screen draws text in the result color.
- The three result screens are all different. This catches a smoke that opens
  the same command twice.

Screenshots, cropped to the window (and inspected):

| Step | Evidence |
| --- | --- |
| Root search lists the three sample commands | [1-root.png](evidence/macos/1-root.png) |
| Rust command opened; action result "Waited 50 ms inside the Rust guest" | [2-command-0.png](evidence/macos/2-command-0.png), [2-result-0.png](evidence/macos/2-result-0.png) |
| JavaScript command; "Waited 50 ms inside the JavaScript guest" | [3-command-1.png](evidence/macos/3-command-1.png), [3-result-1.png](evidence/macos/3-result-1.png) |
| TypeScript command; "Waited 50 ms inside the TypeScript guest" | [4-command-2.png](evidence/macos/4-command-2.png), [4-result-2.png](evidence/macos/4-result-2.png) |
| Escape returns to root search | [5-back-to-root.png](evidence/macos/5-back-to-root.png) |

Rendering, keyboard focus, selection, guest execution and result display all
worked natively.

## macOS-specific fixes made for this sample

- **Text rendering:** GPUI CE's macOS text system is behind the `font-kit`
  feature of `gpui_platform`, which was off. The first run
  ([36363402510](https://github.com/wasimysaid/pane/actions/runs/36363402510))
  drew backgrounds but no text. The feature is now enabled in
  `crates/pane/Cargo.toml`.
- **Smoke script portability:** BSD `seq` prints `1 0` for `seq 0`, so an earlier
  smoke opened the TypeScript command where it meant to open Rust, and passed
  because it only checked that text was drawn. The key loop now counts in bash
  arithmetic, and the smoke asserts that the result screens differ.

No process or path changes were needed. On every OS the launcher reads guests
from `PANE_EXTENSIONS_DIR`, or else from `target/guests` in the source tree it
was built from. That is a development-build assumption, and the installer
tickets will replace it.

## Platform availability (#19)

The smoke also runs the platform-availability steps (screenshots 13 to 15,
[platform availability](../platform-availability.md#checks)): the Rust
command's Windows-only and macOS-and-Linux actions, then a package listing
only the two other systems. In run [36372625940](https://github.com/wasimysaid/pane/actions/runs/36372625940) (commit `38a95cb`, macOS 15.7.9, arm64) every step passed: the Windows-only action was listed with "Not available on macOS: this action supports only Windows" and did not run, the macOS-and-Linux action answered, and the package for Windows and Linux was refused with "Not available on macOS: this package supports only Windows and Linux". The list scrolled to keep the selected row visible. The later #19 fixes (per-command platforms, re-focusing before these steps) have not run here yet.

| Step | Evidence |
| --- | --- |
| Windows-only action | [13-windows-only.png](evidence/macos/13-windows-only.png) |
| macOS-and-Linux action | [14-not-windows.png](evidence/macos/14-not-windows.png) |
| Package for the other two systems | [15-no-compatible-package.png](evidence/macos/15-no-compatible-package.png) |

## Text input and accessibility findings

- **Text input / IME (#20):** extension forms now have a text field. The
  smoke also opens the Rust command's form, submits it empty (the error color
  must appear), types "Ada" through System Events `keystroke`, then Tab, Down
  and Return (the result color must appear, which only happens if the typed
  text reached the name field). In run [36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770) (macOS 15.7.9, arm64) every step passed and the result read "Good morning, Ada, from the Rust guest". Composition with a macOS input method (for example
  Japanese Kana) through `NSTextInputClient` is unverified; the window tests
  cover composition only on the field's editing state
  ([what that proves](../forms.md#checks)). Editing bindings
  follow the element's macOS defaults (Cmd-A/C/V/X/Z, Option-arrow words); only typing, Tab and the arrow keys ran natively.

Screenshots from run [36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770) (commit `949e35d`), cropped to the window:

| Step | Evidence |
| --- | --- |
| Form opened; focus in the name field | [6-form.png](evidence/macos/6-form.png) |
| Submitted empty; "Enter a name" on the field and status | [7-form-error.png](evidence/macos/7-form-error.png) |
| Typed "Ada", Tab, Down to "Good morning", submitted | [8-form-result.png](evidence/macos/8-form-result.png) |

- **Accessibility:** the window exposes a `ListBox` labelled with the view
  title, `ListBoxOption` rows with label, description and selected state, the
  selected row as the active descendant, and a `Status` node for the result.
  This is verified through GPUI's accessibility tree in the window tests
  (`assistive_technology_sees_the_list_the_selection_and_the_result`), which
  pass on macOS but are platform-independent. **VoiceOver was not run**, so how
  the tree reaches NSAccessibility and what VoiceOver announces are unverified.
  Forms: see [accessibility of forms](../forms.md#accessibility).
- **Custom view (#21):** after a final restart the smoke opens the Rust command's
  color picker, presses Right (key code 124) and clicks the dark green swatch
  with a Quartz mouse event posted through Python `ctypes`, converting the
  screenshot's pixels to points (half on Retina). Each step must show the
  chosen color over at least 3000 pixels. **This phase has not run on macOS
  yet**; whether posting the Quartz event needs a permission beyond the one
  System Events has is unverified. Accessibility: see
  [custom views](../custom-views.md#accessibility).

## Local extension package (#9)

`scripts/smoke-macos.sh` also installs `target/guests/packages/sample-rust` with
`pane --install <folder>` (with `PANE_DATA_DIR` pointing at a fresh folder),
runs its command, and restarts Pane. In run [36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770) (commit `949e35d`) every
step passed. The `packages` identity tests also passed there: folder paths with
spaces and Unicode, letter case and Unicode normalization as this file system
treats them, and symbolic links.

| Step | Evidence |
| --- | --- |
| Package screen: source, version, commands, compatibility | [9-package.png](evidence/macos/9-package.png) |
| Installed; the new command is selected in root search | [10-installed.png](evidence/macos/10-installed.png) |
| The installed command answers ("Hello from the Rust guest") | [11-installed-result.png](evidence/macos/11-installed-result.png) |
| Still listed after a restart | [12-restarted.png](evidence/macos/12-restarted.png) |

The installed copy has the same title as the built-in Rust sample, so the
screenshots can't show which copy opened; the core tests prove the installed
copy runs. In these screenshots the root list is taller than the window and its
last row is cut off; since #19 the list scrolls to keep the selected row
visible.

## Remaining limits

- Only a CI runner was used; no one has run it on a contributor's own Mac.
- Intel Macs, other macOS versions and the JS/TS toolchain build on macOS are
  untested. The wasi-sdk checksum for macOS in `tools/componentize-js/pins.json`
  has not been checked against a real download.
- The smoke confirms that text appears in the expected colors and that the
  three results differ. Which command and guest each screenshot shows was
  checked by inspection.
