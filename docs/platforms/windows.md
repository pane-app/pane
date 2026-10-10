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

The smoke forces the dark theme and the opaque material, so its screenshots do not depend on the desktop ([smoke appearance](smoke-appearance.md)).

```powershell
cargo build -p pane
./scripts/smoke-windows.ps1 -OutDir smoke   # needs Python 3 with Pillow
```

The script launches `target\debug\pane.exe` and waits for its main window. It
then brings the window to the foreground and sends real key events with
`SendKeys`. For each of the Rust, JavaScript and TypeScript sample commands it
presses Enter to open it, Down and Enter to run "Wait briefly" (an async WASI 0.3
clock import inside the guest), then Escape.

Extensions are managed in Settings since #168, where switches, menus and
confirmation rows answer the pointer only. The smoke opens Settings with
root search's Manage Extensions command, then finds each control by its
accessible name in the Settings window through UI Automation
(`UIAutomationClient`; Pane's tree comes from AccessKit) and toggles or
invokes it, clicking its center where it offers no pattern. It checks an
operation's outcome by the name of the page's status line, not by a
color, and closes Settings with Ctrl+W before it goes back to the
launcher. Lines of plain text (a preview's details) have no accessible
name, so a preview is waited for by its row (Update, Install). Release run
37689872256 drove Settings this way through frame 68; it failed at frame
69, Echo's answer: a command's answer is a toast since #141, which leaves
the footer 3 seconds after it appears, and the fixed 3-second wait
captured the screen after it had left. Such answers are now captured
until their color shows (`Capture-Until`), from half a second after the
key. Release run 37698693722 then reached the Files phase, where an
Escape after installing Files hid the launcher (the install lands on a
blank root search, where Escape hides it), so frame 221 showed the
desktop; the return to root key (Shift+Escape) is pressed there instead.

It fails if the window does not appear or Pane exits. The same three screenshot
checks as on macOS and Linux also run (`scripts/check_screenshot.py`):

- The root screen draws text in the hint color.
- Each result screen draws text in the result color.
- The three result screens are all different.

Screenshots, cropped to the window (and inspected):

| Step | Evidence |
| --- | --- |
| Root search lists the three samples, installed first into a data folder of their own (#162) | [1-root.png](evidence/windows/1-root.png) |
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
The smoke's search phase (screenshots 24 to 26) types "typescr" with
`SendKeys`, opens the only match and runs "Wait briefly", which must look
exactly like step 4, then types "zzz" and presses Enter on no results. In run [36420611977](https://github.com/wasimysaid/pane/actions/runs/36420611977) (commit `6d73d18`) every step passed: typing "typescr" left only TypeScript sample and Enter ran it, and "zzz" showed No results ([24-search.png](evidence/windows/24-search.png), [26-no-results.png](evidence/windows/26-no-results.png)). Input-method composition in the query field is still unverified here.

## Calculator (#27)

The smoke's installer phase (screenshots 501 and 502) covers the
calculator: set up on a clean machine from the commit this release pins
(its repository cloned at that commit and served on 127.0.0.1), typing
"6*7" checks the selected answer row and Enter copies the answer, with
no developer tool anywhere. (A by-hand calculator phase, whose
screenshots 27 to 30 below are from, installed the package from this
repository's guests tree and also checked the clipboard round-trip; it
left with the extension's sources, #285, its arithmetic being its
repository's to test.) In that phase's run [36423871204](https://github.com/wasimysaid/pane/actions/runs/36423871204) (commit `ab91081`) every step passed: "6*7" answered 42, Enter copied it, and pasting then typing "+1" matched typing "42+1", so the system clipboard held "42" ([27-answer.png](evidence/windows/27-answer.png), [30-pasted.png](evidence/windows/30-pasted.png)).

## Applications (#24)

[Applications](../applications.md) finds the `.lnk` shortcuts in the user's
and all users' Start menu Programs folders and opens one with
`ShellExecuteEx`, as Explorer does, plus the packaged (AppX/MSIX) apps of
the shell's Apps folder, such as Calculator on Windows 11, opened by their
AppUserModelID; a native test requires an inbox packaged app (Calculator or
Settings) to be found. The smoke's
last phase (screenshots 44 and 45) makes a shortcut "Pane Smoke App" to
`cmd.exe` writing a marker file (with `WScript.Shell`, minimized) under an
APPDATA given to Pane only, installs the JavaScript applications sample
(which supplies the host's applications to root search as the
[Applications](../applications.md) default extension does), types "pane
smoke", checks the selected row, presses Enter and checks "Opened Launch
Pane Smoke App"
and the marker; the adapter tests open such a shortcut too. **Not run on
Windows yet**: this branch was not pushed, so the phase, the native tests
and the `ShellExecuteEx` path are unverified here (the Windows code was
only type-checked and linted for `x86_64-pc-windows-gnu`).

## Opening a web link (#28)

The smoke's quicklinks phase, which installed the Quicklinks package and
created a quicklink in its form, left with the extension's sources
(#285, its quicklinks being its repository's to test); the host's link
opening is the Linux smoke's web-link phase, which runs the actions
sample's Open Website through the system's handler. Opening a link on
Windows is checked only through the tests' recording opener.

## Global hotkeys (#32)

The smoke's hotkey phase (screenshots 52 to 58, [global hotkeys](../hotkeys.md#checks))
assigns Ctrl+Alt+G to Greeting in the hotkey recorder on the settings
sample's page in Settings (#168; it was the launcher's hotkey screen
before), minimizes Pane,
presses it with `SendKeys` and checks that Pane's window is the foreground
window again with Greeting open; then again after a restart; then, with the
extension disabled, that pressing it leaves Pane minimized and unchanged.
`RegisterHotKey` needs no permission. `hotkey_adapters.rs` registers a
shortcut on the test session, checks that a second registration is refused
as taken and that the released shortcut can be registered again. The
adapter was only compile- and lint-checked for `x86_64-pc-windows-gnu` from
Linux; **not run on Windows yet**, so registration, delivery and the focus
transition (foreground rules) are unverified natively.

## Deleting retained data (#41)

The smoke's retained-data phase (screenshots 63 to 65, [deleting retained
data](../extension-data.md#deleting-retained-data)), with a data folder of
its own, saves a note with the settings sample, uninstalls it keeping its
saved data, deletes its retained data from its row on the Extensions
group's page in Settings and confirms (#168), checks that `installed.json`
and `content.json` no longer hold it, and reinstalls the same folder, which
must show nothing kept. **Not run on Windows yet.** A data file locked by
another program is covered only by the tests' unreadable files, and a
record that cannot be written after the data is deleted only by a Unix test.

## Aliases and fallbacks (#31)

The smoke's alias phase (screenshots 66 to 74, [aliases and fallbacks](../aliases.md#checks))
gives Echo, the query sample's command, the alias "ec" and makes it a
fallback on its extension's page in Settings (its alias cell and fallback
switch, #168), sends "ec hello" and "zqx" to it — the first fallback
preselected when nothing else matches (ADR 0031), so Enter sends "zqx"
without a move — and checks that with the
extension disabled "ec hello" lists nothing. Nothing in it is specific to
Windows (no system API is involved); **not run on Windows yet**.

## Dependencies (#42)

The dependencies phase (screenshots 75 to 77, [dependencies](../dependencies.md#checks)),
with a data folder of its own, previews the dependencies sample (its
required JavaScript operations sample and optional Rust one listed),
installs it with the JavaScript sample and runs "Greet through the required
greeter", which must answer from the JavaScript guest; `installed.json`
must then hold exactly two packages and the recorded dependency. A
dependency's `local:../…` source is joined to the package folder and
resolved with the same `canonicalize` (without the `\\?\` prefix) as
package identity; a folder that does not exist is resolved from its
spelling. **Not run on Windows yet**, so `..` across drive-letter and UNC
paths is untested natively.

## Native helpers (#15)

The first native-helper phase (screenshots 90 to 93, data folder `helper-data`,
[native helpers](../helpers.md#checks)) installs the helper sample, whose
`pane-echo.exe` `cargo xtask guests` builds for the runner
(`windows-x86_64` on `windows-2025`), runs it (the answer must name Windows
x86-64), cancels a slow run after a second, starts the ten-second run,
checks with `Get-Process` that the helper runs from the managed copy,
disables the package and checks that the process is gone, that the saved
"started" note is kept, and that no helper outlives Pane. A second phase
(screenshot 94, `helper-quit-data`) starts the waiting helper, closes
Pane's window with `CloseMainWindow` (WM_CLOSE), and checks that Pane
exits, no helper runs and its heartbeat file stops growing. The tests in
`crates/pane-core/tests/helpers.rs` (Rust, JavaScript and TypeScript
samples; a PE header) and the runner's unit tests (the `.exe` rule and
absolute path are checked on every system) run in `cargo xtask ci` there,
against the `pane-echo.exe` built natively on the runner. Pane
starts a helper without a console window (`CREATE_NO_WINDOW`) and ends it
with `TerminateProcess`; a helper's own children are not in a job object.
The runner was only compile- and lint-checked for `x86_64-pc-windows-gnu`
from Linux; **not run on Windows yet**, so starting, ending and reaping a
helper natively, and the smoke's process checks, are unverified there. An
update ends the old copy's helpers before removing its folder, so the
folder is not in use; the removal at the next start remains a fallback.
No Windows arm64 build was made.

## Development mode (#12, #13)

The smoke's last phase (screenshots 110 to 136, [development
mode](../development-mode.md#checks)) builds a copy of each development
sample, develops it from its page in Settings (Develop in the Actions
menu, #168), saves an edit, a change that
does not build, two saves in a row and, after stopping, one more, checking
the answers, the error and that nothing is built after stopping. The
TypeScript and JavaScript samples run only where the JS toolchain is built,
so CI's smoke runs the Rust one. The platform code (`ReadDirectoryChangesW` through notify, a Job Object
with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, `CREATE_NEW_PROCESS_GROUP` and
builds without a console window) was only compile- and lint-checked for
`x86_64-pc-windows-gnu` from Linux; **not run on Windows yet**, so the file
watcher's events, the build's processes being killed and the whole phase
are unverified natively.

## Disabling required dependents (#43)

The disable-dependents phase (screenshots 140 to 143, [disabling a required dependency](../dependencies.md#disabling-a-required-dependency)),
with a data folder of its own, installs the dependencies sample, turns off
the JavaScript operations sample's switch on its page in Settings (#168),
which must ask first (its Disable all row shown), cancels, then chooses
Disable all (the page's status says so) and enables the JavaScript sample
again alone; `installed.json` must then record exactly one disabled package.
Nothing in it is specific to Windows (no system API is involved; the
closure reuses the dependency identities recorded at install); **not run on
Windows yet**.

## Runtime crashes (#17)

The runtime-crash phase (screenshots 200 to 209, data folder
`runtime-crash-data`, [runtime crashes](../pausing.md#when-the-extension-runtime-itself-crashes))
installs the helper and settings samples, starts Pane with
`PANE_TEST_RUNTIME_FAULTS` naming a fault file, runs the settings sample's
Count, starts the waiting helper and has the runtime crash: the helper must
be gone (`Get-Process`, and its heartbeat must stop growing), the note it saved kept,
and the status line the error color. A second crash, injected before
Count's answer, must leave the count at 2 and the runtime stopped; root
search explains it, the Extensions group's page in Settings shows why (its
runtime rows, and the details behind one), a disable works, **Restart the
extension runtime** runs extensions again and
Count then counts 3; no package may be recorded as paused. Nothing in it is
specific to Windows (the runtime is a thread; helpers are ended as for a
disable); **not run on Windows yet**.

## Extensions that stop responding (#18)

The unresponsive phase (screenshots 240 to 248, data folder
`unresponsive-data`, [extensions that stop responding](../pausing.md#when-an-extension-stops-responding))
sets the runtime's limits through the fault file, first `limits:60,4,15`
(a minute of a guest's own computing), installs the settings sample and
runs its **Stop responding**, which computes without waiting: while it
still computes (Pane's standard error has stopped no call yet), Escape and
Manage Extensions must answer (Settings opens, frame 240); then
`limits:2,4,15` must stop that call at once, as it computed longer; run
again, the call must be stopped after 2 seconds of its own computing (thread CPU time from
`GetThreadTimes` on Windows, `CLOCK_THREAD_CPUTIME_ID` on macOS) with the
error color, and the third time pause the package (the error and reason
colors), with the saved `busy` note still "started"; the pause details and
Retry must work. Then the runtime thread is made to hang through the fault
file (`hang`): opening Greeting must first show "not responding yet" (the
progress color, frame 245), then the error color after Pane gave up on the
thread, the runtime row of the Extensions group's page in Settings must
open the runtime's details, and after `release` a fresh thread must save
the formal greeting; no package may be recorded as paused. Epoch
interruption and the watchdog are Wasmtime's and Pane's own, with nothing
specific to Windows; **not run on Windows yet**.

## Uninstalling required dependents (#44)

A phase of the smoke (screenshots 180 to 183, [uninstalling a required dependency](../dependencies.md#uninstalling-a-required-dependency)),
with a data folder of its own, installs the dependencies sample, chooses
Uninstall in the Actions menu of the JavaScript operations sample's page in
Settings (#168), which must ask first, cancels, then chooses Uninstall all 2
keeping saved data (the page's status says so); `installed.json` must then hold
no package. Pane is started again to install the JavaScript operations
sample alone, and `installed.json` must then hold exactly one package.
Removing a managed copy uses the same `remove_dir_all` as a single
uninstall; a folder still in use (a file open on Windows) is listed and removed at the next start,
reported against its own package. Nothing else in it is specific to
Windows; **not run on Windows yet**.

## npm packages (#45)

A phase of the smoke (screenshots 260 to 268, [npm packages](../npm.md)),
with a data folder of its own, starts `scripts/npm_registry.py` (with
`python`) on 127.0.0.1 serving the npm sample `cargo xtask guests` packed,
and points the development build at it with `PANE_NPM_REGISTRY`: it installs
the local Dependencies from npm sample, which downloads and installs the npm
package it requires, calls its `greet` operation, then names the npm package
in the npm field "Install extension from npm…" opens in Settings (#168;
SendKeys types `@pane-samples/greeter`, and Show Package previews it there),
updates it and runs its command, which answers
"Hello from the npm package". #49 extends the phase: a 0.2.0 of the
sample is published into the registry's folder (`scripts/npm_publish.py`;
the registry reads its folder on request), Pane is stopped and started
again, and the check a second after its start replaces the installed
unpinned copy by itself — nothing of it running — saying "Updated Greeter
from npm to 0.2.0" (267), the new copy's command answering as before
(268); `installed.json` must then record `"npm":
"@pane-samples/greeter"` at `"npmVersion": "0.2.0"` with both packages.
Unpacking refuses, on every system alike, the names Windows reads
differently or cannot write: `\ : < > " | ? *`, control characters,
trailing dots and spaces, and device names such as `con`, `conin$`,
`conout$`, `com1` or `lpt³` (compared by character, with any extension), so
a tarball Linux accepts never fails or writes elsewhere on Windows; the
real registry would be reached through the HTTP client guests' requests
use, trusting the certificates rustls-native-certs reads from the Windows
certificate store, which no check exercises (the smoke never reaches the
network; [by hand](../npm.md#trying-the-real-registry-by-hand), not run). The packing, in `cargo xtask guests`, runs in CI on
Windows. After the macOS smoke's first run ([macOS](macos.md#npm-packages-45)),
the registry and the Git phase's repository server skip a reverse DNS
lookup before they listen, and the smoke waits up to 60 s for their port
files, stopping early if a server exits; on Windows that change was checked
by reading only. **Not run on Windows yet.**

## Git packages (#46)

A phase of the smoke (screenshots 300 to 304, [Git packages](../git.md)),
with a data folder of its own, makes the Git sample's repository with
`scripts/repository_server.py make-sample` (the source on `main`, the build
on `release`, tagged `v0.1.0`; `git` runs there with none of the user's
configuration) and serves it on 127.0.0.1 with `scripts/repository_server.py
serve`, which runs `git upload-pack`: `--install git:<address>` explains the
default branch as source-only (300, captured again every half second until
the explanation's color shows, for up to 60 s, rather than after a fixed
delay), then "Install extension from Git…", found by its title, opens the
Git field in Settings (#168), which takes `<address>@v0.1.0`
(301); Show Package previews the tag, pinned (302), Install installs it (303), and the smoke runs its
command, which answers "Hello from the Git repository" (304);
`installed.json`, read as JSON (`scripts/check_git_record.py`'s checks, done with `ConvertFrom-Json`), must then
record one package from Git with `"gitRef": "refs/tags/v0.1.0"`, `pinned`
and the `gitCommit` the tag points to (`repository_server.py commit`), and
the downloads folder must be empty. Pane itself runs no
`git`; the address is plain `http://` on a loopback address, which only a
development build fetches, so the smoke never reaches the network, and the
HTTPS path to a real host ([by hand](../git.md#trying-a-real-host-by-hand))
is not exercised. Tree names are refused alike on every system (the same
rules as npm's, plus `.git`, `git~1` and names differing only in case).
The phase runs the script with `python` and needs `git` on `PATH` (as the tests do); SendKeys types the address, which holds no SendKeys special character. A failure in the phase stops Pane in its `finally` block, as well as the server. **Not run on Windows yet.**

#50 extends the phase with a second repository of the same sample
(`make-sample` again, `greeter-tracked` beside `greeter`, served by a
server of its own), installed in a data folder of its own from its tracked
`release` branch (`--install git:<address>@release`: the preview says
"Revision: branch release, tracked: an update fetches that branch again",
305; installed, 306); `repository_server.py move-sample` then commits a
0.2.0 on the branch while Pane is stopped, and the check a second after
the restart replaces the installed copy by itself — nothing of it running
— saying "Updated Greeter from Git to 0.2.0" (307, captured until it
shows), the new copy's command answering as before (308);
`installed.json` must record the branch's new commit, tracked and
unpinned. Written and checked by reading only; **not run on Windows yet**
(the local Windows smoke run of this feature run will capture its
evidence).

## Files (#29)

The files phase (screenshots 220 to 224, [files](../files.md)), with a data
folder of its own, installs Files (220, "Installed Files"), whose file
index (#175) covers a fixture folder "Pane smoke files" (spaces) in the
smoke's output folder that a debug build's `PANE_TEST_FILE_INDEX_HOME`
names instead of the home folder; there is no folder to choose any more.
It types "plan" in root search, which must list "Résumé plan ü.txt" under
"Files", selected, and presses Enter. Files installed from its folder is not
the registered default, so its Search Files command keeps the generic list
(#177): the smoke does not open it. The real opener (PowerShell's
`Invoke-Item -LiteralPath` for an existing path, through the `open` crate,
the path passed in an environment variable) can show the "Open with" dialog
for a type with no handler, so a debug build's `PANE_TEST_OPEN_FILE_LOG`
makes it record the path in a file instead; the recorded path, resolved,
must be the fixture file's, resolved. Last it types "runner" and presses
Enter on a batch file, which Pane must show in File Explorer without
recording or running it (ADR 0037). A positive native open on Windows is
therefore not run by the smoke. The
scan policy skips entries with the hidden or system attribute and junctions
(reparse points) on Windows only, and a grant refuses UNC paths from their
text before any file system call; a Windows-only test (`attrib +h`,
`mklink /J`) is written but has not run. **Not run on Windows yet.**

## Searching inside a command (#30)

The smoke's search phase (screenshots 160 to 169, [command search](../command-search.md#checks)),
with a data folder of its own, builds and starts the fixture service on a
free port of 127.0.0.1 (`fixture_service --port 0`, the port read from its
log) and installs Package search, the Rust search sample. "aurora" typed in
root search must leave the service's log without a request; opened, the
command's "Service address" form is set to the service; its search field
sends "aurora" (results listed, Enter shows a package's details); "slow" then "ember"
must log the held search as abandoned; the service's 503, then the service
stopped, are errors; restarted, a search lists results again. Windows retries a refused connection for about two seconds, so the offline step waits longer.
The phase's first run, in CI (run 36536816781 of the fork
`wasimysaid/pane`, at ca9563b), passed up to 168 and failed at 169, "the
Pane window is not visible": `Start-Process` without `-NoNewWindow` gave
the restarted fixture service a console window of its own, which came to
the front over Pane and took the keys typed next (the restarted service
logged no search; the screenshot shows its console window). The service now
starts with `-NoNewWindow`, as the npm registry and the repository server
do. The fix was checked by reading only (no PowerShell or Windows here).
**Not run on Windows yet.**

## Clipboard history (#35)

The smoke's clipboard phase (screenshots 280 to 285, [clipboard history](../clipboard-history.md#checks)),
with a data folder of its own, runs after the #51 phase: since #166 only
Pane's registered Clipboard History records from the first start, so the
phase sets the default set up from the pinned repositories that phase
serves, with the smoke's own build (Files' index on an empty folder,
`PANE_TEST_FILE_INDEX_HOME`). It checks `clipboard-history.json` at each
step: plain text copied with nothing turned on is kept, while text
carrying `ExcludeClipboardContentFromMonitorProcessing`,
`CanIncludeInClipboardHistory` = 0 or `CanUploadToCloudClipboard` = 0 is
not; nothing is kept while paused (Pause Recording in the split view's
Actions panel, Ctrl+K), or while disabled (the switch on its page in
Settings), also after a restart; Enter on a record (the view's filter
leaves it) puts it on the clipboard again and moves it to the front;
enabled again, text is kept, also after a restart, before the command is
opened. The smoke copies only its own `pane-smoke-...` text, through the
clipboard API from PowerShell, and so replaces what was on the clipboard,
without reading or restoring it. `clipboard_adapter.rs` checks the adapter
alone: plain text reported with its owner (the test's process), each marker
read and withholding the text, a written text reported, and nothing once
the watch is dropped; it too replaces the clipboard, so it runs only with
`PANE_TEST_REAL_CLIPBOARD=1`, which CI's Windows job sets. `atomic.rs`'s
Windows unit test checks the owner-only DACL of `clipboard-history.json`
and `credentials.json`. Since #130 each local credential and each kept clipboard item's
text and files are also encrypted in those files with DPAPI for the current
user ([protected credentials](../extension-data.md#protected-credentials)),
so the smoke checks the token by its `dpapi` value, never its text, and
reads the kept texts through `scripts/clipboard_history.py`, which
decrypts them as the same user.

The adapter, the shared message thread (also the hotkey adapter's), the
DACL and these tests were only compile- and lint-checked for
`x86_64-pc-windows-gnu` from Linux; **not run on Windows yet**. They run in
CI (`cargo xtask ci` with `PANE_TEST_REAL_CLIPBOARD=1`, then
`smoke-windows.ps1`) on `windows-2025`, and the next green Windows run of
the branch is their evidence: until then the listener's delivery, the
retry and stop paths, the markers as real password managers set them, the
owner lookup and the DACL are unverified natively.

## Clipboard history expiry and deletion (#36)

The clipboard phase goes on (screenshots 400 to 405, [clipboard
history](../clipboard-history.md#checks)) with the history it kept. With
Pane stopped, the smoke makes `pane-smoke-kept` 8 days old and
`pane-smoke-enabled` 2 hours old in `clipboard-history.json`
(`scripts/clipboard_history.py`), as a downtime would; once Pane starts,
the first is gone from the file and the list before the command shows
anything. Then, in the split view's Actions panel (#166): Delete Entry on
`pane-smoke-second` deletes that record alone, and the clipboard is
unchanged; keeping records for 1 Hour deletes `pane-smoke-enabled` at once;
a file copied as File Explorer copies it (`CF_HDROP`) is kept as a file
(#167, 403); and Clear History, once confirmed (404), deletes every record
while recording goes on (`capture` stays `on`), the clipboard still
holding the copied text, and a later copy is kept. The forms #36 added
(Delete recent items, Turn off and delete) are gone since #166. Rewritten
with #161 and parsed, **not run yet**.


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
- **Operations (#22):** the operations phase (screenshots 31 and 32) installs
  the JavaScript operations sample, then the Rust one, opens the Rust
  sample's command and fills its form with the JavaScript package's identity
  and the name "Rust"; the Rust guest calls that package's `greet` operation
  through Pane, the same steps as on
  [Linux](linux.md#operations-22). In run
  [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
  (commit `1848494`) every step passed: the Rust guest's call into the
  JavaScript package answered "Hello, Rust, from JavaScript"
  ([32-operation-answer.png](evidence/windows/32-operation-answer.png)).
- **Reload (#11):** after the operations phase, the smoke installs a package from
  `<output-dir>\dev`, replaces its component with the JavaScript sample and
  reloads it in Manage extensions, then reloads it without its component (the
  checks fail and the old code keeps answering) and with the `failing-start`
  fixture (a startup failure, then Retry); screenshots 33 to 39, the same
  steps as on [Linux](linux.md#reloading-a-package-11). In run
  [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
  (commit `1848494`) every step passed: reloaded with the JavaScript build,
  Dev answered from the new code
  ([35-dev-after.png](evidence/windows/35-dev-after.png)); reloading the
  `failing-start` fixture showed "Reloaded Dev, but it failed to start…
  Retry…" ([38-start-failed.png](evidence/windows/38-start-failed.png)), and
  Retry then showed "Started Dev"
  ([39-retried.png](evidence/windows/39-retried.png)). Replacing the managed
  copy removes the old folder on a best-effort basis; on Windows a folder
  still in use is left behind, listed in `installed.json`, and removal is
  tried again at the next start (tested on Linux with a folder whose files
  cannot be deleted; not run on Windows).
- **Clearing an extension's cache (#39):** after the reload phase, the smoke
  restarts Pane, saves one value of each kind of
  [extension data](../extension-data.md) for the Settings sample (style,
  content, cache and credential), then chooses "Clear cache of Settings
  sample" in Manage extensions and confirms, the same steps as on
  [Linux](linux.md#clearing-an-extensions-cache-39). In run
  [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
  (commit `1848494`) every step passed: Manage extensions showed "Cleared the
  cache of Settings sample"
  ([42-cache-cleared.png](evidence/windows/42-cache-cleared.png)), and "Show
  what Pane keeps" then read "Style: formal · Note: Water the plants ·
  Signed in: yes · Cached greeting: none", with the cached greeting gone and
  the other three values kept
  ([43-kept-after-clear.png](evidence/windows/43-kept-after-clear.png)).

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

Since #162 Pane registers no sample command: the first phase installs the
three samples into a data folder of their own, and this phase's data folder
lists the installed Rust sample alone, so the screenshots show that copy. In these screenshots the root list is taller than the window and its
last row is cut off; since #19 the list scrolls to keep the selected row
visible.

## Installing Pane and acquiring its calculator (#51)

A final phase, after the clipboard-expiry one, proves the whole outcome of
[#51](https://github.com/pane-app/pane/issues/51)
([installer](../installer.md)), the Windows half of what
[#53](linux.md#installing-pane-and-acquiring-its-calculator-53) proved on
Linux. `cargo xtask package-windows --dev` builds the Windows package — a
zip, because a Windows user unzips with whatever is at hand — holding
`pane.exe`, `install.ps1` and a README (the development profile, so its
program takes its pins from `PANE_DEFAULTS`) and the application-update
artifacts; no default-extension payload is written — first setup fetches
the five defaults from the commits this release pins. The smoke clones
their repositories at those commits from their real addresses on GitHub
(its own setup, on the runner) and serves the clones on 127.0.0.1 over
Git's smart HTTP protocol (`scripts/repository_server.py`; nothing the
Pane under test does reaches the network
or Pane's published downloads), named by the pins file the development
build reads through `PANE_DEFAULTS`. The package is unzipped into a folder of
its own and its install script runs with a **clean machine's**
environment: a fresh user profile (`LOCALAPPDATA` and `APPDATA` pointing
into the smoke's output folder, so the install, Pane's data and the
shortcut touch nothing of the runner's user) that holds no data. It
installs `pane.exe` to `%LOCALAPPDATA%\Pane` and a `Pane` shortcut to the
user's Start menu, and runs `pane --version` (its exit code) to check
what it installed; no administrator rights are involved. The installed
Pane then starts with a PATH that holds nothing at all (an empty folder,
checked with `Get-Command` of cargo, rustc, node, npm, git, cc, clang and
make; a running process's own environment cannot be read on Windows, so
what is checked is the environment `Start-Process` hands the child),
taking its pins from the override and its artifact source from the local
server the smoke serves the update index on. It fetches the
five default extensions by itself, with Pane's own Git client
(`installed.json` must record each under
`"default"`, with the repository, release tag, commit and pinned state of
its pin — checked by `scripts/check_git_record.py` — and no sample: the
helper sample left the default set with
#162, and a helper running from an acquired revision is
`crates/pane-core/tests/installer.rs`'s), root search lists their
commands, "6*7" answers 42 and Enter copies it, with no developer tool
reachable. The downloads folder the fetches used must end empty. The program files are removed again at the phase's end, so the
uploaded evidence is the screenshots and records (frames 500 to 502), not
the program. CI builds the
release-profile package after the smoke and uploads it with the artifacts
of the job (`windows-package`).

**Recorded 2026-09-29, this branch's machine (headless aarch64 Linux —
no Windows, and no PowerShell to parse the scripts):** what ran locally
is everything that machine can run: `cargo check`, `clippy`, `fmt` and
the unit tests of `xtask` with its zip writer (whose bytes are pinned by
a test and were first decoded with Python's `zipfile`: the names, sizes,
CRCs, the fixed 1985-10-26 08:15 time and the round-tripped contents);
`cargo xtask package-windows` far enough to assemble the whole artifact
tree, before it refuses with the message that only a Windows checkout
builds `pane.exe`; and `cargo xtask package-linux` end-to-end in both
profiles as the shared packaging's regression (the dev package unpacked,
its install script run into a temporary home with a scrubbed PATH:
`pane --version` answered). The acquisition the installed Pane
does is the same platform-independent code `crates/pane-core/tests/installer.rs`
checks (rerun here, all passing, including the prebuilt helper running
from the managed copy — this machine's helper file names `linux-aarch64`,
which CI's Windows run assembles as `windows-x86_64`). The PowerShell
install script, the smoke phase, the zip package itself and the
release-profile package are **pending CI**: they need the Windows build
and interactive desktop only the `windows-2025` runner provides. The
smoke's clean machine is a fresh profile on that runner, not a fresh
machine; no Windows 10 or 11 client, ARM64 or real desktop install has
been tried; and nothing is signed (no Authenticode credentials exist),
so Windows may warn about an unknown publisher when `pane.exe` runs and
PowerShell may refuse the install script until the policy question the
README answers is answered.

| Step | Evidence |
| --- | --- |
| The clean machine's Pane acquired the five default extensions, and no sample (#162), and lists their commands | pending CI (frame 500) |
| "6*7" answers 42 | pending CI (frame 501) |
| Enter copies the answer | pending CI (frame 502) |

## Installing a Pane application update by the user's choice (#54)

A final phase, after the installer one, proves the whole outcome of
[#54](https://github.com/pane-app/pane/issues/54)
([installer](../installer.md)). The same `cargo xtask package-windows
--dev` builds a **second** package with `--package-version 99.0.0`: a
program that reports 99.0.0, a package named by it, and an index whose
`application` entry names that package for `windows-x86_64` — the two
runnable builds an update goes between. The 0.1.0 package the installer
phase built is installed on another clean profile (its install script,
its empty PATH, its own data under `%LOCALAPPDATA%\Pane\data`), and the
smoke serves the 99.0.0 artifacts from 127.0.0.1 with
`scripts/artifact_server.py` (nothing reaches the network or Pane's
published downloads). The installed 0.1.0 Pane, started with
`PANE_ARTIFACTS` and the same pinned repositories, fetches
its default extensions and, in the same
background, checks the index for a newer version of itself: the offer
appears as **Update Pane to 99.0.0** in root search (frame 601; the
status line tells what was found, frame 600). The artifact server's log
must hold **no request for the package** until the row is chosen —
nothing is downloaded, installed or restarted automatically. Clipboard
History is disabled first (the Helper sample was, until #162 took it out
of the default set), so an extension the user disabled before the update
must stay disabled after it. Choosing the row with a **damaged
package** is explained (its bytes do not match the sha512 its entry
gives, frame 602) with the program, the data and the folder untouched
and the row ready to try again; then the real choice downloads the
package, checks it, unpacks it and swaps the running `pane.exe` — the
old program renamed `pane.exe.old`, the new one in its place, the
staging folder gone (frame 603, and hash checks of both programs against
the two packages' own files). The next start runs the new version: it
reports `Pane 99.0.0`, removes `pane.exe.old` at start, the calculator
still answers "6*7" with 42 from the old version's install (the
calculator set up at first setup, from the pinned repositories;
frames 604 and 605), and the disabled Clipboard History stays disabled —
Pane's data was never touched. Pane itself was never restarted by the
update: the smoke stops the old process and starts the new program
itself, exactly as the user would.

**Recorded 2026-09-30, this branch's machine (headless aarch64 Linux —
no Windows, and no PowerShell to parse the scripts):** what ran locally
is everything that machine can run: the platform-independent half
(`crates/pane-core/tests/app_update.rs`, all passing, including the
swap, the failures and the data kept), the strict zip reader's unit
tests, and `cargo xtask package-linux --dev` (with and without
`--package-version 99.0.0`) end-to-end — two packages named by their
versions, the index carrying the `application` entry naming the package,
the program reporting the overridden version, and the package served
from the artifacts folder. The PowerShell smoke phase, the
`--package-version` Windows build and the running-exe rename on Windows
itself are **pending CI**: they need the Windows build and interactive
desktop only the `windows-2025` runner provides. The running-`pane.exe`
rename the swap depends on is a documented Windows behavior (renaming a
running executable is allowed; overwriting one is not) that only the
runner can prove end to end.

| Step | Evidence |
| --- | --- |
| The check at start tells the user; nothing is downloaded until they choose | pending CI (frames 600, 601) |
| A damaged package is explained, everything untouched, the row retried | pending CI (frame 602) |
| The user's choice swaps the running program; the new one is used the next start | pending CI (frame 603) |
| The new version reports itself; the old version's data and enablement are kept | pending CI (frames 604, 605) |

## Remaining limits

- Only a CI runner (Windows Server) was used, not a Windows 10/11 desktop.
- The #4 resource and latency workload exists for Linux only. On this
  platform only Pane's cost while hidden has a script
  (`scripts/measure-windows.ps1`, #189), run on the user's machine with
  their consent and not run yet; the other measurements and every target
  are not started
  ([the measurement record](../research/resource-measurements.md)).
- The installer (#51, [above](#installing-pane-and-acquiring-its-calculator-51))
  and the application update (#54, [above](#installing-a-pane-application-update-by-user-choice-54))
  have run nowhere yet: their PowerShell scripts and smoke phases were
  written without PowerShell on hand, and their runtime evidence is CI's
  Windows leg. Nothing is signed, so an unknown-publisher warning and
  PowerShell's policy question are expected, not errors.
- No screen reader (Narrator/NVDA) was run. The accessibility tree is verified
  only through GPUI in the platform-independent window tests.
- The smoke confirms that text appears in the expected colors and that the
  three results differ. Which command and guest each screenshot shows was
  checked by inspection.
