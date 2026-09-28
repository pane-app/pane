# Windows native baseline (#5, #6)

Recorded 2026-09-28 from GitHub Actions run
[36366760796](https://github.com/wasimysaid/pane/actions/runs/36366760796) on the
fork `wasimysaid/pane`, commit `572d629`.

## Tested combination

| | |
| --- | --- |
| OS | Windows Server 2025 Datacenter, `Microsoft Windows NT 10.0.26100.0` (build 26100) |
| Architecture | x86_64 (`AMD64`) |
| Machine | GitHub-hosted runner, image `windows-2025-vs2026` version 20260922.246.2 |
| Session | the runner's interactive desktop session (Explorer shell, taskbar) |
| Toolchain | MSVC from the image's Visual Studio; Rust 1.98.1 (`rust-toolchain.toml`) |

**Not tested, so not claimed:** Windows 10/11 client editions, ARM64, a physical
display or GPU driver other than the runner's, high-DPI scaling, and any
installer or signed build.

## Fresh checkout build and checks

```powershell
rustup toolchain install
cargo xtask ci          # guests, prebuilt JS/TS check, fmt, clippy, all tests
cargo build --locked -p pane
```

Result: `cargo xtask ci` passed (13 window, 9 launcher-model, 1 runtime-cache
and 25 sample-contract tests), and the launcher built. The Rust guests are built
natively; the JS and TS samples use the committed prebuilt components.
Rebuilding them from source on Windows (`cargo xtask js-guests`) has **not**
been run. The Cargo cache was warm (`Swatinem/rust-cache`).

## Native GUI smoke

```powershell
cargo build -p pane
./scripts/smoke-windows.ps1 -OutDir smoke   # needs Python 3 with Pillow
```

The script launches `target\debug\pane.exe` and waits for its main window. It
then brings the window to the foreground and sends real key events with
`SendKeys`. For each of the Rust, JavaScript and TypeScript sample commands it
presses Enter to open it, Down and Enter to run "Wait briefly" (an async WASI 0.3
clock import inside the guest), then Escape.

It fails if the window does not appear or Pane exits. The same three screenshot
checks as on macOS and Linux also run (`scripts/check_screenshot.py`):

- The root screen draws text in the hint color.
- Each result screen draws text in the result color.
- The three result screens are all different.

Screenshots, cropped to the window (and inspected):

| Step | Evidence |
| --- | --- |
| Root search lists the three sample commands | [1-root.png](evidence/windows/1-root.png) |
| Rust command opened; action result "Waited 50 ms inside the Rust guest" | [2-command-0.png](evidence/windows/2-command-0.png), [2-result-0.png](evidence/windows/2-result-0.png) |
| JavaScript command; "Waited 50 ms inside the JavaScript guest" | [3-command-1.png](evidence/windows/3-command-1.png), [3-result-1.png](evidence/windows/3-result-1.png) |
| TypeScript command; "Waited 50 ms inside the TypeScript guest" | [4-command-2.png](evidence/windows/4-command-2.png), [4-result-2.png](evidence/windows/4-result-2.png) |
| Escape returns to root search | [5-back-to-root.png](evidence/windows/5-back-to-root.png) |

Rendering, keyboard focus, selection, guest execution and result display all
worked natively. In the root screenshots the TypeScript row also has a lighter
background. This is most likely hover under wherever the runner's mouse pointer
sits, since keyboard selection (the Rust row) opened the Rust command. This was
not confirmed.

## Platform availability (#19)

The smoke also runs the platform-availability steps (screenshots 13 to 15,
[platform availability](../platform-availability.md#checks)): the Rust
command's Windows-only and macOS-and-Linux actions, then a package listing
only the two other systems. In run [36372625940](https://github.com/wasimysaid/pane/actions/runs/36372625940) (commit `38a95cb`, Windows NT 10.0.26100, AMD64) every step passed: the Windows-only action answered, the macOS-and-Linux action was listed with "Not available on Windows: this action supports only macOS and Linux" and did not run, and the package for macOS and Linux was refused with "Not available on Windows: this package supports only macOS and Linux". The list scrolled to keep the selected row visible. The later #19 fixes (per-command platforms, re-focusing before these steps) have not run here yet.

| Step | Evidence |
| --- | --- |
| Windows-only action | [13-windows-only.png](evidence/windows/13-windows-only.png) |
| macOS-and-Linux action | [14-not-windows.png](evidence/windows/14-not-windows.png) |
| Package for the other two systems | [15-no-compatible-package.png](evidence/windows/15-no-compatible-package.png) |

## Root search (#23)

Root search has a query field with focus ([root search](../root-search.md)).
The smoke's last phase (screenshots 24 to 26) types "typescr" with
`SendKeys`, opens the only match and runs "Wait briefly", which must look
exactly like step 4, then types "zzz" and presses Enter on no results. In run [36420611977](https://github.com/wasimysaid/pane/actions/runs/36420611977) (commit `6d73d18`) every step passed: typing "typescr" left only TypeScript sample and Enter ran it, and "zzz" showed No results ([24-search.png](evidence/windows/24-search.png), [26-no-results.png](evidence/windows/26-no-results.png)). Input-method composition in the query field is still unverified here.

## Calculator (#27)

The smoke's last phase (screenshots 27 to 30) installs the calculator
package, types "6*7", checks the selected answer row, presses Enter to copy
it, then compares typing "42+1" with pasting the copy (Ctrl+A, Ctrl+V through `SendKeys`) and typing
"+1", which must look the same. In run [36423871204](https://github.com/wasimysaid/pane/actions/runs/36423871204) (commit `ab91081`) every step passed: "6*7" answered 42, Enter copied it, and pasting then typing "+1" matched typing "42+1", so the system clipboard held "42" ([27-answer.png](evidence/windows/27-answer.png), [30-pasted.png](evidence/windows/30-pasted.png)).

## Quicklinks (#28)

The smoke's last phase (screenshots 33 and 34) installs the Quicklinks
package, creates "Pane issues" (https://example.com/pane-issues) in its
form, restarts Pane and types "pane iss", which must list it selected. It
stops before Enter, which would open the default browser; opening a link
here is checked only through the tests' recording opener. Not run yet.

## Text input and accessibility findings

- **Text input / IME (#20):** the smoke now also opens the Rust command's
  form, submits it empty (the error color must appear), types "Ada" with
  `SendKeys`, then Tab, Down and Enter (the result color must appear, which
  only happens if the typed text reached the name field). In run [36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770) (Windows NT
  10.0.26100, AMD64) every step passed and the result read "Good morning, Ada,
  from the Rust guest". Windows IME
  (TSF) composition, for example with Microsoft Japanese IME, is unverified;
  the window tests cover composition only on the field's editing state
  ([what that proves](../forms.md#checks)).

Screenshots from run [36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770) (commit `949e35d`), cropped to the window:

| Step | Evidence |
| --- | --- |
| Form opened; focus in the name field | [6-form.png](evidence/windows/6-form.png) |
| Submitted empty; "Enter a name" on the field and status | [7-form-error.png](evidence/windows/7-form-error.png) |
| Typed "Ada", Tab, Down to "Good morning", submitted | [8-form-result.png](evidence/windows/8-form-result.png) |

- **Accessibility:** see [accessibility of forms](../forms.md#accessibility)
  and [of custom views](../custom-views.md#accessibility). Narrator/NVDA were
  not run.
- **Custom view (#21):** after a final restart the smoke opens the Rust command's
  color picker, presses Right and clicks the dark green swatch with `user32`
  `SetCursorPos` and `mouse_event`, at the screenshot's pixel position. Each
  step must show the chosen color over at least 3000 pixels. In run [36378453278](https://github.com/wasimysaid/pane/actions/runs/36378453278) (commit `1487dc8`, Windows NT 10.0.26100, AMD64) every step passed: the picker opened on blue (#1E88E5), Right moved to purple (#8E24AA) and the click chose dark green (#1B5E20). The runner displays at 100 % scaling, so other scaling is still unverified. The script calls `SetProcessDPIAware` first, so
  the screenshot, the screen bounds and `SetCursorPos` all use physical
  pixels and the click should land on the swatch at any display scaling;
  scaling other than 100 % is unverified.

## Disabling an extension and keeping its settings (#10)

In run [36378453278](https://github.com/wasimysaid/pane/actions/runs/36378453278) (commit `1487dc8`) the disable phase passed: the Settings sample
saved the formal greeting, was disabled in Manage extensions, stayed disabled
and absent from root search after a restart (the root screenshot matches the
one taken before the package was installed), was enabled again, and "Greet me"
answered "Good day to you" from the kept setting.

| Step | Evidence |
| --- | --- |
| Formal greeting saved | [16-setting-saved.png](evidence/windows/16-setting-saved.png) |
| Disabled in Manage extensions | [17-disabled.png](evidence/windows/17-disabled.png) |
| After a restart: Greeting absent from root | [18-restarted-disabled.png](evidence/windows/18-restarted-disabled.png) |
| Enabled again | [19-enabled.png](evidence/windows/19-enabled.png) |
| The kept setting answers | [20-greeted.png](evidence/windows/20-greeted.png) |

Custom view screenshots from the same run: [21-color.png](evidence/windows/21-color.png),
[22-color-key.png](evidence/windows/22-color-key.png),
[23-color-click.png](evidence/windows/23-color-click.png).

## Local extension package (#9)

`scripts/smoke-windows.ps1` also installs `target/guests/packages/sample-rust` with
`pane --install <folder>` (with `PANE_DATA_DIR` pointing at a fresh folder),
runs its command, and restarts Pane. In run [36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770) (commit `949e35d`) every
step passed. The `packages` identity tests also passed there: folder paths with
spaces and Unicode, letter case and Unicode normalization as this file system
treats them, and symbolic links. The symbolic-link test skips itself where
directory links are not allowed, and cargo hides that notice for a passing
test. The runner's administrator account can normally create them, but a skip
can't be ruled out from the log.

| Step | Evidence |
| --- | --- |
| Package screen: source, version, commands, compatibility | [9-package.png](evidence/windows/9-package.png) |
| Installed; the new command is selected in root search | [10-installed.png](evidence/windows/10-installed.png) |
| The installed command answers ("Hello from the Rust guest") | [11-installed-result.png](evidence/windows/11-installed-result.png) |
| Still listed after a restart | [12-restarted.png](evidence/windows/12-restarted.png) |

The installed copy has the same title as the built-in Rust sample, so the
screenshots can't show which copy opened; the core tests prove the installed
copy runs. In these screenshots the root list is taller than the window and its
last row is cut off; since #19 the list scrolls to keep the selected row
visible.

## Remaining limits

- Only a CI runner (Windows Server) was used, not a Windows 10/11 desktop.
- No screen reader (Narrator/NVDA) was run. The accessibility tree is verified
  only through GPUI in the platform-independent window tests.
- The smoke confirms that text appears in the expected colors and that the
  three results differ. Which command and guest each screenshot shows was
  checked by inspection.
