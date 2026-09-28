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
(an async WASI 0.3 clock import inside the guest), then Escape. It fails if the
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

Rendering, keyboard focus, selection, guest execution and result display all
worked natively. Mesa reported "No DRI3 support detected - required for
presentation" on stderr (expected under Xvfb); frames were still presented.

## Text input and accessibility findings

- **Text input / IME:** the current controls have no text field (root search
  lists commands but has no query input yet), so text input and IME could not
  be exercised. They remain open for the ticket that adds query input.
- **Accessibility:** before this slice the window exposed only an empty
  `Window` node to assistive technology. The list is now a `ListBox` labelled
  with the view title and holding keyboard focus; rows are `ListBoxOption`s
  with label, description and selected state; the selected row is the active
  descendant; the result or error line is a `Status` node. This is verified
  through GPUI's accessibility tree in the window tests
  (`assistive_technology_sees_the_list_the_selection_and_the_result`), which is
  platform-independent. **No screen reader (Orca/AT-SPI) was run**, so
  announcement behaviour on Linux is unverified.

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
