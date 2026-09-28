# Linux native baseline (#8)

Recorded 2026-09-28 on top of commit `1048ffb`.

## Tested combination

| | |
| --- | --- |
| Distribution | Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic |
| Architecture | x86_64 (24 cores) |
| Display protocol | **X11**, through Xvfb 21.1.22 (Ubuntu `xvfb 2:21.1.22-1ubuntu1.2`), 1280×800×24 |
| Desktop / window manager | none (bare Xvfb, no compositor) |
| GPU / Vulkan | Mesa 26.0.8 lavapipe (software Vulkan, `mesa-vulkan-drivers`), Vulkan loader 1.4.341 |
| Rust | 1.98.1 (`rust-toolchain.toml`) |

**Not tested, so not claimed:** Wayland (no compositor was available), a physical
display or real GPU driver, any desktop environment (GNOME, KDE, …), aarch64,
and any other distribution. The GUI ran on a real X server with real X11 input
events, but a virtual framebuffer is not a desktop session.

## Fresh checkout build and checks

A fresh `git clone` of the repository, then only the documented commands:

```sh
rustup toolchain install
cargo xtask ci          # guests, prebuilt JS/TS check, fmt, clippy, all tests
cargo build -p pane
```

Result: `cargo xtask ci` passed (12 launcher, 9 core, 1 cache and 25
sample-contract tests; window tests at that time 12), and the launcher built, in
2 min 43 s wall time from an empty `target/` with a warm Cargo registry. No
Windows machine or PowerShell was needed. Linux prerequisites are in the
[README](../../README.md#build-run-and-test).

## Native GUI smoke

```sh
cargo build -p pane
scripts/smoke-linux.sh smoke            # needs Xvfb, xdotool, and ImageMagick or Pillow
```

The script starts Xvfb, launches `target/debug/pane`, and sends real X11 key
events with xdotool: for each of the Rust, JavaScript and TypeScript sample
commands it presses Enter to open it, Down and Enter to run "Wait briefly"
(an async WASI 0.3 clock import inside the guest), then Escape. Since #20 it
then opens the Rust command's form, submits it empty, types a name, presses
Tab, Down and Enter, and checks the error and result colors. It fails if the
window does not appear or Pane exits. Without root, Xvfb and xdotool were
unpacked from the distribution packages (`apt-get download`, `dpkg -x`) and
selected with `PANE_XVFB`, `PANE_XDOTOOL` and `LD_LIBRARY_PATH`; CI installs them
normally.

Screenshots (inspected, not machine-asserted):

| Step | Evidence |
| --- | --- |
| Root search lists the three sample commands | [1-root.png](evidence/linux-x11/1-root.png) |
| Rust command opened; action result "Waited 50 ms inside the Rust guest" | [2-command-0.png](evidence/linux-x11/2-command-0.png), [2-result-0.png](evidence/linux-x11/2-result-0.png) |
| JavaScript command; "Waited 50 ms inside the JavaScript guest" | [3-command-1.png](evidence/linux-x11/3-command-1.png), [3-result-1.png](evidence/linux-x11/3-result-1.png) |
| TypeScript command; "Waited 50 ms inside the TypeScript guest" | [4-command-2.png](evidence/linux-x11/4-command-2.png), [4-result-2.png](evidence/linux-x11/4-result-2.png) |
| Escape returns to root search | [5-back-to-root.png](evidence/linux-x11/5-back-to-root.png) |
| Rust command's form "Greet someone" opened, name field focused (#20) | [6-form.png](evidence/linux-x11/6-form.png) |
| Submitted empty: "Enter a name" under the field, focus back on it | [7-form-error.png](evidence/linux-x11/7-form-error.png) |
| Typed "Ada", Tab, Down, Enter: "Good morning, Ada, from the Rust guest" | [8-form-result.png](evidence/linux-x11/8-form-result.png) |

Rendering, keyboard focus, selection, guest execution and result display all
worked natively. Mesa reported "No DRI3 support detected - required for
presentation" on stderr (expected under Xvfb); frames were still presented.

### Installing a local package (#9)

The smoke then keeps Pane's data in `<output-dir>/data` (`PANE_DATA_DIR`),
starts `pane --install target/guests/packages/sample-rust`, presses Enter on
**Install**, opens and runs the installed command, and restarts Pane. Run
locally on 2026-09-28 (Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic, x86_64,
same Xvfb/lavapipe setup), all screenshot checks passed:

| Step | Evidence |
| --- | --- |
| Package screen: source folder, version, commands, compatibility, Install | `9-package.png` (not committed: it shows the local checkout path) |
| Installed; root lists the samples, the installed "Rust sample", then the install row; status "Installed Rust sample" | [10-installed.png](evidence/linux-x11/10-installed.png) |
| The installed command answers "Hello from the Rust guest" | [11-installed-result.png](evidence/linux-x11/11-installed-result.png) |
| After a restart the installed command is still listed | [12-restarted.png](evidence/linux-x11/12-restarted.png) |

The folder picker itself is the XDG desktop portal, which this Xvfb session
does not run, so the smoke uses `--install`; the picker flow is covered by the
GPUI window tests (`crates/pane/tests/install.rs`). The macOS and Windows
smokes run the same phase (screenshots 9 to 12); it has not run there yet.

### Platform availability (#19)

After the restart the smoke opens the Rust command again and activates its
seventh item, declared for Windows only, then its eighth, declared for macOS
and Linux; finally it starts `pane --install` on a package whose `pane.json`
lists only Windows and macOS. Run locally on 2026-09-28 (same Ubuntu 26.04.1,
Xvfb/lavapipe setup), all screenshot checks passed. The list scrolls to keep
the selected row visible:

| Step | Evidence |
| --- | --- |
| "Windows-only action" listed with "Not available on Linux: this action supports only Windows"; Enter shows the reason as the error | [13-windows-only.png](evidence/linux-x11/13-windows-only.png) |
| "macOS and Linux action" runs: "Ran the macOS and Linux action in the Rust guest" | [14-not-windows.png](evidence/linux-x11/14-not-windows.png) |
| The Windows/macOS package: "Not available on Linux: this package supports only Windows and macOS", nothing to install | `15-no-compatible-package.png` (not committed: it shows the local path) |

The macOS and Windows smokes run the same steps with their own expected
results; they have not run there yet.

### Disabling a package and keeping its settings (#10)

The smoke then installs `target/guests/packages/sample-settings`, opens its
Greeting command and chooses "Use a formal greeting" (the guest saves it with
`pane:extension/settings`), and disables Settings sample in **Manage
extensions…**. It checks that `installed.json` records `"disabled": true` and
`settings.json` holds the saved style, restarts Pane, enables the package
again, and runs "Greet me", which answers in the saved style and is an error
when no style is saved. Run locally on 2026-09-28 (Ubuntu 26.04.1 LTS, kernel
7.0.0-31-generic, x86_64, same Xvfb/lavapipe setup), all checks passed:

| Step | Evidence |
| --- | --- |
| The guest saves the choice: "Saved the formal greeting" | [16-setting-saved.png](evidence/linux-x11/16-setting-saved.png) |
| Manage extensions: Settings sample "Disabled", status "Disabled Settings sample" | `17-disabled.png` (not committed: it shows the local checkout path) |
| After a restart root search no longer lists Greeting | [18-restarted-disabled.png](evidence/linux-x11/18-restarted-disabled.png) |
| Enabled again: "Enabled Settings sample" | `19-enabled.png` (not committed: it shows the local checkout path) |
| Greeting is back, titled "Greeting: formal", and "Greet me" answers "Good day to you" | [20-greeted.png](evidence/linux-x11/20-greeted.png) |

The smoke asserts Greeting's absence after the restart by comparing the
root screenshot with the one taken before the settings sample was installed
(`check_screenshot.py --same`, pixel for pixel): Greeting would take the fifth
visible row. The launcher tests (`crates/pane-core/tests/disable.rs`) assert it
row by row, for the Rust, JavaScript and TypeScript settings samples.
The macOS and Windows smokes run the same phase (screenshots 16 to 20); they
have not been run for this change.

### Custom view (#21)

Finally the smoke restarts Pane again and opens the Rust command's "Choose a color", a
color picker the guest draws ([custom views](../custom-views.md)), presses
Right with a real X11 key event, then moves the real pointer onto the dark
green swatch (found in the screenshot by its color with
`check_screenshot.py --locate`) and clicks it with `xdotool`. Each screenshot
must show the chosen color over at least 3000 pixels: its swatch and the
preview together cover about 5100, any other swatch about 1000. Run locally on
2026-09-28 (Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic, x86_64, same
Xvfb/lavapipe setup), all checks of the whole smoke passed.

| Step | Evidence |
| --- | --- |
| View opened with focus (focus ring); blue chosen, its hex code under the preview | [21-color.png](evidence/linux-x11/21-color.png) |
| Right: purple chosen, "#8E24AA" | [22-color-key.png](evidence/linux-x11/22-color-key.png) |
| Click on the dark green swatch: dark green chosen, "#1B5E20" | [23-color-click.png](evidence/linux-x11/23-color-click.png) |

A drag is not driven natively; it is covered by the window tests. The macOS
and Windows smokes run the same phase (screenshots 21 to 23), with their own
click helpers; it has not run there yet.

## Text input and accessibility findings

- **Text input / IME (#20):** extension forms have a text field (GPUI CE's
  editable text element). On 2026-09-28 the smoke above, on the same Ubuntu
  26.04.1 / Xvfb combination, typed "Ada" with real X11 key events
  (`xdotool type`) into the name field after a rejected empty submission had
  returned focus to it, then used Tab and Down to change the greeting and
  Enter to submit; the guest's answer shows the typed text arrived
  ([8-form-result.png](evidence/linux-x11/8-form-result.png), checked by the
  script's result-color assertion and by inspection). Editing keys (arrows,
  Backspace), Tab order and composition are covered by the window tests
  through GPUI's test platform, where composition is driven on the
  focused field's editing state (`replace_and_mark_text_in_range` then
  `replace_text_in_range`) as a platform input method would, not through the
  window's platform input handler ([what that proves](../forms.md#checks)). **No real input
  method (IBus, Fcitx) was run**: X11 XIM/preedit handling in GPUI CE and
  composition with a CJK IME on Linux are unverified.
- **Accessibility:** before this slice the window exposed only an empty
  `Window` node to assistive technology. The list is now a `ListBox` labelled
  with the view title and holding keyboard focus; rows are `ListBoxOption`s
  with label, description and selected state; the selected row is the active
  descendant; the result or error line is a `Status` node. This is verified
  through GPUI's accessibility tree in the window tests
  (`assistive_technology_sees_the_list_the_selection_and_the_result`), which is
  platform-independent. **No screen reader (Orca/AT-SPI) was run**, so
  announcement behaviour on Linux is unverified. Forms are covered in
  [accessibility of forms](../forms.md#accessibility) and custom views in
  [their accessibility](../custom-views.md#accessibility), which apply to all
  three platforms.

## Remaining limits

- Wayland, a real desktop session and hardware GPU drivers are untested.
- The GUI smoke now also asserts that text is drawn in the expected colors and
  that the three result screens differ (`scripts/check_screenshot.py`). Which
  command and guest each screenshot shows is still checked by inspection.

## CI result

The same script runs in GitHub Actions on the fork `wasimysaid/pane`. Run
[36366760796](https://github.com/wasimysaid/pane/actions/runs/36366760796)
(commit `572d629`) passed on the `ubuntu-24.04` runner:

| | |
| --- | --- |
| OS | Ubuntu 24.04.5 LTS, kernel 6.17.0-1022-azure, x86_64 |
| Runner image | `ubuntu-24.04` version 20260920.314.1 |
| Display / GPU | Xvfb and Mesa lavapipe from the Ubuntu 24.04 archive (as above: X11 only, software Vulkan) |

`cargo xtask ci` passed (13 window, 9 launcher-model, 1 runtime-cache and 25
sample-contract tests). The smoke passed all of its screenshot checks, and the
uploaded screenshots show the Rust, JavaScript and TypeScript results in turn.
The upstream repository `hoangvu12/pane` still has no configured runner.
