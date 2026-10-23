# Windows native baseline (#5, #6)

This page records what Pane has been checked to do on Windows, how it was
checked, and what has not run there yet. It is for contributors and anyone
who wants to know how far to trust Pane on Windows. Most of it follows the
native GUI smoke test, `scripts/smoke-windows.ps1`, one phase at a time.

## Summary

Pane builds and passes its checks on Windows Server 2025 on x86_64. The
core launcher works there natively: rendering, keyboard focus, selection,
running guests and showing results. All of it ran on a GitHub-hosted
runner, not on a Windows 10 or 11 desktop.

### What to install

- To build: MSVC from Visual Studio, and the Rust toolchain that
  `rustup toolchain install` reads from `rust-toolchain.toml`.
- To run the smoke: Python 3 with Pillow. The Git phase also runs its
  script with `python` and needs `git` on `PATH`.
- To install Pane itself: the Windows package is a zip holding `pane.exe`,
  `install.ps1` and a README. The script installs `pane.exe` to
  `%LOCALAPPDATA%\Pane` and adds a `Pane` shortcut to your Start menu,
  with no administrator rights
  ([installing Pane](#installing-pane-and-acquiring-its-calculator-51)).
  Nothing is signed, so Windows may warn about an unknown publisher, and
  PowerShell may refuse the install script until the policy question the
  README answers is answered. The package itself is still pending CI.

### What has run

| Feature | On Windows |
| --- | --- |
| [Build and `cargo xtask ci`](#fresh-checkout-build-and-checks) | Passed |
| [Native GUI smoke](#native-gui-smoke) | Passed |
| [Platform availability (#19)](#platform-availability-19) | Passed; the later #19 fixes have not run |
| [Root search (#23)](#root-search-23) | Passed; input-method composition unverified |
| [Calculator (#27)](#calculator-27) | By-hand phase passed; installer phase pending CI |
| [Text input (#20), custom view (#21), operations (#22), reload (#11), clearing a cache (#39)](#text-input-and-accessibility-findings) | Passed; IME composition and scaling other than 100 % unverified, no screen reader run |
| [Disabling an extension (#10)](#disabling-an-extension-and-keeping-its-settings-10) | Passed |
| [Local extension package (#9)](#local-extension-package-9) | Passed |
| [Opening a web link (#28)](#opening-a-web-link-28) | Only through the tests' recording opener |
| [Applications (#24)](#applications-24), [global hotkeys (#32)](#global-hotkeys-32), [deleting retained data (#41)](#deleting-retained-data-41), [aliases (#31)](#aliases-and-fallbacks-31), [dependencies (#42)](#dependencies-42), [native helpers (#15)](#native-helpers-15), [development mode (#12, #13)](#development-mode-12-13) | Not run on Windows yet |
| [Disabling (#43)](#disabling-required-dependents-43) and [uninstalling (#44)](#uninstalling-required-dependents-44) required dependents, [runtime crashes (#17)](#runtime-crashes-17), [extensions that stop responding (#18)](#extensions-that-stop-responding-18) | Not run on Windows yet |
| [npm (#45)](#npm-packages-45) and [Git (#46)](#git-packages-46) packages, [files (#29)](#files-29), [searching inside a command (#30)](#searching-inside-a-command-30), [clipboard history (#35)](#clipboard-history-35) and [its expiry (#36)](#clipboard-history-expiry-and-deletion-36) | Not run on Windows yet |
| [Installing Pane (#51)](#installing-pane-and-acquiring-its-calculator-51), [updating it (#54)](#installing-a-pane-application-update-by-the-users-choice-54) | Pending CI |

## Tested combination

The baseline was recorded 2026-09-28 from GitHub Actions run
[36366760796](https://github.com/wasimysaid/pane/actions/runs/36366760796) on the
fork `wasimysaid/pane`, commit `572d629`.

| | |
| --- | --- |
| OS | Windows Server 2025 Datacenter, `Microsoft Windows NT 10.0.26100.0` (build 26100) |
| Architecture | x86_64 (`AMD64`) |
| Machine | GitHub-hosted runner, image `windows-2025-vs2026` version 20260922.246.2 |
| Session | the runner's interactive desktop session (Explorer shell, taskbar) |
| Toolchain | MSVC from the image's Visual Studio; Rust 1.98.1 (`rust-toolchain.toml`) |

Not tested, and so not claimed: Windows 10/11 client editions, ARM64, a
physical display or GPU driver other than the runner's, high-DPI scaling,
and any installer or signed build.

## Fresh checkout build and checks

```powershell
rustup toolchain install
cargo xtask ci          # guests, prebuilt JS/TS check, fmt, clippy, all tests
cargo build --locked -p pane
```

`cargo xtask ci` passed (13 window, 9 launcher-model, 1 runtime-cache and
25 sample-contract tests), and the launcher built.

The Rust guests are built natively; the JS and TS samples use the
committed prebuilt components. Rebuilding them from source on Windows
(`cargo xtask js-guests`) has not been run. The Cargo cache was warm
(`Swatinem/rust-cache`).

## Native GUI smoke

The smoke forces the dark theme and the opaque material, so its
screenshots do not depend on the desktop
([smoke appearance](smoke-appearance.md)).

```powershell
cargo build -p pane
./scripts/smoke-windows.ps1 -OutDir smoke   # needs Python 3 with Pillow
```

The script launches `target\debug\pane.exe` and waits for its main
window. It then brings the window to the foreground and sends real key
events with `SendKeys`. For each of the Rust, JavaScript and TypeScript
sample commands, it presses Enter to open it, Down and Enter to run "Wait
briefly" (an async WASI 0.3 clock import inside the guest), then Escape.

### Driving Settings

Extensions are managed in Settings since #168
([ADR 0043](../adr/0043-extensions-are-managed-in-settings-one-page-per-extension.md)),
where switches, menus and confirmation rows answer the pointer only. The
smoke opens Settings with root search's Manage Extensions command. It
finds each control by its accessible name in the Settings window through
UI Automation (`UIAutomationClient`; Pane's tree comes from AccessKit)
and toggles or invokes it, clicking its center where it offers no
pattern.

The smoke checks an operation's outcome by the name of the page's status
line rather than by a color. It closes Settings with Ctrl+W before it
goes back to the launcher. Lines of plain text (a preview's details) have
no accessible name, so the smoke waits for a preview by its row (Update,
Install).

Two details come from failed release runs:

- Release run 37689872256 drove Settings this way through frame 68. It
  failed at frame 69, Echo's answer: a command's answer is a toast since
  #141, which leaves the footer 3 seconds after it appears, and the fixed
  3-second wait captured the screen after it had left. Such answers are
  now captured until their color shows (`Capture-Until`), from half a
  second after the key.
- Release run 37698693722 then reached the Files phase, where an Escape
  after installing Files hid the launcher, so frame 221 showed the
  desktop. The install lands on a blank root search, where Escape hides
  it. The return to root key (Shift+Escape) is pressed there instead.

### What the smoke checks

The smoke fails if the window does not appear or Pane exits. The same
three screenshot checks as on macOS and Linux also run
(`scripts/check_screenshot.py`):

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

Rendering, keyboard focus, selection, guest execution and result display
all worked natively.

In the root screenshots the TypeScript row also has a lighter background.
This is most likely hover under wherever the runner's mouse pointer sits,
since keyboard selection (the Rust row) opened the Rust command. This was
not confirmed.

## Platform availability (#19)

The smoke also runs the platform-availability steps (screenshots 13 to 15,
[platform availability](../platform-availability.md#checks)): the Rust
command's Windows-only and macOS-and-Linux actions, then a package listing
only the two other systems.

In run [36372625940](https://github.com/wasimysaid/pane/actions/runs/36372625940)
(commit `38a95cb`, Windows NT 10.0.26100, AMD64) every step passed:

- The Windows-only action answered.
- The macOS-and-Linux action was listed with "Not available on Windows:
  this action supports only macOS and Linux" and did not run.
- The package for macOS and Linux was refused with "Not available on
  Windows: this package supports only macOS and Linux".
- The list scrolled to keep the selected row visible.

The later #19 fixes (per-command platforms, re-focusing before these
steps) have not run here yet.

| Step | Evidence |
| --- | --- |
| Windows-only action | [13-windows-only.png](evidence/windows/13-windows-only.png) |
| macOS-and-Linux action | [14-not-windows.png](evidence/windows/14-not-windows.png) |
| Package for the other two systems | [15-no-compatible-package.png](evidence/windows/15-no-compatible-package.png) |

## Root search (#23)

Root search has a query field with focus ([root search](../root-search.md)).
The smoke's search phase (screenshots 24 to 26) types "typescr" with
`SendKeys`, opens the only match and runs "Wait briefly", which must look
exactly like step 4. Then it types "zzz" and presses Enter on no results.

In run [36420611977](https://github.com/wasimysaid/pane/actions/runs/36420611977)
(commit `6d73d18`) every step passed. Typing "typescr" left only
TypeScript sample and Enter ran it, and "zzz" showed No results
([24-search.png](evidence/windows/24-search.png),
[26-no-results.png](evidence/windows/26-no-results.png)). Input-method
composition in the query field is still unverified here.

## Calculator (#27)

The smoke's installer phase (screenshots 501 and 502) covers the
calculator. It is set up on a clean machine from the commit this release
pins (its repository cloned at that commit and served on 127.0.0.1).
Typing "6*7" checks the selected answer row, and Enter copies the answer,
with no developer tool anywhere.

Screenshots 27 to 30 below come from an earlier by-hand calculator phase.
It installed the package from this repository's guests tree and also
checked the clipboard round-trip. It left with the extension's sources
(#285), its arithmetic being its repository's to test.

In that phase's run
[36423871204](https://github.com/wasimysaid/pane/actions/runs/36423871204)
(commit `ab91081`) every step passed: "6*7" answered 42, Enter copied it,
and pasting then typing "+1" matched typing "42+1", so the system
clipboard held "42" ([27-answer.png](evidence/windows/27-answer.png),
[30-pasted.png](evidence/windows/30-pasted.png)).

## Applications (#24)

Not run on Windows yet. This branch was not pushed, so the phase, the
native tests and the `ShellExecuteEx` path are unverified here. The
Windows code was only type-checked and linted for
`x86_64-pc-windows-gnu`.

[Applications](../applications.md) finds two kinds of application:

- the `.lnk` shortcuts in the user's and all users' Start menu Programs
  folders, opened with `ShellExecuteEx` as Explorer does;
- the packaged (AppX/MSIX) apps of the shell's Apps folder, such as
  Calculator on Windows 11, opened by their AppUserModelID.

A native test requires an inbox packaged app (Calculator or Settings) to
be found.

The smoke's last phase (screenshots 44 and 45):

1. Makes a shortcut "Pane Smoke App" to `cmd.exe` writing a marker file
   (with `WScript.Shell`, minimized) under an APPDATA given to Pane only.
2. Installs the JavaScript applications sample, which supplies the host's
   applications to root search as the [Applications](../applications.md)
   default extension does.
3. Types "pane smoke", checks the selected row and presses Enter.
4. Checks "Opened Launch Pane Smoke App" and the marker.

The adapter tests open such a shortcut too.

## Opening a web link (#28)

Opening a link on Windows is checked only through the tests' recording
opener.

The smoke's quicklinks phase installed the Quicklinks package and created
a quicklink in its form. It left with the extension's sources (#285), its
quicklinks being its repository's to test. The host's link opening is
covered by the Linux smoke's web-link phase, which runs the actions
sample's Open Website through the system's handler.

## Global hotkeys (#32)

Not run on Windows yet. The adapter was only compile- and lint-checked
for `x86_64-pc-windows-gnu` from Linux, so registration, delivery and the
focus transition (foreground rules) are unverified natively.

The smoke's hotkey phase (screenshots 52 to 58,
[global hotkeys](../hotkeys.md#checks)):

1. Assigns Ctrl+Alt+G to Greeting in the hotkey recorder on the settings
   sample's page in Settings (#168; it was the launcher's hotkey screen
   before).
2. Minimizes Pane, presses the hotkey with `SendKeys` and checks that
   Pane's window is the foreground window again with Greeting open.
3. Does the same again after a restart.
4. With the extension disabled, checks that pressing it leaves Pane
   minimized and unchanged.

`RegisterHotKey` needs no permission. `hotkey_adapters.rs` registers a
shortcut on the test session, checks that a second registration is
refused as taken, and checks that the released shortcut can be registered
again.

## Deleting retained data (#41)

Not run on Windows yet.

The smoke's retained-data phase (screenshots 63 to 65,
[deleting retained data](../extension-data.md#deleting-retained-data))
uses a data folder of its own. It saves a note with the settings sample
and uninstalls it, keeping its saved data. Then it deletes that retained
data from its row on the Extensions group's page in Settings and confirms
(#168). It checks that `installed.json` and `content.json` no longer hold
it, and reinstalls the same folder, which must show nothing kept.

A data file locked by another program is covered only by the tests'
unreadable files. A record that cannot be written after the data is
deleted is covered only by a Unix test.

## Aliases and fallbacks (#31)

Not run on Windows yet. Nothing in it is specific to Windows (no system
API is involved).

The smoke's alias phase (screenshots 66 to 74,
[aliases and fallbacks](../aliases.md#checks)) gives Echo, the query
sample's command, the alias "ec" and makes it a fallback on its
extension's page in Settings (its alias cell and fallback switch, #168).

It then sends "ec hello" and "zqx" to it. The first fallback is
preselected when nothing else matches
([ADR 0031](../adr/0031-the-first-fallback-is-preselected-when-nothing-else-matches.md)),
so Enter sends "zqx" without a move. Last, it checks that with the
extension disabled "ec hello" lists nothing.

## Dependencies (#42)

Not run on Windows yet, so `..` across drive-letter and UNC paths is
untested natively.

The dependencies phase (screenshots 75 to 77,
[dependencies](../dependencies.md#checks)) uses a data folder of its own.
It previews the dependencies sample, which lists its required JavaScript
operations sample and optional Rust one. It installs it with the
JavaScript sample and runs "Greet through the required greeter", which
must answer from the JavaScript guest. `installed.json` must then hold
exactly two packages and the recorded dependency.

A dependency's `local:../…` source is joined to the package folder and
resolved with the same `canonicalize` (without the `\\?\` prefix) as
package identity. A folder that does not exist is resolved from its
spelling.

## Native helpers (#15)

Not run on Windows yet. The runner was only compile- and lint-checked for
`x86_64-pc-windows-gnu` from Linux, so starting, ending and reaping a
helper natively, and the smoke's process checks, are unverified there. No
Windows arm64 build was made.

The first native-helper phase (screenshots 90 to 93, data folder
`helper-data`, [native helpers](../helpers.md#checks)) installs the
helper sample. `cargo xtask guests` builds its `pane-echo.exe` for the
runner (`windows-x86_64` on `windows-2025`). The phase then:

- runs it (the answer must name Windows x86-64);
- cancels a slow run after a second;
- starts the ten-second run and checks with `Get-Process` that the helper
  runs from the managed copy;
- disables the package and checks that the process is gone, that the
  saved "started" note is kept, and that no helper outlives Pane.

A second phase (screenshot 94, `helper-quit-data`) starts the waiting
helper and closes Pane's window with `CloseMainWindow` (WM_CLOSE). It
checks that Pane exits, no helper runs and its heartbeat file stops
growing.

The tests in `crates/pane-core/tests/helpers.rs` (Rust, JavaScript and
TypeScript samples; a PE header) and the runner's unit tests (the `.exe`
rule and absolute path are checked on every system) run in
`cargo xtask ci` there, against the `pane-echo.exe` built natively on the
runner.

Pane starts a helper without a console window (`CREATE_NO_WINDOW`) and
ends it with `TerminateProcess`. A helper's own children are not in a job
object. An update ends the old copy's helpers before removing its folder,
so the folder is not in use; the removal at the next start remains a
fallback.

## Development mode (#12, #13)

Not run on Windows yet. The platform code (`ReadDirectoryChangesW`
through notify, a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
`CREATE_NEW_PROCESS_GROUP` and builds without a console window) was only
compile- and lint-checked for `x86_64-pc-windows-gnu` from Linux. So the
file watcher's events, the build's processes being killed and the whole
phase are unverified natively.

The smoke's last phase (screenshots 110 to 136,
[development mode](../development-mode.md#checks)) builds a copy of each
development sample and develops it from its page in Settings (Develop in
the Actions menu, #168). It saves an edit, a change that does not build,
two saves in a row and, after stopping, one more. It checks the answers,
the error, and that nothing is built after stopping. The TypeScript and
JavaScript samples run only where the JS toolchain is built, so CI's
smoke runs the Rust one.

## Disabling required dependents (#43)

Not run on Windows yet. Nothing in it is specific to Windows: no system
API is involved, and the closure reuses the dependency identities
recorded at install.

The disable-dependents phase (screenshots 140 to 143,
[disabling a required dependency](../dependencies.md#disabling-a-required-dependency))
uses a data folder of its own. It installs the dependencies sample and
turns off the JavaScript operations sample's switch on its page in
Settings (#168). Pane must ask first (its Disable all row shown). The
smoke cancels, then chooses Disable all (the page's status says so) and
enables the JavaScript sample again alone. `installed.json` must then
record exactly one disabled package.

## Runtime crashes (#17)

Not run on Windows yet. Nothing in it is specific to Windows: the
runtime is a thread, and helpers are ended as for a disable.

The runtime-crash phase (screenshots 200 to 209, data folder
`runtime-crash-data`,
[runtime crashes](../pausing.md#when-the-extension-runtime-itself-crashes))
installs the helper and settings samples and starts Pane with
`PANE_TEST_RUNTIME_FAULTS` naming a fault file. Then:

1. It runs the settings sample's Count, starts the waiting helper and has
   the runtime crash. The helper must be gone (`Get-Process`, and its
   heartbeat must stop growing), the note it saved kept, and the status
   line the error color.
2. A second crash, injected before Count's answer, must leave the count
   at 2 and the runtime stopped.
3. Root search explains it, and the Extensions group's page in Settings
   shows why (its runtime rows, and the details behind one).
4. A disable works, and Restart the extension runtime runs extensions
   again. Count then counts 3.

No package may be recorded as paused.

## Extensions that stop responding (#18)

Not run on Windows yet. Epoch interruption and the watchdog are
Wasmtime's and Pane's own, with nothing specific to Windows.

The unresponsive phase (screenshots 240 to 248, data folder
`unresponsive-data`,
[extensions that stop responding](../pausing.md#when-an-extension-stops-responding))
sets the runtime's limits through the fault file. It installs the
settings sample and runs its Stop responding, which computes without
waiting.

1. With `limits:60,4,15` (a minute of a guest's own computing), Escape
   and Manage Extensions must answer while the call still computes
   (Pane's standard error has stopped no call yet). Settings opens, frame
   240.
2. `limits:2,4,15` must then stop that call at once, as it computed
   longer.
3. Run again, the call must be stopped after 2 seconds of its own
   computing, with the error color. That is thread CPU time, from
   `GetThreadTimes` on Windows and `CLOCK_THREAD_CPUTIME_ID` on macOS.
4. The third time must pause the package (the error and reason colors),
   with the saved `busy` note still "started". The pause details and
   Retry must work.

Then the runtime thread is made to hang through the fault file (`hang`).
Opening Greeting must first show "not responding yet" (the progress
color, frame 245), then the error color after Pane gave up on the thread.
The runtime row of the Extensions group's page in Settings must open the
runtime's details. After `release`, a fresh thread must save the formal
greeting. No package may be recorded as paused.

## Uninstalling required dependents (#44)

Not run on Windows yet.

A phase of the smoke (screenshots 180 to 183,
[uninstalling a required dependency](../dependencies.md#uninstalling-a-required-dependency))
uses a data folder of its own. It installs the dependencies sample and
chooses Uninstall in the Actions menu of the JavaScript operations
sample's page in Settings (#168). Pane must ask first. The smoke cancels,
then chooses Uninstall all 2, keeping saved data (the page's status says
so). `installed.json` must then hold no package. Pane is started again to
install the JavaScript operations sample alone, and `installed.json` must
then hold exactly one package.

Removing a managed copy uses the same `remove_dir_all` as a single
uninstall. A folder still in use (a file open on Windows) is listed and
removed at the next start, reported against its own package. Nothing else
in it is specific to Windows.

## npm packages (#45)

Not run on Windows yet. The packing, in `cargo xtask guests`, runs in CI
on Windows.

A phase of the smoke (screenshots 260 to 268, [npm packages](../npm.md))
uses a data folder of its own. It starts `scripts/npm_registry.py` (with
`python`) on 127.0.0.1, serving the npm sample that `cargo xtask guests`
packed, and points the development build at it with
`PANE_NPM_REGISTRY`. Then:

1. It installs the local Dependencies from npm sample, which downloads
   and installs the npm package it requires, and calls its `greet`
   operation.
2. It names the npm package in the npm field that "Install extension from
   npm…" opens in Settings (#168). SendKeys types `@pane-samples/greeter`,
   and Show Package previews it there.
3. It updates the package and runs its command, which answers "Hello from
   the npm package".

### Automatic update (#49)

#49 extends the phase. A 0.2.0 of the sample is published into the
registry's folder (`scripts/npm_publish.py`; the registry reads its
folder on request), and Pane is stopped and started again. The check a
second after its start replaces the installed unpinned copy by itself,
with nothing of it running, and says "Updated Greeter from npm to 0.2.0"
(267). The new copy's command answers as before (268). `installed.json`
must then record `"npm": "@pane-samples/greeter"` at
`"npmVersion": "0.2.0"` with both packages.

### Names Windows cannot write

Unpacking refuses, on every system alike, the names Windows reads
differently or cannot write:

- `\ : < > " | ? *`;
- control characters;
- trailing dots and spaces;
- device names such as `con`, `conin$`, `conout$`, `com1` or `lpt³`
  (compared by character, with any extension).

So a tarball Linux accepts never fails or writes elsewhere on Windows.

### The real registry and the local servers

The real registry would be reached through the HTTP client guests'
requests use, trusting the certificates rustls-native-certs reads from
the Windows certificate store. No check exercises that: the smoke never
reaches the network, and the
[by-hand](../npm.md#trying-the-real-registry-by-hand) check has not been
run.

After the macOS smoke's first run ([macOS](macos.md#npm-packages-45)),
the registry and the Git phase's repository server skip a reverse DNS
lookup before they listen. The smoke waits up to 60 s for their port
files, stopping early if a server exits. On Windows that change was
checked by reading only.

## Git packages (#46)

Not run on Windows yet.

A phase of the smoke (screenshots 300 to 304, [Git packages](../git.md))
uses a data folder of its own. It makes the Git sample's repository with
`scripts/repository_server.py make-sample`: the source on `main`, the
build on `release`, tagged `v0.1.0`, with `git` run there with none of
the user's configuration. It serves the repository on 127.0.0.1 with
`scripts/repository_server.py serve`, which runs `git upload-pack`.
Then:

1. `--install git:<address>` explains the default branch as source-only
   (300). The screen is captured again every half second until the
   explanation's color shows, for up to 60 s, rather than after a fixed
   delay.
2. "Install extension from Git…", found by its title, opens the Git field
   in Settings (#168), which takes `<address>@v0.1.0` (301).
3. Show Package previews the tag, pinned (302), and Install installs it
   (303).
4. The smoke runs its command, which answers "Hello from the Git
   repository" (304).

`installed.json`, read as JSON (`scripts/check_git_record.py`'s checks,
done with `ConvertFrom-Json`), must then record one package from Git with
`"gitRef": "refs/tags/v0.1.0"`, `pinned` and the `gitCommit` the tag
points to (`repository_server.py commit`). The downloads folder must be
empty.

Pane itself runs no `git`. The address is plain `http://` on a loopback
address, which only a development build fetches, so the smoke never
reaches the network. The HTTPS path to a real host
([by hand](../git.md#trying-a-real-host-by-hand)) is not exercised. Tree
names are refused alike on every system: the same rules as npm's, plus
`.git`, `git~1` and names differing only in case.

The phase runs the script with `python` and needs `git` on `PATH` (as the
tests do). SendKeys types the address, which holds no SendKeys special
character. A failure in the phase stops Pane in its `finally` block, as
well as the server.

### A tracked branch (#50)

#50 extends the phase with a second repository of the same sample
(`make-sample` again, `greeter-tracked` beside `greeter`, served by a
server of its own). It is installed in a data folder of its own from its
tracked `release` branch (`--install git:<address>@release`). The
preview says "Revision: branch release, tracked: an update fetches that
branch again" (305), then it is installed (306).

`repository_server.py move-sample` then commits a 0.2.0 on the branch
while Pane is stopped. The check a second after the restart replaces the
installed copy by itself, with nothing of it running, saying "Updated
Greeter from Git to 0.2.0" (307, captured until it shows). The new
copy's command answers as before (308). `installed.json` must record the
branch's new commit, tracked and unpinned.

Written and checked by reading only; not run on Windows yet. The local
Windows smoke run of this feature run will capture its evidence.

## Files (#29)

Not run on Windows yet.

The files phase (screenshots 220 to 224, [files](../files.md)) uses a
data folder of its own. It installs Files (220, "Installed Files"). Its
file index (#175) covers a fixture folder "Pane smoke files" (spaces) in
the smoke's output folder, which a debug build's
`PANE_TEST_FILE_INDEX_HOME` names instead of the home folder. There is no
folder to choose any more.

The smoke types "plan" in root search, which must list "Résumé plan
ü.txt" under "Files", selected, and presses Enter. Files installed from
its folder is not the registered default, so its Search Files command
keeps the generic list (#177), and the smoke does not open it.

The real opener is PowerShell's `Invoke-Item -LiteralPath` for an
existing path, through the `open` crate, with the path passed in an
environment variable. It can show the "Open with" dialog for a type with
no handler, so a debug build's `PANE_TEST_OPEN_FILE_LOG` makes it record
the path in a file instead. The recorded path, resolved, must be the
fixture file's, resolved. Last, the smoke types "runner" and presses
Enter on a batch file, which Pane must show in File Explorer without
recording or running it
([ADR 0037](../adr/0037-a-command-declares-its-mode-and-host-functions-decide-what-happens-after-it-runs.md)).
So the smoke runs no positive native open on Windows.

The scan policy skips entries with the hidden or system attribute and
junctions (reparse points) on Windows only. A grant refuses UNC paths
from their text before any file system call. A Windows-only test
(`attrib +h`, `mklink /J`) is written but has not run.

## Searching inside a command (#30)

Not run on Windows yet.

A command's search is the List's search-text event
([the standard views](../designed-tree.md#the-standard-views)). The
smoke's search phase (screenshots 160 to 169) uses a data folder of its
own. It builds and starts the fixture service on a free port of
127.0.0.1 (`fixture_service --port 0`, the port read from its log) and
installs Package search, the Rust search sample. Then:

1. "aurora" typed in root search must leave the service's log without a
   request.
2. Opened, the command's "Service address" form is set to the service.
3. Its search field sends "aurora": results are listed, and Enter shows a
   package's details.
4. "slow" then "ember" must log the held search as abandoned.
5. The service's 503, then the service stopped, are errors. Windows
   retries a refused connection for about two seconds, so the offline
   step waits longer.
6. Restarted, a search lists results again.

### The first CI run

The phase's first run, in CI (run 36536816781 of the fork
`wasimysaid/pane`, at ca9563b), passed up to 168 and failed at 169, "the
Pane window is not visible". `Start-Process` without `-NoNewWindow` gave
the restarted fixture service a console window of its own. That window
came to the front over Pane and took the keys typed next: the restarted
service logged no search, and the screenshot shows its console window.

The service now starts with `-NoNewWindow`, as the npm registry and the
repository server do. The fix was checked by reading only (no PowerShell
or Windows here).

## Clipboard history (#35)

Not run on Windows yet. The adapter, the shared message thread (also the
hotkey adapter's), the DACL and these tests were only compile- and
lint-checked for `x86_64-pc-windows-gnu` from Linux.

### The smoke's clipboard phase

The smoke's clipboard phase (screenshots 280 to 285,
[clipboard history](../clipboard-history.md#checks)) uses a data folder
of its own and runs after the #51 phase. Since #166 only Pane's
registered Clipboard History records from the first start
([ADR 0042](../adr/0042-clipboard-history-records-from-the-first-start.md)).
So the phase sets the default set up from the pinned repositories that
phase serves, with the smoke's own build (Files' index on an empty
folder, `PANE_TEST_FILE_INDEX_HOME`).

It checks `clipboard-history.json` at each step:

- Plain text copied with nothing turned on is kept.
- Text carrying `ExcludeClipboardContentFromMonitorProcessing`,
  `CanIncludeInClipboardHistory` = 0 or `CanUploadToCloudClipboard` = 0
  is not kept.
- Nothing is kept while paused (Pause Recording in the split view's
  Actions panel, Ctrl+K), or while disabled (the switch on its page in
  Settings), also after a restart.
- Enter on a record (the view's filter leaves it) puts it on the
  clipboard again and moves it to the front.
- Enabled again, text is kept, also after a restart, before the command
  is opened.

The smoke copies only its own `pane-smoke-...` text, through the
clipboard API from PowerShell. So it replaces what was on the clipboard,
without reading or restoring it.

### The adapter and storage tests

`clipboard_adapter.rs` checks the adapter alone: plain text reported with
its owner (the test's process), each marker read and withholding the
text, a written text reported, and nothing once the watch is dropped. It
too replaces the clipboard, so it runs only with
`PANE_TEST_REAL_CLIPBOARD=1`, which CI's Windows job sets.

`atomic.rs`'s Windows unit test checks the owner-only DACL of
`clipboard-history.json` and `credentials.json`. Since #130 each local
credential and each kept clipboard item's text and files are also
encrypted in those files with DPAPI for the current user
([protected credentials](../extension-data.md#protected-credentials)).
So the smoke checks the token by its `dpapi` value, never its text, and
reads the kept texts through `scripts/clipboard_history.py`, which
decrypts them as the same user.

### What is still unverified

These run in CI (`cargo xtask ci` with `PANE_TEST_REAL_CLIPBOARD=1`, then
`smoke-windows.ps1`) on `windows-2025`, and the next green Windows run of
the branch is their evidence. Until then the listener's delivery, the
retry and stop paths, the markers as real password managers set them,
the owner lookup and the DACL are unverified natively.

## Clipboard history expiry and deletion (#36)

Rewritten with #161 and parsed, not run yet.

The clipboard phase goes on (screenshots 400 to 405,
[clipboard history](../clipboard-history.md#checks)) with the history it
kept:

1. With Pane stopped, the smoke makes `pane-smoke-kept` 8 days old and
   `pane-smoke-enabled` 2 hours old in `clipboard-history.json`
   (`scripts/clipboard_history.py`), as a downtime would.
2. Once Pane starts, the first is gone from the file and the list before
   the command shows anything.
3. In the split view's Actions panel (#166), Delete Entry on
   `pane-smoke-second` deletes that record alone, and the clipboard is
   unchanged.
4. Keeping records for 1 Hour deletes `pane-smoke-enabled` at once.
5. A file copied as File Explorer copies it (`CF_HDROP`) is kept as a
   file (#167, 403).
6. Clear History, once confirmed (404), deletes every record while
   recording goes on (`capture` stays `on`). The clipboard still holds
   the copied text, and a later copy is kept.

The forms #36 added (Delete recent items, Turn off and delete) are gone
since #166.

## Text input and accessibility findings

### Text input and IME (#20)

The smoke now also opens the Rust command's form and submits it empty,
and the error color must appear. Then it types "Ada" with `SendKeys`,
then Tab, Down and Enter. The result color must appear, which only
happens if the typed text reached the name field.

In run [36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770)
(Windows NT 10.0.26100, AMD64) every step passed, and the result read
"Good morning, Ada, from the Rust guest".

Windows IME (TSF) composition, for example with Microsoft Japanese IME,
is unverified. The window tests cover composition only on the field's
editing state ([what that proves](../forms.md#checks)).

Screenshots from run
[36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770)
(commit `949e35d`), cropped to the window:

| Step | Evidence |
| --- | --- |
| Form opened; focus in the name field | [6-form.png](evidence/windows/6-form.png) |
| Submitted empty; "Enter a name" on the field and status | [7-form-error.png](evidence/windows/7-form-error.png) |
| Typed "Ada", Tab, Down to "Good morning", submitted | [8-form-result.png](evidence/windows/8-form-result.png) |

### Accessibility

See [accessibility of forms](../forms.md#accessibility) and
[of the canvas](../designed-tree.md#the-canvas). Narrator/NVDA were not
run.

### Custom view (#21)

After a final restart, the smoke opens the Rust command's color picker
and presses Right. Then it clicks the dark green swatch with `user32`
`SetCursorPos` and `mouse_event`, at the screenshot's pixel position.
Each step must show the chosen color over at least 3000 pixels.

In run [36378453278](https://github.com/wasimysaid/pane/actions/runs/36378453278)
(commit `1487dc8`, Windows NT 10.0.26100, AMD64) every step passed. The
picker opened on blue (#1E88E5), Right moved to purple (#8E24AA) and the
click chose dark green (#1B5E20).

The script calls `SetProcessDPIAware` first, so the screenshot, the
screen bounds and `SetCursorPos` all use physical pixels, and the click
should land on the swatch at any display scaling. The runner displays at
100 % scaling, so scaling other than 100 % is unverified.

Screenshots from the same run:
[21-color.png](evidence/windows/21-color.png),
[22-color-key.png](evidence/windows/22-color-key.png),
[23-color-click.png](evidence/windows/23-color-click.png).

### Operations (#22)

The operations phase (screenshots 31 and 32) installs the JavaScript
operations sample, then the Rust one. It opens the Rust sample's command
and fills its form with the JavaScript package's identity and the name
"Rust". The Rust guest then calls that package's `greet` operation
through Pane, the same steps as on [Linux](linux.md#operations-22).

In run [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
(commit `1848494`) every step passed: the Rust guest's call into the
JavaScript package answered "Hello, Rust, from JavaScript"
([32-operation-answer.png](evidence/windows/32-operation-answer.png)).

### Reload (#11)

After the operations phase, the smoke installs a package from
`<output-dir>\dev`, replaces its component with the JavaScript sample and
reloads it in Manage extensions. It then reloads it without its component
(the checks fail and the old code keeps answering) and with the
`failing-start` fixture (a startup failure, then Retry). These are
screenshots 33 to 39, the same steps as on
[Linux](linux.md#reloading-a-package-11).

In run [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
(commit `1848494`) every step passed:

- Reloaded with the JavaScript build, Dev answered from the new code
  ([35-dev-after.png](evidence/windows/35-dev-after.png)).
- Reloading the `failing-start` fixture showed "Reloaded Dev, but it
  failed to start… Retry…"
  ([38-start-failed.png](evidence/windows/38-start-failed.png)).
- Retry then showed "Started Dev"
  ([39-retried.png](evidence/windows/39-retried.png)).

Replacing the managed copy removes the old folder on a best-effort basis.
On Windows a folder still in use is left behind, listed in
`installed.json`, and removal is tried again at the next start. This was
tested on Linux with a folder whose files cannot be deleted; it was not
run on Windows.

### Clearing an extension's cache (#39)

After the reload phase, the smoke restarts Pane and saves one value of
each kind of [extension data](../extension-data.md) for the Settings
sample (style, content, cache and credential). Then it chooses "Clear
cache of Settings sample" in Manage extensions and confirms, the same
steps as on [Linux](linux.md#clearing-an-extensions-cache-39).

In run [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
(commit `1848494`) every step passed. Manage extensions showed "Cleared
the cache of Settings sample"
([42-cache-cleared.png](evidence/windows/42-cache-cleared.png)). "Show
what Pane keeps" then read "Style: formal · Note: Water the plants ·
Signed in: yes · Cached greeting: none": the cached greeting was gone and
the other three values kept
([43-kept-after-clear.png](evidence/windows/43-kept-after-clear.png)).

## Disabling an extension and keeping its settings (#10)

In run [36378453278](https://github.com/wasimysaid/pane/actions/runs/36378453278)
(commit `1487dc8`) the disable phase passed. The Settings sample saved
the formal greeting and was disabled in Manage extensions. It stayed
disabled and absent from root search after a restart (the root
screenshot matches the one taken before the package was installed). It
was enabled again, and "Greet me" answered "Good day to you" from the
kept setting.

| Step | Evidence |
| --- | --- |
| Formal greeting saved | [16-setting-saved.png](evidence/windows/16-setting-saved.png) |
| Disabled in Manage extensions | [17-disabled.png](evidence/windows/17-disabled.png) |
| After a restart: Greeting absent from root | [18-restarted-disabled.png](evidence/windows/18-restarted-disabled.png) |
| Enabled again | [19-enabled.png](evidence/windows/19-enabled.png) |
| The kept setting answers | [20-greeted.png](evidence/windows/20-greeted.png) |

## Local extension package (#9)

`scripts/smoke-windows.ps1` also installs
`target/guests/packages/sample-rust` with `pane --install <folder>` (with
`PANE_DATA_DIR` pointing at a fresh folder), runs its command, and
restarts Pane. In run
[36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770)
(commit `949e35d`) every step passed.

The `packages` identity tests also passed there: folder paths with spaces
and Unicode, letter case and Unicode normalization as this file system
treats them, and symbolic links. The symbolic-link test skips itself
where directory links are not allowed, and cargo hides that notice for a
passing test. The runner's administrator account can normally create
them, but a skip can't be ruled out from the log.

| Step | Evidence |
| --- | --- |
| Package screen: source, version, commands, compatibility | [9-package.png](evidence/windows/9-package.png) |
| Installed; the new command is selected in root search | [10-installed.png](evidence/windows/10-installed.png) |
| The installed command answers ("Hello from the Rust guest") | [11-installed-result.png](evidence/windows/11-installed-result.png) |
| Still listed after a restart | [12-restarted.png](evidence/windows/12-restarted.png) |

Since #162 Pane registers no sample command. The first phase installs the
three samples into a data folder of their own, and this phase's data
folder lists the installed Rust sample alone, so the screenshots show
that copy. In these screenshots the root list is taller than the window
and its last row is cut off; since #19 the list scrolls to keep the
selected row visible.

## Installing Pane and acquiring its calculator (#51)

The PowerShell install script, this smoke phase, the zip package itself
and the release-profile package are pending CI. They need the Windows
build and interactive desktop only the `windows-2025` runner provides.

A final phase, after the clipboard-expiry one, proves the whole outcome
of [#51](https://github.com/pane-app/pane/issues/51)
([installer](../installer.md)). It is the Windows half of what
[#53](linux.md#installing-pane-and-acquiring-its-calculator-53) proved on
Linux.

### The package

`cargo xtask package-windows --dev` builds the Windows package. It is a
zip, because a Windows user unzips with whatever is at hand. It holds:

- `pane.exe`;
- `install.ps1`;
- a README (the development profile, so its program takes its pins from
  `PANE_DEFAULTS`);
- the application-update artifacts.

No default-extension payload is written. First setup fetches the eight
defaults from the commits this release pins: the five that run everywhere
plus the three Windows-only ones installed beside them on Windows.

### What the phase does

The smoke clones the defaults' repositories at those commits from their
real addresses on GitHub (its own setup, on the runner). It serves the
clones on 127.0.0.1 over Git's smart HTTP protocol
(`scripts/repository_server.py`), named by the pins file the development
build reads through `PANE_DEFAULTS`. Nothing the Pane under test does
reaches the network or Pane's published downloads.

1. The package is unzipped into a folder of its own. Its install script
   runs with a clean machine's environment: a fresh user profile that
   holds no data, with `LOCALAPPDATA` and `APPDATA` pointing into the
   smoke's output folder, so the install, Pane's data and the shortcut
   touch nothing of the runner's user.
2. The script installs `pane.exe` to `%LOCALAPPDATA%\Pane` and a `Pane`
   shortcut to the user's Start menu, and runs `pane --version` (its exit
   code) to check what it installed. No administrator rights are
   involved.
3. The installed Pane then starts with a PATH that holds nothing at all:
   an empty folder, checked with `Get-Command` of cargo, rustc, node, npm,
   git, cc, clang and make. A running process's own environment cannot be
   read on Windows, so what is checked is the environment `Start-Process`
   hands the child. Pane takes its pins from the override, and its
   artifact source from the local server the smoke serves the update index
   on.
4. Pane fetches the eight default extensions by itself, with its own Git
   client. `installed.json` must record each under `"default"`, with the
   repository, release tag, commit and pinned state of its pin (checked by
   `scripts/check_git_record.py`), and no sample. The helper sample left
   the default set with #162, and a helper running from an acquired
   revision is `crates/pane-core/tests/installer.rs`'s to test.
5. Root search lists their commands, "6*7" answers 42 and Enter copies
   it, with no developer tool reachable.
6. The downloads folder the fetches used must end empty.

The program files are removed again at the phase's end. So the uploaded
evidence is the screenshots and records (frames 500 to 502), not the
program. CI builds the release-profile package after the smoke and
uploads it with the artifacts of the job (`windows-package`).

### What ran locally

Recorded 2026-09-29 on this branch's machine, a headless aarch64 Linux
with no Windows, and no PowerShell to parse the scripts. What ran is
everything that machine can run:

- `cargo check`, `clippy`, `fmt` and the unit tests of `xtask` with its
  zip writer. The writer's bytes are pinned by a test and were first
  decoded with Python's `zipfile`: the names, sizes, CRCs, the fixed
  1985-10-26 08:15 time and the round-tripped contents.
- `cargo xtask package-windows`, far enough to assemble the whole
  artifact tree, before it refuses with the message that only a Windows
  checkout builds `pane.exe`.
- `cargo xtask package-linux` end to end in both profiles, as the shared
  packaging's regression. The dev package was unpacked and its install
  script run into a temporary home with a scrubbed PATH, and
  `pane --version` answered.
- The acquisition the installed Pane does is the same
  platform-independent code `crates/pane-core/tests/installer.rs` checks.
  Those tests were rerun there, all passing, including the prebuilt helper
  running from the managed copy. That machine's helper file names
  `linux-aarch64`, which CI's Windows run assembles as `windows-x86_64`.

The smoke's clean machine is a fresh profile on that runner, not a fresh
machine. No Windows 10 or 11 client, ARM64 or real desktop install has
been tried. Nothing is signed (no Authenticode credentials exist), so
Windows may warn about an unknown publisher when `pane.exe` runs, and
PowerShell may refuse the install script until the policy question the
README answers is answered.

| Step | Evidence |
| --- | --- |
| The clean machine's Pane acquired the eight default extensions (the five that run everywhere and the Windows-only three beside them), and no sample (#162), and lists their commands | pending CI (frame 500) |
| "6*7" answers 42 | pending CI (frame 501) |
| Enter copies the answer | pending CI (frame 502) |

## Installing a Pane application update by the user's choice (#54)

The PowerShell smoke phase, the `--package-version` Windows build and the
running-exe rename on Windows itself are pending CI. They need the
Windows build and interactive desktop only the `windows-2025` runner
provides.

A final phase, after the installer one, proves the whole outcome of
[#54](https://github.com/pane-app/pane/issues/54)
([installer](../installer.md)).

### Setting up the two versions

The same `cargo xtask package-windows --dev` builds a second package with
`--package-version 99.0.0`: a program that reports 99.0.0, a package
named by it, and an index whose `application` entry names that package
for `windows-x86_64`. Those are the two runnable builds an update goes
between.

The 0.1.0 package the installer phase built is installed on another clean
profile, with its install script, its empty PATH and its own data under
`%LOCALAPPDATA%\Pane\data`. The smoke serves the 99.0.0 artifacts from
127.0.0.1 with `scripts/artifact_server.py`. Nothing reaches the network
or Pane's published downloads.

### What the phase checks

1. The installed 0.1.0 Pane is started with `PANE_ARTIFACTS` and the same
   pinned repositories. It fetches its default extensions and, in the
   same background, checks the index for a newer version of itself. The
   offer appears as Update Pane to 99.0.0 in root search (frame 601),
   and the status line tells what was found (frame 600).
2. The artifact server's log must hold no request for the package until
   the row is chosen. Nothing is downloaded, installed or restarted
   automatically.
3. Clipboard History is disabled first (the Helper sample was, until #162
   took it out of the default set), so an extension the user disabled
   before the update must stay disabled after it.
4. Choosing the row with a damaged package is explained: its bytes do not
   match the sha512 its entry gives (frame 602). The program, the data and
   the folder stay untouched, and the row is ready to try again.
5. The real choice downloads the package, checks it, unpacks it and swaps
   the running `pane.exe`. The old program is renamed `pane.exe.old`, the
   new one takes its place, and the staging folder is gone (frame 603, and
   hash checks of both programs against the two packages' own files).
6. The next start runs the new version. It reports `Pane 99.0.0` and
   removes `pane.exe.old` at start. The calculator still answers "6*7"
   with 42 from the old version's install (the calculator set up at first
   setup, from the pinned repositories; frames 604 and 605). The disabled
   Clipboard History stays disabled, since Pane's data was never touched.

Pane itself was never restarted by the update. The smoke stops the old
process and starts the new program itself, exactly as the user would.

### What ran locally

Recorded 2026-09-30 on this branch's machine, a headless aarch64 Linux
with no Windows, and no PowerShell to parse the scripts. What ran is
everything that machine can run:

- The platform-independent half (`crates/pane-core/tests/app_update.rs`),
  all passing, including the swap, the failures and the data kept.
- The strict zip reader's unit tests.
- `cargo xtask package-linux --dev` (with and without
  `--package-version 99.0.0`) end to end: two packages named by their
  versions, the index carrying the `application` entry naming the
  package, the program reporting the overridden version, and the package
  served from the artifacts folder.

The swap depends on renaming the running `pane.exe`. That is a documented
Windows behavior (renaming a running executable is allowed; overwriting
one is not) that only the runner can prove end to end.

| Step | Evidence |
| --- | --- |
| The check at start tells the user; nothing is downloaded until they choose | pending CI (frames 600, 601) |
| A damaged package is explained, everything untouched, the row retried | pending CI (frame 602) |
| The user's choice swaps the running program; the new one is used the next start | pending CI (frame 603) |
| The new version reports itself; the old version's data and enablement are kept | pending CI (frames 604, 605) |

## Remaining limits

- Only a CI runner (Windows Server) was used, not a Windows 10/11
  desktop.
- The #4 resource and latency workload exists for Linux only. On this
  platform only Pane's cost while hidden has a script
  (`scripts/measure-windows.ps1`, #189), run on the user's machine with
  their consent and not run yet. The other measurements and every target
  are not started
  ([the measurement record](../research/resource-measurements.md)).
- The installer (#51, [above](#installing-pane-and-acquiring-its-calculator-51))
  and the application update
  (#54, [above](#installing-a-pane-application-update-by-the-users-choice-54))
  have run nowhere yet. Their PowerShell scripts and smoke phases were
  written without PowerShell on hand, and their runtime evidence is CI's
  Windows leg. Nothing is signed, so an unknown-publisher warning and
  PowerShell's policy question are expected, not errors.
- No screen reader (Narrator/NVDA) was run. The accessibility tree is
  verified only through GPUI in the platform-independent window tests.
- The smoke confirms that text appears in the expected colors and that
  the three results differ. Which command and guest each screenshot shows
  was checked by inspection.
