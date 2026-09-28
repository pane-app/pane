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

The smoke then restarts Pane again and opens the Rust command's "Choose a color", a
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

### Root search (#23)

Root search now has a query field with focus ([root search](../root-search.md));
the earlier phases still reach each command with Down, which moves the
selection while the field keeps focus, and the screenshots above predate the
field. The search phase restarts Pane, types "typescr" with real X11 key
events, presses Enter on the only match and runs "Wait briefly"; the screen
must be pixel for pixel step 4's (`--same` with `4-result-2.png`). Escape
clears the query, "zzz" and Enter show no results and open nothing, and root,
the search, the result and the no-results screens must all differ. Run
locally on 2026-09-28 (Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic, x86_64,
same Xvfb/lavapipe setup), all checks of the whole smoke passed.

| Step | Evidence |
| --- | --- |
| "typescr" typed: only TypeScript sample, selected | [24-search.png](evidence/linux-x11/24-search.png) |
| Enter, Down, Enter: "Waited 50 ms inside the TypeScript guest" | `25-search-result.png` (identical to [4-result-2.png](evidence/linux-x11/4-result-2.png)) |
| "zzz" and Enter: "No results for “zzz”", still root, status idle | [26-no-results.png](evidence/linux-x11/26-no-results.png) |

No input method (IBus, Fcitx) was used; composition in the field is covered
only by the window tests. The macOS and Windows smokes run the same phase
(screenshots 24 to 26); it has not run there yet.

### Calculator (#27)

The calculator phase installs the calculator package
(`--install target/guests/packages/calculator`), types "6*7" with real X11
key events and checks the selected answer row's color; Enter copies the
answer. Ctrl+A and typing "42+1" gives screenshot 29; Ctrl+A, Ctrl+V (the
copied "42") and typing "+1" must give exactly the same screen (`--same`),
which holds only if the X11 clipboard held "42"; screens 27 to 29 must
differ. Run locally on 2026-09-28 (Ubuntu 26.04.1 LTS, kernel
7.0.0-31-generic, x86_64, same Xvfb/lavapipe setup): all checks of the
whole smoke passed.

| Step | Evidence |
| --- | --- |
| "6*7" typed: the answer 42, first and selected, computed by the calculator's guest | [27-answer.png](evidence/linux-x11/27-answer.png) |
| Enter: "Copied 42 to the clipboard", root search unchanged | [28-copied.png](evidence/linux-x11/28-copied.png) |
| Pasted "42", typed "+1": the answer 43, the same screen as typing "42+1" | [30-pasted.png](evidence/linux-x11/30-pasted.png) |

The macOS and Windows smokes run the same phase (screenshots 27 to 30, with
Cmd and Ctrl respectively); it has not run there yet. Disabling the
calculator is covered by the launcher tests, not natively.

### Operations (#22)

The operations phase (screenshots 31 and 32) installs the JavaScript
operations sample, then the Rust one
(`--install target/guests/packages/sample-operations-js`, then
`sample-operations`), opens the Rust sample's command and fills its form with
real X11 key events: the JavaScript package's identity (`local:` and the
resolved folder path) and the name "Rust". The Rust guest calls that
package's `greet` operation through Pane, which starts its guest for the
call. Run locally on 2026-09-28 (Ubuntu 26.04.1 LTS,
kernel 7.0.0-31-generic, x86_64, same Xvfb/lavapipe setup): all checks of
the whole smoke passed.

| Step | Evidence |
| --- | --- |
| JavaScript operations sample installed | `31-operations-target.png` (not committed: it shows local paths) |
| "Hello, Rust, from JavaScript", from the other package's guest | `32-operation-answer.png` (not committed: it shows the typed local path) |

Both steps passed locally; the CI smoke's screenshots are the evidence to
keep for this phase.

The macOS and Windows smokes run the same phase (screenshots 31 and 32); it
has not run there yet. The other directions (JavaScript and TypeScript
calling Rust) and every failure are covered by the launcher tests, not
natively.

### Reloading a package (#11)

After the operations phase, the smoke writes a package `Dev` in
`<output-dir>/dev` whose component
is a copy of the Rust sample, installs it and runs "Say hello". It then
copies the JavaScript sample over the component, reloads Dev in **Manage
extensions…** without restarting Pane, and runs "Say hello" again. Next it
deletes the component and reloads (the checks fail, so the working code must
keep answering exactly as before), and finally copies in the
`failing-start` fixture, whose first start traps, reloads, and presses Retry.
It checks that `settings.json` kept the setting the failed start saved. Run
locally on 2026-09-28 (Ubuntu 26.04.1 LTS, kernel 7.0.0-37-generic, x86_64,
same Xvfb/lavapipe setup), all checks of the whole smoke passed:

| Step | Evidence |
| --- | --- |
| Dev as installed: the Rust guest answers | [33-dev-before.png](evidence/linux-x11/33-dev-before.png) |
| Reload Dev: "Reloaded Dev" | `34-reloaded.png` (not committed: it shows local paths) |
| The same command now shows the JavaScript sample and its answer (`--distinct` from step 33) | [35-dev-after.png](evidence/linux-x11/35-dev-after.png) |
| Component deleted, Reload Dev: "Dev was not reloaded: Not ready to run: … It keeps running its installed code." | `36-not-reloaded.png` (not committed: it shows local paths) |
| The command still answers from the JavaScript code, pixel for pixel as in step 35 (`--same`) | `37-still-running.png` |
| Failing start: "Reloaded Dev, but it failed to start; its earlier code is not restored. …" | `38-start-failed.png` (not committed: it shows local paths) |
| Retry starting Dev: "Started Dev" | `39-retried.png` (not committed: it shows local paths) |

The macOS and Windows smokes run the same phase (screenshots 33 to 39); it
has not run there yet.

### Clearing an extension's cache (#39)

After the reload phase, the smoke restarts Pane, and in Greeting chooses
"Save a note" and
"Sign in", so the settings sample keeps one value of each kind of
[extension data](../extension-data.md): its style (settings), a note
(content), the greeting cached by "Greet me" earlier (cache) and a token
(credentials). It checks each value in `content.json`, `credentials.json` and
`cache.json`, restarts, chooses "Clear cache of Settings sample" in **Manage
extensions…**, confirms, and shows what Pane keeps again. Finally it checks
that `cache.json` no longer holds the greeting while the style, note and
token are still in their files. Run locally on 2026-09-28 (Ubuntu 26.04.1
LTS, kernel 7.0.0-37-generic, x86_64, same Xvfb/lavapipe setup), all checks
of the whole smoke passed.

| Step | Evidence |
| --- | --- |
| "Style: formal · Note: Water the plants · Signed in: yes · Cached greeting: Good day to you" | [40-kept.png](evidence/linux-x11/40-kept.png) |
| "Clear the cache of Settings sample?", its source, what is deleted and what is kept; Clear cache and Cancel | `41-confirm-clear-cache.png` (not committed: it shows the local checkout path) |
| "Cleared the cache of Settings sample" | `42-cache-cleared.png` (not committed: it shows the local checkout path) |
| "... Cached greeting: none", the other three kept | [43-kept-after-clear.png](evidence/linux-x11/43-kept-after-clear.png) |

The macOS and Windows smokes run the same phase (screenshots 40 to 43); it
has not run there yet.

### Applications (#24, #25, #26)

The applications phase adds a desktop entry "Pane Smoke App" whose `Exec` writes a
marker file, in an `XDG_DATA_HOME` given to Pane only (the system's
`XDG_DATA_DIRS` entries are searched too), installs the
[applications](../applications.md) package, types "pane smoke" with real
X11 key events, checks the selected row, presses Return and checks "Opened
Pane Smoke App" and that the marker was written. Run locally on 2026-09-28
(Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic, x86_64, same Xvfb/lavapipe
setup): all checks of the whole smoke passed. The adapter tests also run a
desktop entry's program natively and explain a missing program and a
terminal application.

| Step | Evidence |
| --- | --- |
| "pane smoke" typed: the application found by name, selected | [44-application.png](evidence/linux-x11/44-application.png) |
| Return: "Opened Pane Smoke App"; its program wrote the marker | [45-opened.png](evidence/linux-x11/45-opened.png) |

Only X11 (Xvfb, no desktop session) ran; Wayland and real desktops'
`XDG_CURRENT_DESKTOP`, Flatpak and Snap folders are untested. Disabling is
covered by the launcher tests, not natively.

### Quicklinks (#28)

The quicklinks phase, after the applications phase, installs the Quicklinks package
(`--install target/guests/packages/quicklinks`), opens its command, and in
"Create quicklink" types "Pane issues", Tab and
"https://example.com/pane-issues" with real X11 key events, then Return.
After a restart it types "pane iss" (the selected row's color must appear)
and presses Return: `xdg-open` runs with no desktop session variables, every
XDG configuration and data location in the smoke's output folder and
`BROWSER` set to a script that records its argument, so no real browser
starts; the script must have received the URL. Run locally on 2026-09-28
(Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic, x86_64, same Xvfb/lavapipe
setup): all checks of the whole smoke passed.

| Step | Evidence |
| --- | --- |
| Form submitted: "Saved quicklink “Pane issues”" | [46-quicklink-saved.png](evidence/linux-x11/46-quicklink-saved.png) |
| Restarted, "pane iss" typed: the quicklink, first and selected | [47-quicklink-found.png](evidence/linux-x11/47-quicklink-found.png) |
| Return: "Opened https://example.com/pane-issues", the URL received by the handler | [48-quicklink-opened.png](evidence/linux-x11/48-quicklink-opened.png) |

A real desktop's handler (GNOME's `gio open`, a browser chosen in the
desktop settings) was not run. The macOS and Windows smokes run the phase up
to screenshot 47; it has not run there yet.

### Uninstalling an extension (#40)

The last phase, after the quicklinks phase, restarts Pane, chooses "Uninstall Settings sample" in
**Manage extensions…** and the first choice, "Uninstall and keep saved
data". It then checks that `installed.json` records the retained data, that
the token is gone from `credentials.json` and that the style and note are
still in `settings.json` and `content.json`; installs the same folder again
and shows what Pane keeps, which must differ from screenshot 43 (signed out
now), and checks that the retained record was dropped. Run locally on
2026-09-28 (Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic, x86_64, same
Xvfb/lavapipe setup): all checks of the whole smoke passed.

| Step | Evidence |
| --- | --- |
| "Uninstall Settings sample?", its source, what is removed, "Saved data: 1 setting and 1 content record", the source folder kept; the three choices | `49-confirm-uninstall.png` (not committed: it shows the local checkout path) |
| "Uninstalled Settings sample; its settings and content are kept" | `50-uninstalled.png` (not committed: the list's rows show the local checkout path) |
| Reinstalled: "Style: formal · Note: Water the plants · Signed in: no · Cached greeting: none" | [51-reinstalled.png](evidence/linux-x11/51-reinstalled.png) |

An earlier run showed the confirmation's list scrolled past its selected
first choice: the first frame of a new screen scrolled with the long
extension list's size and rows, and nothing asked for another frame. The
window now asks for one whenever the screen or rows change and scrolls
again; the rerun shows the first choice selected at the top of the list.

The macOS and Windows smokes run the same phase (screenshots 49 to 51); it
has not run there yet. A managed folder that Windows keeps in use is covered
only by the leftover mechanism's Unix test (a read-only folder), not
natively.

### Global hotkeys (#32, #33, #34)

The last phase, after the uninstall phase ([global hotkeys](../hotkeys.md#checks)), with a data folder
of its own, installs the settings sample, opens "Hotkey for Greeting" in
Manage extensions and presses Ctrl+Alt+G with real X11 key events, then
moves X input focus to the root window (checked with `xdotool
getwindowfocus`) and presses Ctrl+Alt+G again through XTEST: the X server
delivers it to Pane's passive grab and Greeting opens. After a restart the
hotkey (read from `hotkeys.json`) opens Greeting the same way; after
disabling the extension the press changes nothing. Pane's grab is on the
smoke's Xvfb display only (`DISPLAY`; `WAYLAND_DISPLAY` unset). Run locally
on 2026-09-28 (Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic, x86_64, same
Xvfb/lavapipe setup): all checks of the whole smoke passed. The adapter test
also grabs, conflicts, receives an `xdotool` press and releases on an Xvfb
of its own.

| Step | Evidence |
| --- | --- |
| The hotkey screen of Greeting | [52-hotkey-screen.png](evidence/linux-x11/52-hotkey-screen.png) |
| Pane unfocused at root search before the press | [54-unfocused.png](evidence/linux-x11/54-unfocused.png) |
| Ctrl+Alt+G pressed elsewhere: Greeting open in Pane | [55-hotkey-opened.png](evidence/linux-x11/55-hotkey-opened.png) |
| After a restart, the same | [56-hotkey-after-restart.png](evidence/linux-x11/56-hotkey-after-restart.png) |
| Extension disabled: root search before and after the press | [57-disabled.png](evidence/linux-x11/57-disabled.png), [58-disabled-pressed.png](evidence/linux-x11/58-disabled-pressed.png) |

(Screenshot 53, "Ctrl+Alt+G now opens Greeting" on the extension list, is
checked but not kept here: it shows the local package paths.) Xvfb has no
window manager, so raising and focusing Pane's window
(`_NET_ACTIVE_WINDOW`) is not verified; Wayland is explained as unavailable
(tested with a fake, not natively), and no real desktop session ran.

### Deleting retained data (#41)

The retained-data phase, after the hotkeys and pausing phases, with a data folder of its own,
installs the settings sample, saves a note, uninstalls it keeping its saved
data, then chooses "Delete retained data of Settings sample" (the extension
list's last row) and confirms with Down from the selected Cancel, then Return,
waiting for Pane's files to change rather than a fixed time. It checks that `installed.json` no longer has
a `retained` record and that the note is gone from `content.json`; installs
the same folder again and shows what Pane keeps, which must differ from
screenshot 51. Run locally on 2026-09-28 (Ubuntu 26.04.1 LTS, kernel
7.0.0-31-generic, x86_64, same Xvfb/lavapipe setup): all checks of the whole
smoke passed.

| Step | Evidence |
| --- | --- |
| "Delete the retained data of Settings sample?", its source, "Retained data: 1 content record", what is not touched; Cancel (selected) and Delete retained data | `63-confirm-delete-retained.png` (not committed: it shows the local checkout path) |
| "Deleted the retained data of Settings sample", the row gone and nothing installed | [64-retained-deleted.png](evidence/linux-x11/64-retained-deleted.png) |
| Reinstalled: "Style: none · Note: none · Signed in: no · Cached greeting: none" | [65-reinstalled-empty.png](evidence/linux-x11/65-reinstalled-empty.png) |

The macOS and Windows smokes run the same phase (screenshots 63 to 65); it
has not run there yet. A file locked by another program on Windows is
covered only by the tests' unreadable and unwritable files, not natively.

### Aliases and fallbacks (#31)

The phase after the retained-data phase ([aliases and fallbacks](../aliases.md#checks)),
with data folders of its own, installs the query sample, gives Echo the
alias "ec" in its alias form (typed with real X11 key events) and makes it a
fallback, then in root search types "ec hello" (the row sending "hello" to
Echo is listed and selected) and presses Enter ("Echo heard “hello”");
types "zqx" ("No results", then the fallback, not selected), presses Down
(now selected) and Enter ("Echo heard “zqx”"). It checks `aliases.json`,
restarts, disables the extension and types "ec hello": the screen is pixel
for pixel the one a Pane with nothing installed shows for it. Run locally on
2026-09-28 (Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic, x86_64, same
Xvfb/lavapipe setup): all checks of the whole smoke passed.

| Step | Evidence |
| --- | --- |
| "ec hello": the alias row selected, the fallback below | [68-alias-row.png](evidence/linux-x11/68-alias-row.png) |
| Enter: Echo's answer | [69-alias-answer.png](evidence/linux-x11/69-alias-answer.png) |
| "zqx": no results, the fallback not selected | [70-fallback-listed.png](evidence/linux-x11/70-fallback-listed.png) |
| Down and Enter: Echo's answer to "zqx" | [72-fallback-answer.png](evidence/linux-x11/72-fallback-answer.png) |
| Extension disabled: "ec hello" lists nothing | [73-alias-disabled.png](evidence/linux-x11/73-alias-disabled.png) |

(Screenshots 66 and 67, the extension list after saving the alias and the
fallback, are checked but not kept here: they show the local package
paths.)

### Dependencies (#42)

The phase after the alias phase ([dependencies](../dependencies.md#checks)), with a data
folder of its own, previews the dependencies sample, which requires the
JavaScript operations sample (`local:../sample-operations-js`) and can use
the Rust one (optional); the preview lists both. Enter on Install installs
it with the JavaScript sample only; its command, selected, opens, and "Greet
through the required greeter" calls `greet` by the dependency id `greeter`.
Afterwards `installed.json` must hold exactly two packages and the
recorded dependency. Run locally on 2026-09-28 (same Ubuntu 26.04.1 / Xvfb /
lavapipe setup): all checks of the whole smoke passed.

| Step | Evidence |
| --- | --- |
| The preview: "Requires: JavaScript operations sample, installed with it from local:../sample-operations-js", "Optional: `rust-greeter` from local:../sample-operations, not installed: …" and the Install row | [75-dependencies-preview-cropped.png](evidence/linux-x11/75-dependencies-preview-cropped.png) (cropped below the title and the Source line, which shows the local checkout path; the smoke checks the whole frame) |
| "Installed Dependencies sample with JavaScript operations sample, which it requires", its command selected | [76-dependencies-installed.png](evidence/linux-x11/76-dependencies-installed.png) |
| "Hello, Pane, from JavaScript", from the dependency's guest | [77-dependency-answer.png](evidence/linux-x11/77-dependency-answer.png) |

### Native helpers (#15)

The last two phases, after the dependencies phase ([native helpers](../helpers.md#checks)). The first,
with a data folder of its own (`helper-data`), installs the helper sample,
whose `pane-echo` `cargo xtask guests` built for `linux-x86_64`, and runs
it with real X11 key events: the answer names Linux x86-64; "Echo within a
second" cancels the slow run after one second; "Echo after waiting" starts
the ten-second run, which `pgrep -f` finds running from the managed copy in
the data folder; Escape, then disabling the package in Manage extensions,
ends it: `pgrep` finds no helper, `settings.json` keeps "started" and never
gets "finished", and no helper outlives Pane. The second (`helper-quit-data`)
starts the waiting helper again ("Running…"), finds its heartbeat file,
then quits Pane by sending its window `WM_DELETE_WINDOW`
([`scripts/close_window.py`](../../scripts/close_window.py), as a window
manager's close button does; `xdotool windowclose` would destroy the
window instead): Pane must exit within five seconds, `pgrep` must find no
helper, and the heartbeat must stop growing. With the app's quit handler
disabled the phase fails ("a helper outlived Pane quitting"). Run locally
on 2026-09-28 (Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic, x86_64, same
Xvfb/lavapipe setup): all checks of the whole smoke passed.

| Step | Evidence |
| --- | --- |
| "Echo through the helper": 'Echoed "hello from Pane" on Linux x86-64' | [90-helper-echoed.png](evidence/linux-x11/90-helper-echoed.png) |
| "Echo within a second": "Stopped the helper after one second"; no helper runs | [91-helper-cancelled.png](evidence/linux-x11/91-helper-cancelled.png) |
| "Echo after waiting" running; the helper process runs | [92-helper-waiting.png](evidence/linux-x11/92-helper-waiting.png) |
| "Disabled Helper sample"; the helper process is gone, the note kept | `93-helper-disabled.png` (not committed: the list's rows show the local checkout path) |
| "Echo after waiting" running, before Pane is quit | [94-helper-before-quit.png](evidence/linux-x11/94-helper-before-quit.png) |

The tests in `crates/pane-core/tests/helpers.rs` also end the helper by
reloading, updating, uninstalling and quitting, for the Rust, JavaScript
and TypeScript samples, and check each ended helper by its heartbeat file,
not its process id. Only Linux x86-64 ran a helper; `linux-aarch64` was
not built or run.

### Development mode (#12, #13)

The last phase ([development mode](../development-mode.md#checks)) takes a
copy of each development sample in `<output-dir>/develop-<sample>`, builds
it once with its documented command, installs it with a data folder of its
own and chooses **Develop <title>** in Manage extensions. It then edits the
greeting in the copy's source as an editor would save it and waits until
the managed copy holds the new build, and checks the answer; saves a
greeting that does not compile or type-check and checks the error and that
the old answer stays, pixel for pixel; saves twice in a row (the second
while the first builds) and checks the newer greeting; and after **Stop
developing** saves again and checks that nothing was built. The Rust sample
builds with `cargo build --release --target wasm32-wasip2` (with cargo's
JSON messages), the TypeScript and JavaScript samples with `pane_js.py`, each
into a staging folder under the phase's data folder, and the latter only
where the JS toolchain is built (not in CI's smoke, which skips them). Run
locally on 2026-09-28, after the review fixes (Ubuntu 26.04.1 LTS, kernel
7.0.0-31-generic, x86_64, same Xvfb/lavapipe setup, with the JS toolchain):
all checks of the whole smoke passed.

| Step | Evidence |
| --- | --- |
| Hello Rust as installed | [111-hello-rust-greeting-before.png](evidence/linux-x11/111-hello-rust-greeting-before.png) |
| Its source saved: "Reloaded Hello Rust", with Pane open | [112-hello-rust-rebuilt.png](evidence/linux-x11/112-hello-rust-rebuilt.png) |
| The new greeting (`--distinct` from 111) | [113-hello-rust-greeting-after.png](evidence/linux-x11/113-hello-rust-greeting-after.png) |
| A save that does not compile: "Hello Rust did not build: error[E0308]: mismatched types. It keeps running its installed code; …" | [114-hello-rust-build-failed.png](evidence/linux-x11/114-hello-rust-build-failed.png) |
| The working code still answers (`--same` as 113) | [115-hello-rust-kept.png](evidence/linux-x11/115-hello-rust-kept.png) |
| Two saves, the second during the build: the newer greeting | [117-hello-rust-greeting-fixed.png](evidence/linux-x11/117-hello-rust-greeting-fixed.png) |
| TypeScript: "Hello TypeScript did not build: src/index.ts(12,7): error TS2322: …" | [123-hello-ts-build-failed.png](evidence/linux-x11/123-hello-ts-build-failed.png) |
| TypeScript after the two saves | [126-hello-ts-greeting-fixed.png](evidence/linux-x11/126-hello-ts-greeting-fixed.png) |
| JavaScript (checked through JSDoc): "Hello JavaScript did not build: src/index.js(15,7): error TS2322: …" | [132-hello-js-build-failed.png](evidence/linux-x11/132-hello-js-build-failed.png) |
| JavaScript after the two saves | [135-hello-js-greeting-fixed.png](evidence/linux-x11/135-hello-js-greeting-fixed.png) |

Screenshots 110, 118, 119, 127, 128 and 136 (developing started and stopped, on
the extension list) are checked but not kept here: they show local package
paths; the other steps of each language (116, 120 to 122, 124, 125, 129 to 131, 133,
134) match those above.

### Disabling required dependents (#43)

A phase of its own, after the development-mode phase, with its own data folder
([disabling a required dependency](../dependencies.md#disabling-a-required-dependency)),
installs the dependencies sample with the JavaScript operations sample,
opens Manage extensions and presses Enter on the JavaScript operations
sample (the first row). Pane asks first, listing the Dependencies sample;
Down and Enter (Cancel) returns to the list with both enabled; Enter and
Enter (Disable all 2) disables both; Enter again enables the JavaScript
operations sample alone. Afterwards `installed.json` must record exactly one
disabled package. Run locally on 2026-09-28 (same Ubuntu 26.04.1 / Xvfb /
lavapipe setup): all checks of the whole smoke passed, and frames 140 to
143 were looked at.

| Step | Evidence |
| --- | --- |
| The question: "Disable JavaScript operations sample and the extensions that require it?", "Dependencies sample, which requires JavaScript operations sample", Disable all 2 selected | [140-disable-dependents-asked-masked.png](evidence/linux-x11/140-disable-dependents-asked-masked.png) |
| Disable all: both rows "Disabled", "Disabled JavaScript operations sample and Dependencies sample, which requires it" | [142-disable-dependents-disabled-masked.png](evidence/linux-x11/142-disable-dependents-disabled-masked.png) |
| Enter: "Enabled JavaScript operations sample"; the Dependencies sample stays "Disabled" | [143-disable-dependents-enabled-alone-masked.png](evidence/linux-x11/143-disable-dependents-enabled-alone-masked.png) |

(The kept frames are cropped to Pane's window and the local package paths
are painted over with the background; the smoke checks the whole frames.
Frame 141, the list after Cancel with both enabled, is checked to differ
from the others but not kept, as it shows those paths.)

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
