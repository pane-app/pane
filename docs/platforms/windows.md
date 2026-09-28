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

## Remaining limits

- Only a CI runner (Windows Server) was used, not a Windows 10/11 desktop.
- No screen reader (Narrator/NVDA) was run. The accessibility tree is verified
  only through GPUI in the platform-independent window tests.
- The smoke confirms that text appears in the expected colors and that the
  three results differ. Which command and guest each screenshot shows was
  checked by inspection.
