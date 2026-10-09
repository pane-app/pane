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

The smoke forces the dark theme and the opaque material, so its screenshots do not depend on the desktop ([smoke appearance](smoke-appearance.md)).

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

Extensions are managed in Settings since #168, where switches, menus and
confirmation rows answer the pointer only. The smoke (and
`scripts/measure-linux.sh`) opens Settings with root search's Manage
Extensions command, then finds each control by its accessible name in the
Settings window through AT-SPI and invokes it (`a11y` in the script; Pane's
tree comes from AccessKit), clicking its center with xdotool where it
offers no action. It checks an operation's outcome by the name of the
page's status line, not by a color, and closes Settings with Ctrl+W before
it goes back to the launcher. AT-SPI needs a session bus, which the smoke
starts with `dbus-launch`, and the accessibility bus on it
(`at-spi-bus-launcher`, reported enabled before Pane starts), read with the
system's `python3` and its GObject bindings: the packages `dbus-x11`,
`at-spi2-core`, `python3-gi` and `gir1.2-atspi-2.0`. The runs recorded below
predate #168: where they mention **Manage extensions…**, they drove the
launcher's extension list, which is gone. The Settings flow has not run
yet: release run 37689872256 ended the smoke before its first step, with
status 2 and no message, because the bus launcher was looked for with `ls`
over two paths, one of them missing, under `pipefail`. A switch is the
toggle button of that name; lines of plain text (a preview's details) have
no accessible name, so a preview is waited for by its row (Update,
Install). A command's answer is a toast since #141, which leaves the footer
3 seconds after it appears (run 37612772185 missed one at frame 77), so
such answers are captured until their color shows (`capture_until`).
Release run 37698693722 drove Settings through frame 19, then ended on an
X error (`BadMatch`, `X_SetInputFocus`) focusing the launcher after
Settings closed: a window is now focused only once it is viewable, and
the launcher is summoned with the Open Pane hotkey (Ctrl+Alt+Space) where
it is not shown (`focus_launcher`). The helper phase opens Settings the
shortest way, so the switch is reached within the helper's ten-second
wait (that run's macOS leg reached it after the wait had ended).

Screenshots (inspected, not machine-asserted):

| Step | Evidence |
| --- | --- |
| Root search lists the three samples, installed first into a data folder of their own (#162) | [1-root.png](evidence/linux-x11/1-root.png) |
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
| Installed; root lists the installed "Rust sample" (the samples are no built-in commands since #162), then the install row; status "Installed Rust sample" | [10-installed.png](evidence/linux-x11/10-installed.png) |
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
`pane:extension/settings`), and disables Settings sample (with the switch
on its page in Settings since #168; in **Manage extensions…** in the run
below). It checks that `installed.json` records `"disabled": true` and
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
copies the JavaScript sample over the component, reloads Dev (Reload in the
Actions menu of its page in Settings since #168; in **Manage extensions…**
in the run below) without restarting Pane, and runs "Say hello" again. Next it
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
`cache.json`, restarts, chooses Clear Cache in the Actions menu of the
settings sample's page in Settings (since #168; "Clear cache of Settings
sample" in **Manage extensions…** in the run below), confirms, and shows
what Pane keeps again. Finally it checks
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

The last phase, after the quicklinks phase, restarts Pane, chooses
Uninstall in the Actions menu of the settings sample's page in Settings
(since #168; "Uninstall Settings sample" in **Manage extensions…** in the
run below) and the first choice, "Uninstall and keep saved
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
of its own, installs the settings sample, starts Greeting's hotkey recorder
on the settings sample's page in Settings (since #168; "Hotkey for
Greeting" in Manage extensions in the run below) and presses Ctrl+Alt+G
with real X11 key events, then
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
data, then chooses "Delete retained data of Settings sample" (a row of the
Extensions group's page in Settings since #168; the extension list's last
row, confirmed with Down from the selected Cancel and Return, in the run
below) and confirms, waiting for Pane's files to change rather than a fixed
time. It checks that `installed.json` no longer has
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
alias "ec" (typed with real X11 key events in its alias cell on its page in
Settings since #168; its alias form in the run below) and makes it a
fallback (its fallback switch there), then in root search types "ec hello" (the row sending "hello" to
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
the data folder; Escape, then disabling the package (its switch in
Settings since #168; Manage extensions in the run below),
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
own and chooses Develop in the Actions menu of its page in Settings (since
#168; **Develop <title>** in Manage extensions in the run below). It then edits the
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
installs the dependencies sample with the JavaScript operations sample and
turns off the JavaScript operations sample's switch on its page in
Settings (since #168; Enter on its row in Manage extensions in the run
below). Pane asks first, listing the Dependencies sample; Cancel returns
with both enabled; the switch and Disable all 2 disable both; the switch
again enables the JavaScript operations sample alone. Afterwards `installed.json` must record exactly one
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

### Runtime crashes (#17)

A phase of its own, after the disable-dependents phase, with its own data
folder (`runtime-crash-data`, [runtime crashes](../pausing.md#when-the-extension-runtime-itself-crashes)).
It installs the helper sample and the settings sample (two extensions
active), and starts Pane with `PANE_TEST_RUNTIME_FAULTS` naming a fault
file in the output folder: writing `crash` or `crash-before-answer` there
has Pane's runtime thread panic, which is how the smoke kills it (the
runtime is a thread, not a process). Count (the settings sample's last
item) answers "Counted 1"; "Echo after waiting" starts the helper, which
`pgrep -f` finds; a crash then ends it (`pgrep` finds none and its
heartbeat stops growing), keeps its "started" note and never saves
"finished", and the status line explains the crash without naming an
extension. Count again, with the answer lost to a second crash: the count
in `content.json` is 2 and stays 2, and the runtime is not restarted.
Opening Greeting explains that nothing runs; the Extensions group's page in
Settings (Manage extensions in the run below) lists **Restart the extension
runtime** and **Why the extension runtime stopped**; the details screen
renders; disabling the helper sample
works while the runtime is stopped; Restart runs extensions again, and
Count answers "Counted 3" only when asked. `installed.json` must record the
disable and no pause, and no helper may outlive Pane. Run locally on
2026-09-28 (Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic, x86_64, same
Xvfb/lavapipe setup): all checks of the whole smoke passed, and frames 200
to 209 were looked at.

| Step | Evidence |
| --- | --- |
| The runtime crashed while the helper waited: the waiting call answers "Extension runtime unavailable: it stopped before answering and was started again; Pane does not run this again by itself" (the crash report's own line, "Pane's extension runtime stopped unexpectedly and was started again; …", shows instead when it arrives last) | [202-runtime-crashed.png](evidence/linux-x11/202-runtime-crashed.png) |
| Count's answer lost in a second crash: "… it stopped before answering and was not restarted (it crashed twice within 5 minutes; …); Pane does not run this again by itself …" | [203-runtime-stopped.png](evidence/linux-x11/203-runtime-stopped.png) |
| Manage extensions: Restart the extension runtime, Why the extension runtime stopped | [205-runtime-manage.png](evidence/linux-x11/205-runtime-manage.png) |
| The details: no extension named, what Pane did, the diagnostics, Restart | [206-runtime-details.png](evidence/linux-x11/206-runtime-details.png) |
| After Restart, Count asked again: "Counted 3" | [209-runtime-counted-again.png](evidence/linux-x11/209-runtime-counted-again.png) |

Frames 200, 201, 204 and 208 match these; 207 ("Disabled Helper sample")
is checked but not kept, as its row shows the local package path.

### Extensions that stop responding (#18)

A phase of its own, after the runtime-crash phase, with its own data
folder (`unresponsive-data`, [extensions that stop responding](../pausing.md#when-an-extension-stops-responding)).
It sets the runtime's limits through the fault file, first
`limits:60,4,15` (a minute of a guest's own computing, "not responding
yet" after 4 seconds without progress, given up on after 15), installs
the settings sample and runs **Stop responding**, which computes without
waiting: while it still computes (Pane's standard error has stopped no
call yet), Escape returns to root search and Manage Extensions opens
Settings (the launcher's list in the run below; frame 240). Then
`limits:2,4,15` stops that call at once, as it computed
longer. Run again, the call is stopped after 2 seconds of its computing
(thread CPU time) with its error; the third time
pauses the package, whose saved `busy` note stays "started"; the pause
details and Retry work. Then the fault file's `hang` blocks the runtime
thread: opening Greeting first shows that the runtime is not responding
yet, then answers that it stopped responding once Pane gave up on it,
the runtime row of the Extensions group's page in Settings (Manage
extensions' first row in the run below) opens the runtime's details, and after
`release` a fresh thread saves the formal greeting; `installed.json`
records no pause (read as JSON). Run locally on 2026-09-29 after the
second #18 review, with #46 merged (Ubuntu 26.04.1 LTS, kernel
7.0.0-31-generic, x86_64, same Xvfb/lavapipe setup): all checks of the
whole smoke passed, frame 240 was taken while the first Stop responding
still computed (no call stopped yet; Pane stopped it once the limit was
shortened after the frame), and frames 240 to 248 were looked at.

| Step | Evidence |
| --- | --- |
| The second run: "The extension stopped responding: it computed for 2 seconds without finishing, so Pane stopped it; other extensions' calls waited meanwhile" | [241-unresponsive-stopped.png](evidence/linux-x11/241-unresponsive-stopped.png) |
| The third: "Settings sample stopped responding 3 times within 5 minutes and is paused …", Greeting listed with why | [242-unresponsive-paused.png](evidence/linux-x11/242-unresponsive-paused.png) |
| The runtime thread made to hang, first: "Pane's extension runtime is not responding yet. Pane starts it again if it stays stuck; saved data is kept." | [245-unresponsive-not-yet.png](evidence/linux-x11/245-unresponsive-not-yet.png) |
| Then given up on: "Extension runtime unavailable: it stopped responding before answering and was started again; Pane does not run this again by itself" | [246-unresponsive-runtime.png](evidence/linux-x11/246-unresponsive-runtime.png) |
| The runtime's details: no progress for 15 seconds, not in an extension's code nor a host call, which code held it not known, none named or paused, the stuck thread abandoned, its last known work | [247-unresponsive-runtime-details.png](evidence/linux-x11/247-unresponsive-runtime-details.png) |
| A fresh thread: "Saved the formal greeting" | [248-unresponsive-runs-again.png](evidence/linux-x11/248-unresponsive-runs-again.png) |

Frames 240 (the extension list while the guest computes), 243 (the pause
details) and 244 ("Started Settings sample" on the extension list) are
checked but not kept, as they show the local package path.

### Uninstalling required dependents (#44)

A phase of its own, after the runtime crash phase, with its own data folder
([uninstalling a required dependency](../dependencies.md#uninstalling-a-required-dependency)),
installs the dependencies sample with the JavaScript operations sample and
chooses Uninstall in the Actions menu of the JavaScript operations sample's
page in Settings (since #168; "Uninstall JavaScript operations sample",
the seventh row of Manage extensions, in the run below). Pane asks first,
listing the Dependencies sample and the saved data of both; Cancel returns
with both installed; Uninstall again and "Uninstall all 2 and keep saved
data" uninstall both, and `installed.json`
must then hold no package. Pane started again installs the JavaScript
operations sample alone, and `installed.json` must then hold exactly one
package. Run locally on 2026-09-28 (same Ubuntu 26.04.1 / Xvfb / lavapipe
setup): all checks of the whole smoke passed, and frames 180 to 183 were
looked at. A first run showed the question's details pushing its choices
out of Pane's window; a confirmation's details now scroll within 40% of the
window, and frame 180 shows the first choice selected.

| Step | Evidence |
| --- | --- |
| The question: "Uninstall JavaScript operations sample and the extensions that require it?", "Dependencies sample, which requires JavaScript operations sample", "Uninstall all 2 and keep saved data" selected | [180-uninstall-dependents-asked-masked.png](evidence/linux-x11/180-uninstall-dependents-asked-masked.png) |
| Uninstall all: "No extensions are installed.", "Uninstalled JavaScript operations sample and Dependencies sample, which requires it; their settings and content are kept" | [182-uninstall-dependents-uninstalled-masked.png](evidence/linux-x11/182-uninstall-dependents-uninstalled-masked.png) |

(The kept frames are cropped to Pane's window and, in frame 180, the local
package paths are painted over with the background; the smoke checks the
whole frames. Frames 181, the list after Cancel, and 183, the list after
installing the JavaScript operations sample again with only it listed, are
checked to differ from the others but not kept, as they show those paths.)

### npm packages (#45)

A phase after the one uninstalling required dependents, with a data folder
of its own ([npm packages](../npm.md)),
starts `scripts/npm_registry.py` on 127.0.0.1 serving
`target/guests/npm/pane-samples-greeter-0.1.0.tgz` (the npm sample `cargo
xtask guests` packed) and points the development build at it with
`PANE_NPM_REGISTRY`; nothing reaches the network. `--install` of the local
Dependencies from npm sample previews it (frame 260: "Requires: Greeter from
npm, installed with it from npm:@pane-samples/greeter"); Enter installs both
(261) and "Greet through the required greeter" answers "Hello, Pane, from
the npm package" from the npm package's own component (262; the item's
subtitle is the Rust dependencies sample's, shared with the local
Dependencies sample). "Install extension from npm…", found by its title,
opens the npm field in Settings since #168 (a form in the launcher in the
runs below; 263), which takes
`@pane-samples/greeter`; Show Package previews it there (264) with the npm
lines and, the package being installed, **Update**, which the smoke
chooses ("Updated Greeter
from npm to 0.1.0", 265); its command's "Say hello" answers "Hello from the
npm package" (266). #49 extends the phase: a 0.2.0 of the sample is
published into the registry's folder (`scripts/npm_publish.py`; the
registry reads its folder on request), Pane is stopped and started again,
and the check a second after its start replaces the installed unpinned
copy by itself — nothing of it running — saying "Updated Greeter from npm
to 0.2.0" (267); the new copy's command answers as before (268), and
`installed.json` must record `"npmVersion": "0.2.0"`. Those frames have
not been captured: this box has no display, so they await the next smoke
run (CI's next green run is their evidence). `installed.json` must then
hold both packages and record the npm name and version. Run locally on 2026-09-29 (same Ubuntu
26.04.1 / Xvfb / lavapipe setup): all checks of the whole smoke passed and
frames 260 to 266 were looked at. A first run showed the Update row pushed
out of Pane's window by the preview's longer details; a package preview's
details now scroll within 40% of the window, as a confirmation's do. The
same phase, serving instead the tarball `npm pack` (npm 11.19.0) made of the
assembled sample folder, also passed: it holds the same four files as
Pane's own packing. After merging #30 a run failed at frame 261 with "Peer
disconnected": the smoke's registry (Python's HTTP/1.0 server) closed the
connection kept from the metadata request as the tarball's was sent on it,
about one download in forty. Pane now opens a connection per npm request;
the whole smoke then passed again and frames 260 to 266 were looked at.
After #45's review (npm through #30's HTTP client, raw tar reading, a
download folder each, the npm sample's own component answering as the npm
package), the whole smoke passed again on 2026-09-29 and frames 260 to 266
were looked at; the kept frames below are from that run.

| Step | Evidence |
| --- | --- |
| Installing the dependency from npm: "Installed Dependencies from npm sample with Greeter from npm, which it requires" | [261-npm-dependency-installed.png](evidence/linux-x11/261-npm-dependency-installed.png) |
| The npm package's `greet`, called by the dependency id: "Hello, Pane, from the npm package" | [262-npm-dependency-called.png](evidence/linux-x11/262-npm-dependency-called.png) |
| The preview: "Source: npm package @pane-samples/greeter", "npm version: 0.1.0, the latest", the tarball and its sha512 integrity, what Pane runs, and Update in view | [264-npm-preview.png](evidence/linux-x11/264-npm-preview.png) |
| Its command: "Hello from the npm package" | [266-npm-command-ran.png](evidence/linux-x11/266-npm-command-ran.png) |

(The kept frames are cropped to Pane's window; the smoke checks the whole
frames. Frame 260 shows the local checkout path and is not kept.)

### Git packages (#46)

A phase after the npm one, with a data folder of its own ([Git
packages](../git.md)), makes the Git sample's repository with
`scripts/repository_server.py make-sample` from
`target/guests/git/greeter` (the source on `main`, the build on the branch
`release`, tagged `v0.1.0`; `git` runs in the script with none of the user's
configuration, never in Pane) and serves it on 127.0.0.1 with
`scripts/repository_server.py serve`, which answers each request with `git
upload-pack --stateless-rpc`; nothing reaches the network. `--install
git:http://127.0.0.1:<port>/greeter.git` (the default branch) is explained
as source-only, with nothing offered (frame 300, captured again every half
second until the explanation's color shows, for up to 60 s: "The default branch, main
(commit …) of the Git repository 127.0.0.1:<port>/greeter holds only the
source of "Greeter from Git" …"). Escape, then "Install extension from
Git…", found by its title, opens the Git field in Settings since #168 (a
form in the launcher in the runs below; 301), which takes
`<address>@v0.1.0`; Show Package previews it there (302): "Source: Git
repository 127.0.0.1:<port>/greeter",
"Revision: tag v0.1.0, which you named: installing pins it to that
revision", the commit fetched ("Fetched: commit … “Release 0.1.0”, served at
http://127.0.0.1:<port>/greeter.git; …") and "Pane builds nothing and runs
no repository hooks, …", with Install in view below them (a preview's details
may take 62% of the window since the review, so none is cut); Install installs it
(303, "Installed Greeter from Git") and its command's "Say hello" answers
"Hello from the Git repository" (304). `installed.json`, read as JSON
(`scripts/check_git_record.py`), must then record one package from Git with
`"gitRef": "refs/tags/v0.1.0"`, `pinned` and the `gitCommit` the tag points
to (`scripts/repository_server.py commit`), and `extensions/downloads/` must
be empty. Run locally on 2026-09-29 (same
Ubuntu 26.04.1 / Xvfb / lavapipe setup): the phase passed on its own first,
after a first run found the form row one row lower than written (no Manage
extensions… row in a fresh data folder); then the whole smoke passed and
frames 300 to 304 were looked at. After the review, with #36 merged, the
whole smoke passed again, frames 300 to 304 were looked at, and 300 and 302
were recorded again: 302 now shows every Git line whole above Install (304
was unchanged). After the second review the whole smoke passed again and
300 and 302 were recorded once more: 302 shows the reworded last Git line.

#50 extends the phase with a second repository of the same sample
(`make-sample` again, `greeter-tracked` beside `greeter`, served by the
same server, which answers each request from the folder as it is),
installed in a data folder of its own from its tracked `release` branch
(`--install git:<address>@release`: the preview says "Revision: branch
release, tracked: an update fetches that branch again", 305; installed,
306); `repository_server.py move-sample` then commits a 0.2.0 on the
branch while Pane is stopped, and the check a second after the restart
replaces the installed copy by itself — nothing of it running — saying
"Updated Greeter from Git to 0.2.0" (307, captured until it shows), the
new copy's command answering as before (308); `installed.json` must
record the branch's new commit, tracked and unpinned. Those frames await
their first run (the next smoke run, local or CI's).

| Step | Evidence |
| --- | --- |
| The default branch, source only: explained, nothing installed | [300-git-source-only.png](evidence/linux-x11/300-git-source-only.png) |
| The preview of the tag: its identity, the pinned revision, the commit fetched and Install in view | [302-git-preview.png](evidence/linux-x11/302-git-preview.png) |
| Its command: "Hello from the Git repository" | [304-git-command-ran.png](evidence/linux-x11/304-git-command-ran.png) |

(Cropped to Pane's window, as above; no frame of this phase shows a local
path.)

### Files (#29)

Since #175 Files answers from Pane's file index: the phase installs Files
(220 is now `220-files-installed`, "Installed Files"), whose index covers
the fixture folder a debug build's `PANE_TEST_FILE_INDEX_HOME` names, with
no folder to choose; "plan" lists the file under "Files" in root search;
and Return on the script shows its folder in the file manager (ADR 0037)
instead of refusing it, so 224 is `224-files-program-revealed`. Files
installed from its folder is not the registered default, so its Search
Files command keeps the generic list (#177), which the smoke does not
open. The run recorded below is of the earlier, granted-folder flow:

The last phase, with its own data folder ([files](../files.md)), makes a
fixture folder `/tmp/pane-smoke-files.XXXXXX/Pane smoke files` (spaces;
outside the home folder, so no frame shows a home path) holding "Résumé
plan ü.txt", `notes/todo.txt` and an executable `notes/runner.sh`, installs
Files, opens its command and presses Return on Pane's own "Choose folder…"
row; a debug build's `PANE_TEST_CHOOSE_FOLDER` names the folder instead of
showing the system's picker. It then returns to root search, types "plan"
and presses Return. `xdg-open` runs with no desktop session variables,
XDG_CONFIG_HOME and XDG_DATA_HOME in the smoke's output folder whose
`mimeapps.list` makes a recording script the only handler for `text/plain`
(checked with `xdg-mime query default`), and BROWSER the same script, so no
program of the user's opens the file; the path the script received,
resolved, must be the fixture file's, resolved. Last it types "runner" and
presses Return: Pane must refuse the script, hand nothing to the handler,
and the script must not run. Run locally on 2026-09-28 (Ubuntu 26.04.1 LTS,
kernel 7.0.0-31-generic, x86_64, same Xvfb/lavapipe setup): all checks of
the whole smoke passed, and frames 220 to 223 were looked at.

| Step | Evidence |
| --- | --- |
| "Choose folder…": "Files may now list “Pane smoke files”" | [220-files-folder-granted.png](evidence/linux-x11/220-files-folder-granted.png) |
| "plan" typed: "Résumé plan ü.txt", "File in Pane smoke files", selected | [221-files-found.png](evidence/linux-x11/221-files-found.png) |
| Return: "Opened Résumé plan ü.txt", the path received by the handler | [222-files-opened.png](evidence/linux-x11/222-files-opened.png) |
| "runner", Return: "Could not open runner.sh: it is a program or script…" | [223-files-program-refused.png](evidence/linux-x11/223-files-program-refused.png) |

The system's folder picker (the XDG portal) and a real desktop's handler
(GNOME's `gio open`, a text editor) were not run; cancelling, the slow
listing and the re-checks at Enter are checked by the launcher tests, not
natively.

### Searching inside a command (#30)

The last phase ([command search](../command-search.md#checks)), with a data
folder of its own, builds and starts the fixture service on a free port of
127.0.0.1 (`fixture_service --port 0`, its log in the smoke's output, the
port read from it) and installs Package search, the Rust search sample.
"aurora" typed in root search lists nothing and the service's log must hold
no request; opened, the command's "Service address" form is set to the
service; its own search field (the same query field) sends "aurora" (the
log must hold `GET /search?q=aurora`), Down and Enter show aurora-cli's
details; "slow" (held by the service) then "ember" must show ember-tz and
log `ABANDONED /search?q=slow`; "down" shows the service's 503 as an error;
with the service stopped, "basalt" shows "connection refused"; restarted on
the same port, "cobalt" lists cobalt-http: the extension was not paused.
Run locally on 2026-09-29 after the #30 review (Ubuntu 26.04.1 LTS, kernel
7.0.0-31-generic, x86_64, same Xvfb/lavapipe setup): all checks of the
whole smoke passed, and frames 160 to 169 were looked at.

| Step | Evidence |
| --- | --- |
| Root search, "aurora": no results, nothing sent | [161-root-typed.png](evidence/linux-x11/161-root-typed.png) |
| Package search opened: its own list, its search field empty | [162-command-opened.png](evidence/linux-x11/162-command-opened.png) |
| Its "Service address" form set to the fixture service's free port | [163-service-set.png](evidence/linux-x11/163-service-set.png) |
| "aurora": the service's results | [164-search-results.png](evidence/linux-x11/164-search-results.png) |
| Enter: aurora-cli's details, fetched from the service | [165-details.png](evidence/linux-x11/165-details.png) |
| "slow" replaced by "ember": the newer results | [166-newer-search.png](evidence/linux-x11/166-newer-search.png) |
| "down": the service's 503 as an error | [167-service-error.png](evidence/linux-x11/167-service-error.png) |
| Service stopped: "connection refused" | [168-offline.png](evidence/linux-x11/168-offline.png) |
| Service back: results again | [169-back-online.png](evidence/linux-x11/169-back-online.png) |

### Clipboard history (#35, #38)

The X11 adapter ([`clipboard/linux.rs`](../../crates/pane-core/src/clipboard/linux.rs))
watches the `CLIPBOARD` selection through XFIXES on a thread of Pane's own
and reads each new owner's text through a selection transfer; no permission
is needed. It was written, compile- and lint-checked, and its pure parts
unit-tested (the session's refusals, Latin-1, the `WM_CLASS` and `/proc`
reads) on this machine; the machine has no display, so the real-X11 test
([`clipboard_adapter_linux.rs`](../../crates/pane-core/tests/clipboard_adapter_linux.rs),
run by CI under `xvfb-run` with `PANE_TEST_REAL_CLIPBOARD=1`) and the smoke
phases have **not run locally**. CI's next green Linux run of the branch is
their evidence: until then the watcher's delivery, the selection transfers
(including `INCR`), the owner lookup and the write-back through Pane's own
serving window are unverified natively.

The smoke's clipboard phase (screenshots 280 to 287, [clipboard
history](../clipboard-history.md#checks)), with a data folder of its own,
runs after the #53 phase: since #166 only Pane's registered Clipboard
History records from the first start, so the phase acquires the default
set from the artifacts that phase built, served on 127.0.0.1, with the
smoke's own build (Files' index on an empty folder,
`PANE_TEST_FILE_INDEX_HOME`). It checks `clipboard-history.json` at each
step: the smoke's own copies are kept with nothing turned on, newest
first; nothing is kept while paused (Pause Recording in the split view's
Actions panel, Ctrl+K), or while disabled (the switch on its page in
Settings), also after a restart; once enabled again, text is kept, also
after a restart, before the command is opened; Return on a record (the
view's filter leaving it) copies it again, as Pane cannot paste here yet,
and the record moves to the front, the pasted text proving the clipboard
holds it. The smoke copies only its own `pane-smoke-...` text, by typing it into
root search and copying it with Ctrl+A and Ctrl+C through the window's X11
clipboard (Xvfb is the smoke's own display), and so replaces what is on the
clipboard without reading or putting it back. X11 has no marker formats a
password manager could set, so no marked copy is checked; a copy no text can
be read from and an unknown owner are the adapter test's checks.

| Step | Evidence |
| --- | --- |
| The split view: Today, the two records kept with nothing turned on | 280-clipboard-recording.png |
| Pause Recording, in the Actions panel: "Recording paused" | 281-clipboard-paused.png |
| Three records, newest first | 282-clipboard-kept.png |
| Return on a record: copied again, the window closed with a HUD | 283-clipboard-copied.png |
| Pasting into root search shows the copied record | 284-clipboard-pasted.png |
| Typing the same text (the same frame) | 285-clipboard-typed.png |
| Settings: "Disabled Clipboard History", after which nothing is kept | 286-clipboard-disabled.png |
| Kept again across a restart, before the command is opened | 287-clipboard-after-restart.png |

(Cropped to Pane's window; the smoke checks the whole frames. The frames
will be kept under `evidence/linux-x11/` once they have run, as the earlier
ones are.)

### Clipboard history expiry (#36)

The phase goes on (screenshots 400 to 407, [clipboard
history](../clipboard-history.md#checks)) with the history it kept: with
Pane stopped, the smoke makes `pane-smoke-kept` 8 days old and
`pane-smoke-enabled` 2 hours old (`scripts/clipboard_history.py`), as a
downtime would; once Pane starts, the first is gone from the file and the
list before the command shows anything. Then, in the split view's Actions
panel (#166): Delete Entry on `pane-smoke-second` (the filter leaving it)
deletes that record alone; keeping records for 1 Hour deletes
`pane-smoke-enabled` at once; after one more copy, Clear History, once
confirmed, deletes every record while recording goes on (`capture` stays
`on`), a later copy is kept, and pasting into root search still shows the
copied text, so the clipboard still holds it (Windows reads the clipboard
API directly; on Linux, where reading means a paste, this is checked once,
at the end, as after the copy in phase 280 to 287). No file is copied here
(#167): nothing on this session offers one, and the adapter test checks
`text/uri-list`. The forms #36 added (Delete recent items, Turn off and
delete) are gone since #166. As the #35 phase, **not run locally**; CI's
next green Linux run is its evidence.

| Step | Evidence |
| --- | --- |
| After the restart, the 8-day-old record is gone | 400-clipboard-expired.png |
| "Deleted the kept item" | 401-clipboard-item-deleted.png |
| "History is kept for 1 hour; deleted 1 kept item older" | 402-clipboard-retention-changed.png |
| "Clear Clipboard History?", asked over the view | 404-clipboard-clear-asked.png |
| Cleared, recording going on | 405-clipboard-cleared.png |
| Pasting into root search still shows the copied text | 406-clipboard-still-held.png |
| Typing the same text (the same frame) | 407-clipboard-held-typed.png |


### Installing Pane and acquiring its calculator (#53)

A final phase, after the clipboard-expiry one, proves the whole outcome of
[#53](https://github.com/pane-app/pane/issues/53)
([installer](../installer.md)). `cargo xtask package-linux --dev` builds the
Linux package (the development profile, so its program accepts the
controlled artifact source) and the default extensions' payloads; the
smoke serves `target/dist/artifacts` from 127.0.0.1 with
`scripts/artifact_server.py` (nothing reaches the network or Pane's
published downloads). The package is unpacked into a folder of its own and
its install script runs with a **clean machine's** environment: a fresh
home folder and `PATH=/usr/bin:/bin`, so the home holds no data and no
development tool is configured. The installed Pane
(`~/.local/bin/pane` of that home) then starts with a PATH that holds
nothing at all (an empty folder, checked by `command -v` of cargo, rustc,
node, npm, git, cc, clang and make, and by reading the running process's
`PATH`), pointed at the controlled source with `PANE_ARTIFACTS`. It
acquires the five default extensions by itself (`installed.json` must
record each under `"default"`, and no sample: the helper sample left the
default set with #162, and a helper running from an acquired payload is
`crates/pane-core/tests/installer.rs`'s), root search lists their
commands, "6*7" answers 42 and Enter copies it, with no developer tool
reachable. The acquired payloads must be cached and the downloads folder
empty. The program files are removed again at the phase's end, so the
uploaded evidence is the screenshots and records (frames 500 to 502),
not the program.

**Recorded 2026-09-29, this machine (Ubuntu aarch64, no display — the GUI
cannot run here):** what ran locally is everything short of the GUI: the
packaging task in both profiles, the install script into a temporary home
with a scrubbed PATH (`pane --version` answered), and the acquisition and
its recovery through the launcher's public interface
(`crates/pane-core/tests/installer.rs`, ten checks, all passing, including
the helper running from the managed copy — this machine's payload names
`linux-aarch64`, which CI's x86_64 runner builds as `linux-x86_64`). The
smoke phase itself, and the release-profile package on x86_64, are
**pending CI**: they need the Xvfb smoke and the x86_64 build only CI's
runner provides.

| Step | Evidence |
| --- | --- |
| The clean machine's Pane acquired the five default extensions, and no sample (#162), and lists their commands | pending CI (frame 500) |
| "6*7" answers 42 | pending CI (frame 501) |
| Enter copies the answer | pending CI (frame 502) |


### Installing a Pane application update by the user's choice (#56)

A final phase, after the installer one, proves the whole outcome of
[#56](https://github.com/pane-app/pane/issues/56)
([installer](../installer.md)), the Linux half of what
[#54](windows.md#installing-a-pane-application-update-by-the-users-choice-54)
proved on Windows. The same `cargo xtask package-linux --dev` builds a
**second** package with `--package-version 99.0.0`: a program that
reports 99.0.0, a package named by it (the gzipped tarball the Linux
package is, holding `pane/`), and an index whose `application` entry
names that package for this system — the two runnable builds an update
goes between. The 0.1.0 package the installer phase built is installed
on another clean home (its install script, its empty PATH, its own data
under `~/.local/share/pane`), and the smoke serves the 99.0.0 artifacts
from 127.0.0.1 with `scripts/artifact_server.py` (nothing reaches the
network or Pane's published downloads). The installed 0.1.0 Pane,
started with `PANE_ARTIFACTS`, acquires its default extensions and, in
the same background, checks the index for a newer version of itself: the
offer appears as **Update Pane to 99.0.0** in root search (frame 601;
the status line tells what was found, frame 600). The artifact server's
log must hold **no request for the package** until the row is chosen —
nothing is downloaded, installed or restarted automatically. Clipboard
History is disabled first (the Helper sample was, until #162 took it out
of the default set), so an extension the user disabled before the update
must stay disabled after it. Choosing the row with a **damaged
package** is explained (its bytes do not match the sha512 its entry
gives, frame 602) with the program, the data and the bin folder
untouched and the row ready to try again; then the real choice
downloads the tarball, checks it, unpacks it (the same tar reader npm
tarballs are read by, with the application package's own size bound)
and swaps the running program — the old `~/.local/bin/pane` renamed
`pane.old` beside it, the new one in its place, the staging folder gone
(frame 603, and byte comparisons of both programs against the two
packages' own files). The next start runs the new version: it reports
`Pane 99.0.0`, removes `pane.old` at start, the calculator still
answers "6*7" with 42 from the old version's acquired payload (frames
604 and 605), and the disabled Clipboard History stays disabled — Pane's
data was never touched. The desktop entry the install script put in
`~/.local/share/applications` keeps naming the same program file, which
the swap leaves in place, so it never points anywhere else. Pane itself
was never restarted by the update: the smoke stops the old process and
starts the new program itself, exactly as the user would.

**Recorded 2026-09-30, the machines that wrote it (a headless aarch64
Linux box, then a Windows one):** nothing of this phase has run — no
Apple-less-aarch64 or Windows machine can run the Linux GUI smoke. What
ran is the platform-independent half (`crates/pane-core/tests/application_update.rs`,
including the new tarball-install test, all passing where it ran) and
`bash -n` on the smoke script; the phase itself is **pending the Linux
smoke run** (CI's `ubuntu-24.04` leg with Xvfb, or a local Linux box):
its frames, the tarball swap and the running-program rename it depends
on (renaming a running program's file is allowed on Linux as on
Windows; overwriting one is not) are proven only there.

| Step | Evidence |
| --- | --- |
| The check at start tells the user; nothing is downloaded until they choose | pending the Linux smoke run (frames 600, 601) |
| A damaged package is explained, everything untouched, the row retried | pending the Linux smoke run (frame 602) |
| The user's choice swaps the running program; the new one is used the next start | pending the Linux smoke run (frame 603) |
| The new version reports itself; the old version's data and enablement are kept | pending the Linux smoke run (frames 604, 605) |

### Resource and latency measurements (#4)

The [documented workload](../research/resource-measurements.md)
(`scripts/measure-linux.sh`, the smoke's runner, guests and binary) runs
Pane through cold and warm start, an idle core, Pane hidden with its
default extensions (#189), seven installed-but-unused extensions, repeated
calculator use, the Watching service and the Counting schedule, and
repeated reload and disable, sampling Pane's whole process tree and the
wake-ups of each of its threads from `/proc` at 1 s into a
machine-readable record. It is wired into
CI's Linux leg, its record uploaded as the `resource-measurements`
artifact, and its summary checked against the proposed targets
(`scripts/resource-targets.json`), which are **all pending numbers**: this
combination has not run it yet (this machine cannot; see the record), no
target is inferred from the older CLI peaks, and Q3's budgets stay open
until the first record is collected and the user confirms ceilings.

| | |
| --- | --- |
| The workload, sampler, targets file and CI wiring | [the measurement record](../research/resource-measurements.md) |
| This combination's numbers | pending the next CI Linux run (`measure/summary.json` in the artifact) |

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
- The HUD's window is placed where the specification has it (centred, its
  bottom edge 150 logical pixels above the monitor's bottom) on Windows,
  macOS and X11; Wayland lets a client place no such window, so there the
  HUD reads as a toast-like message the compositor places (the
  specification's fallback, `crates/pane/src/features/hud.rs`).
- Clipboard history works through X11 only: on Wayland (with or without
  XWayland) it says why it cannot watch, and only the X11 combination
  (Xvfb in CI) is claimed; a real desktop's programs, password managers and
  selection transfers have not been run.
- aarch64 has now built and checked headlessly (the #53 package and its
  acquisition, above), but no aarch64 GUI has run: Xvfb, the smoke and the
  window tests on aarch64 remain untested, and the prebuilt-helper fixture
  the smoke installs is built for the system that builds it.
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
The upstream repository `pane-app/pane` still has no configured runner.
