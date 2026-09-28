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

## Text input and accessibility findings

- **Text input / IME:** the current controls have no text field (root search
  lists commands but has no query input yet), so text input and IME could not
  be exercised. They remain open for the ticket that adds query input.
- **Accessibility:** the window exposes a `ListBox` labelled with the view
  title, `ListBoxOption` rows with label, description and selected state, the
  selected row as the active descendant, and a `Status` node for the result.
  This is verified through GPUI's accessibility tree in the window tests
  (`assistive_technology_sees_the_list_the_selection_and_the_result`), which
  pass on macOS but are platform-independent. **VoiceOver was not run**, so how
  the tree reaches NSAccessibility and what VoiceOver announces are unverified.

## Remaining limits

- Only a CI runner was used; no one has run it on a contributor's own Mac.
- Intel Macs, other macOS versions and the JS/TS toolchain build on macOS are
  untested. The wasi-sdk checksum for macOS in `tools/componentize-js/pins.json`
  has not been checked against a real download.
- The smoke confirms that text appears in the expected colors and that the
  three results differ. Which command and guest each screenshot shows was
  checked by inspection.
