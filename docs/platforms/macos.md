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

The smoke forces the dark theme and the opaque material, so its screenshots do not depend on the desktop ([smoke appearance](smoke-appearance.md)).

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

Extensions are managed in Settings since #168, where switches, menus and
confirmation rows answer the pointer only. The smoke opens Settings with
root search's Manage Extensions command, then finds each control by its
accessible name in the Settings window through System Events (Pane's tree
comes from AccessKit; the same Accessibility permission covers it) and
presses it, clicking its center where it offers no press. It checks an
operation's outcome by the name of the page's status line, not by a
color, and closes Settings with Command+W before it goes back to the
launcher. A switch is the `AXCheckBox` of that name: AccessKit reports a
heading's role as `Heading`, not `AXHeading`, so release run 37689872256,
which only ruled out the roles a switch is not, pressed the page's heading
"Settings sample" instead of its switch and waited in vain for "Disabled
Settings sample". Lines of plain text (a preview's details) have no
accessible name, so a preview is waited for by its row (Update, Install).
A command's answer is a toast since #141, which leaves the footer 3
seconds after it appears (run 37612772185 missed Echo's at frame 72), so
such answers are captured until their color shows (`capture_until`).
Release run 37698693722 reached the helper phase, where the disable came
after the helper's ten-second wait had ended (the note said "finished"):
Settings is now opened there the shortest way, without `open_extension`'s
pauses, so the switch is reached within the wait.

It fails if Pane exits. Three checks also run against the `screencapture`
screenshots (`scripts/check_screenshot.py`):

- The root screen draws text in Pane's hint color.
- Each result screen draws text in the result color.
- The three result screens are all different. This catches a smoke that opens
  the same command twice.

Screenshots, cropped to the window (and inspected):

| Step | Evidence |
| --- | --- |
| Root search lists the three samples, installed first into a data folder of their own (#162) | [1-root.png](evidence/macos/1-root.png) |
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

## Root search (#23)

Root search has a query field with focus ([root search](../root-search.md)).
The smoke's search phase (screenshots 24 to 26) types "typescr" with System
Events `keystroke`, opens the only match and runs "Wait briefly", which must
look exactly like step 4, then types "zzz" and presses Return on no results.
In run [36420611977](https://github.com/wasimysaid/pane/actions/runs/36420611977) (commit `6d73d18`) every step passed: typing "typescr" left only TypeScript sample and Enter ran it, and "zzz" showed No results ([24-search.png](evidence/macos/24-search.png), [26-no-results.png](evidence/macos/26-no-results.png)). Input-method composition in the query field is still unverified here.

## Calculator (#27)

The smoke's calculator phase (screenshots 27 to 30) installs the calculator
package, types "6*7", checks the selected answer row, presses Enter to copy
it, then compares typing "42+1" with pasting the copy (Cmd+A, Cmd+V) and typing
"+1", which must look the same. In run [36423871204](https://github.com/wasimysaid/pane/actions/runs/36423871204) (commit `ab91081`) every step passed: "6*7" answered 42, Enter copied it, and pasting then typing "+1" matched typing "42+1", so the system clipboard held "42" ([27-answer.png](evidence/macos/27-answer.png), [30-pasted.png](evidence/macos/30-pasted.png)).

## Applications (#25)

[Applications](../applications.md) finds the `.app` bundles in
`/Applications`, `/System/Applications` and `~/Applications` (and their
subfolders such as `Utilities`) and opens one with `/usr/bin/open`. The
smoke's last phase (screenshots 44 and 45) makes a bundle "Pane Smoke App"
whose program is a shell script writing a marker file, in `~/Applications`
of a HOME given to Pane only, installs the package, types "pane smoke",
checks the selected row, presses Return and checks "Opened Pane Smoke App"
and the marker. The adapter tests also open such a bundle and require
Calculator among the system's applications. **Not run on macOS yet**: this
branch was not pushed, so the phase, the native tests and the
`open`/Launch Services path are unverified here, including whether Launch
Services runs an unsigned script bundle on the runner.

## Quicklinks (#28)

The smoke's last phase (screenshots 46 and 47) installs the Quicklinks
package, creates "Pane issues" (https://example.com/pane-issues) in its
form, restarts Pane and types "pane iss", which must list it selected. It
stops before Enter, which would open the default browser; opening a link
here is checked only through the tests' recording opener. Not run yet.

## Global hotkeys (#33)

The smoke's hotkey phase (screenshots 52 to 58, [global hotkeys](../hotkeys.md#checks))
assigns Control+Option+G to Greeting in the hotkey recorder on the settings
sample's page in Settings (#168; it was the launcher's hotkey screen
before), brings Finder to
the front, presses it through System Events and checks that Pane is the
frontmost process with Greeting open; then again after a restart; then,
with the extension disabled, that pressing it leaves Finder in front and
Pane's window unchanged. The hotkey is a Carbon hot key
(`RegisterEventHotKey`), which needs no Accessibility or Input Monitoring
permission of Pane's own (System Events, which sends the keys, needs the
Accessibility permission the runners grant). The adapter code was only
compile- and lint-checked for `x86_64-apple-darwin` from Linux; **not run
on macOS yet**, so registration, delivery on the main run loop and the
focus transition are unverified natively.

## Deleting retained data (#41)

The smoke's retained-data phase (screenshots 63 to 65, [deleting retained
data](../extension-data.md#deleting-retained-data)), with a data folder of
its own, saves a note with the settings sample, uninstalls it keeping its
saved data, deletes its retained data from its row on the Extensions
group's page in Settings and confirms (#168), checks that `installed.json`
and `content.json` no longer hold it, and reinstalls the same folder, which
must show nothing kept. **Not run on macOS yet.**

## Aliases and fallbacks (#31)

The smoke's alias phase (screenshots 66 to 74, [aliases and fallbacks](../aliases.md#checks))
gives Echo, the query sample's command, the alias "ec" and makes it a
fallback on its extension's page in Settings (its alias cell and fallback
switch, #168), sends "ec hello" and "zqx" to it — the first fallback
preselected when nothing else matches (ADR 0031), so Enter sends "zqx"
without a move — and checks that with the
extension disabled "ec hello" lists nothing. Nothing in it is specific to
macOS (no system API is involved); **not run on macOS yet**.

## Dependencies (#42)

The dependencies phase (screenshots 75 to 77, [dependencies](../dependencies.md#checks)),
with a data folder of its own, previews the dependencies sample (its
required JavaScript operations sample and optional Rust one listed),
installs it with the JavaScript sample and runs "Greet through the required
greeter", which must answer from the JavaScript guest; `installed.json`
must then hold exactly two packages and the recorded dependency. The logic
is platform-independent except path resolution, which uses the same
`canonicalize` as package identity. **Not run on macOS yet.**

## Native helpers (#15)

The first native-helper phase (screenshots 90 to 93, data folder `helper-data`,
[native helpers](../helpers.md#checks)) installs the helper sample, whose
`pane-echo` `cargo xtask guests` builds for the runner (`macos-aarch64` on
`macos-15`), runs it (the answer must name macOS arm64), cancels a slow run
after a second, starts the ten-second run, checks with `pgrep` that the
helper runs from the managed copy, disables the package and checks that the
process is gone, that the saved "started" note is kept, and that no helper
outlives Pane. A second phase (screenshot 94, `helper-quit-data`) starts
the waiting helper, asks Pane to quit with a quit Apple event
(`NSRunningApplication.terminate`, through Python's ctypes), and checks
that Pane exits, no helper runs and its heartbeat file stops growing. The
tests in `crates/pane-core/tests/helpers.rs` (Rust, JavaScript and
TypeScript samples) and the runner's unit tests (Mach-O headers) run in
`cargo xtask ci` there, against the `pane-echo` built natively on the
runner. The runner was only compile- and lint-checked for
`x86_64-apple-darwin` from Linux; **not run on macOS yet**, so starting,
ending and reaping a helper natively, and the executable permission of the
copied file, are unverified there, and whether GPUI runs Pane's quit
handler for a quit Apple event is unverified until the smoke runs. No macOS x86-64 build was made.

## Development mode (#12, #13)

The smoke's last phase (screenshots 110 to 136, [development
mode](../development-mode.md#checks)) builds a copy of each development
sample, develops it from its page in Settings (Develop in the Actions
menu, #168), saves an edit, a change that
does not build, two saves in a row and, after stopping, one more, checking
the answers, the error and that nothing is built after stopping. The
TypeScript and JavaScript samples run only where the JS toolchain is built,
so CI's smoke runs the Rust one. The platform code (FSEvents through notify, with the folder and event
paths made canonical, and a process group killed with `SIGKILL`) was only
compile- and lint-checked for `x86_64-apple-darwin` from Linux; **not run on
macOS yet**, so the file watcher's events, the build's processes being
killed and the whole phase are unverified natively. macOS has no parent
death signal, so a build outlives a Pane that is killed or crashes (one
that quits kills it).

## Disabling required dependents (#43)

The disable-dependents phase (screenshots 140 to 143, [disabling a required dependency](../dependencies.md#disabling-a-required-dependency)),
with a data folder of its own, installs the dependencies sample, turns off
the JavaScript operations sample's switch on its page in Settings (#168),
which must ask first (its Disable all row shown), cancels, then chooses
Disable all (the page's status says so) and enables the JavaScript sample
again alone; `installed.json` must then record exactly one disabled package.
Nothing in it is specific to macOS (no system API is involved; the
closure reuses the dependency identities recorded at install); **not run on
macOS yet**.

## Runtime crashes (#17)

The runtime-crash phase (screenshots 200 to 209, data folder
`runtime-crash-data`, [runtime crashes](../pausing.md#when-the-extension-runtime-itself-crashes))
installs the helper and settings samples, starts Pane with
`PANE_TEST_RUNTIME_FAULTS` naming a fault file, runs the settings sample's
Count, starts the waiting helper and has the runtime crash: the helper must
be gone (`pgrep`, and its heartbeat must stop growing), the note it saved kept,
and the status line the error color. A second crash, injected before
Count's answer, must leave the count at 2 and the runtime stopped; root
search explains it, the Extensions group's page in Settings shows why (its
runtime rows, and the details behind one), a disable works, **Restart the
extension runtime** runs extensions again and
Count then counts 3; no package may be recorded as paused. Nothing in it is
specific to macOS (the runtime is a thread; helpers are ended as for a
disable); **not run on macOS yet**.

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
specific to macOS; **not run on macOS yet**.

## Uninstalling required dependents (#44)

A phase of the smoke (screenshots 180 to 183, [uninstalling a required dependency](../dependencies.md#uninstalling-a-required-dependency)),
with a data folder of its own, installs the dependencies sample, chooses
Uninstall in the Actions menu of the JavaScript operations sample's page in
Settings (#168), which must ask first, cancels, then chooses Uninstall all 2
keeping saved data (the page's status says so); `installed.json` must then hold
no package. Pane is started again to install the JavaScript operations
sample alone, and `installed.json` must then hold exactly one package.
Removing a managed copy uses the same `remove_dir_all` as a single
uninstall; a folder still in use is listed and removed at the next start,
reported against its own package. Nothing else in it is specific to
macOS; **not run on macOS yet**.

## npm packages (#45)

A phase of the smoke (screenshots 260 to 268, [npm packages](../npm.md)),
with a data folder of its own, starts `scripts/npm_registry.py` on
127.0.0.1 serving the npm sample `cargo xtask guests` packed, and points the
development build at it with `PANE_NPM_REGISTRY`: it installs the local
Dependencies from npm sample, which downloads and installs the npm package
it requires, calls its `greet` operation, then names the npm package in
the npm field "Install extension from npm…" opens in Settings (#168; Show
Package previews it there), updates it and runs
its command, which answers "Hello from the npm package". #49 extends the
phase: a 0.2.0 of the sample is published into the registry's folder
(`scripts/npm_publish.py`; the registry reads its folder on request), Pane
is stopped and started again, and the check a second after its start
replaces the installed unpinned copy by itself — nothing of it running —
saying "Updated Greeter from npm to 0.2.0" (267), the new copy's command
answering as before (268); `installed.json`
must then record `"npm": "@pane-samples/greeter"` at `"npmVersion":
"0.2.0"` with both packages. The automatic-update frames have not been
captured; the phase is **not run on macOS yet**. The real registry would be reached through the
HTTP client guests' requests use, trusting the certificates rustls-native-certs
reads from the system keychains, which no check exercises (the smoke never
reaches the network; [by hand](../npm.md#trying-the-real-registry-by-hand),
not run); nothing else in it is specific to macOS.
The packing, in `cargo xtask guests`, runs in CI on macOS. The phase's
first run, in CI (run 36536816781 of the fork `wasimysaid/pane`, at
ca9563b), stopped before Pane started: "the local npm registry did not
start", with nothing in `npm-registry.log` and no port file after the
smoke's 5 s wait. The likely cause, not confirmed on macOS: Python's
`HTTPServer.server_bind` looks the address up in reverse DNS
(`socket.getfqdn`) before it listens, which is reported to take many
seconds on macOS runners. Both local servers (`npm_registry.py` and
`repository_server.py`) now skip that lookup, whose name nothing uses, and
the smoke waits up to 60 s for the port file, stopping early if the server
exits. The fix was checked by reading only, and on Linux with reverse
lookups slowed to 8 s (the old servers then failed the same way). **Not run
on macOS yet.**

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
`installed.json`, read as JSON (`scripts/check_git_record.py` on macOS), must then
record one package from Git with `"gitRef": "refs/tags/v0.1.0"`, `pinned`
and the `gitCommit` the tag points to (`repository_server.py commit`), and
the downloads folder must be empty. Pane itself runs no
`git`; the address is plain `http://` on a loopback address, which only a
development build fetches, so the smoke never reaches the network, and the
HTTPS path to a real host ([by hand](../git.md#trying-a-real-host-by-hand))
is not exercised. Tree names are refused alike on every system (the same
rules as npm's, plus `.git`, `git~1` and names differing only in case).
The phase types the address with System Events. Its repository server
skips the reverse DNS lookup the npm phase's first run stopped on (above)
and is waited for as long; checked by reading only. **Not run on macOS
yet.**

#50 extends the phase with a second repository of the same sample
(`make-sample` again, `greeter-tracked` beside `greeter`, served by the
same server), installed in a data folder of its own from its tracked
`release` branch (`--install git:<address>@release`: the preview says
"Revision: branch release, tracked: an update fetches that branch again",
305; installed, 306); `repository_server.py move-sample` then commits a
0.2.0 on the branch while Pane is stopped, and the check a second after
the restart replaces the installed copy by itself, saying "Updated
Greeter from Git to 0.2.0" (307), the new copy's command answering as
before (308); `installed.json` must record the branch's new commit,
tracked and unpinned. Written and checked by reading only; **not run on
macOS yet**.

## Files (#29)

The files phase (screenshots 220 to 224, [files](../files.md)), with a data
folder of its own, installs Files (220, "Installed Files"), whose file
index (#175) covers a fixture folder "Pane smoke files" (spaces) in the
system's temporary folder that a debug build's `PANE_TEST_FILE_INDEX_HOME`
names instead of the home folder; there is no folder to choose any more.
It types "plan" in root search, which must list "Résumé plan ü.txt" under
"Files", selected, and presses Return. Files installed from its folder is
not the registered default, so its Search Files command keeps the generic
list (#177): the smoke does not open it. A debug build's
`PANE_TEST_OPEN_FILE_LOG` makes the opener record the path in a file instead
of running `/usr/bin/open`, so no application of the user's opens it; the
recorded path, resolved, must be the fixture file's, resolved. Last it types
"runner" and presses Return on an executable script, which Pane must show
in Finder without recording or running it (ADR 0037). A positive native
open on macOS is
therefore not run by the smoke. Listing the folder and the scan policy use
only `std::fs` (case-insensitive APFS sorts names by bytes like the other
systems; links are skipped); the policy tests with symbolic links and the
executable bit run on macOS in CI. **Not run on macOS yet.**

## Searching inside a command (#30)

The smoke's search phase (screenshots 160 to 169, [command search](../command-search.md#checks)),
with a data folder of its own, builds and starts the fixture service on a
free port of 127.0.0.1 (`fixture_service --port 0`, the port read from its
log) and installs Package search, the Rust search sample. "aurora" typed in
root search must leave the service's log without a request; opened, the
command's "Service address" form is set to the service; its search field
sends "aurora" (results listed, Enter shows a package's details); "slow" then "ember"
must log the held search as abandoned; the service's 503, then the service
stopped, are errors; restarted, a search lists results again.
**Not run on macOS yet.**

## Clipboard history (#35, #37)

macOS gets the pasteboard adapter of clipboard history
([`macos.rs`](../../crates/pane-core/src/clipboard/macos.rs)) behind the
same `ClipboardSystem` trait as the Windows and Linux ones: a thread of
Pane's own polls the pasteboard's `changeCount` every 250 ms (macOS'
`NSPasteboardDidChangeNotification` needs a run loop Pane's own threads do
not run; the count is the established mechanism without one) and, when the
count moved, reads the pasteboard's types first, then the text
(`NSPasteboardTypeString`) only if no type is
`org.nspasteboard.ConcealedType`, the nspasteboard.org convention that
password managers such as 1Password and Strongbox set — honored, with the
text never read when it is present. The pasteboard never names the program
that copied, so every report's owner is unknown and no program can be
excluded by name (an excluded name never matches, as the contract says of
an unknown owner). `write_text` puts the text in the pasteboard server
(`clearContents` + `setString:forType:`), which holds it for whoever pastes
beyond the watch; no permission is needed. What is kept is decided by the
same `clipboard::accept`, unchanged, and the retention/storage contract is
reused as is (the fake-clipboard tests cover it).

The smoke's clipboard phase (screenshots 280 to 287, on the pattern of the
Windows and Linux ones), with a data folder of its own, runs after the #52
phase: since #166 only Pane's registered Clipboard History records from
the first start, so the phase acquires the default set from the artifacts
that phase built, served on 127.0.0.1, with the smoke's own build (Files'
index on an empty folder, `PANE_TEST_FILE_INDEX_HOME`). It keeps the
smoke's own copies with nothing turned on (put on the pasteboard with
AppleScript's `set the clipboard to`), pauses and resumes recording in the
split view's Actions panel (Command+K), copies a kept record again (Return
on it, the view's filter leaving it) and checks the pasteboard with
`pbpaste` and by pasting into root search, disables it (the switch on its
page in Settings), restarts disabled, enables and keeps again across a
restart, with `clipboard-history.json` checked at each step. The expiry
and deletion phase (400 to 405) backdates the kept records through
`scripts/clipboard_history.py`, checks the expired one is gone after the
restart, deletes one record (Delete Entry), keeps records for 1 Hour,
which deletes the older one, copies a file as Finder does (a file URL,
kept as a file, #167), and clears the history once confirmed (Clear
History), recording going on, and checks the pasteboard still holds what
was copied last (`pbpaste`). No marked copy is made in the smoke
(AppleScript cannot set a custom type); the concealed marker is checked by
the adapter test instead.

**Not run on macOS yet.** The adapter was written on this headless Linux
machine, where macOS code cannot build: it was reviewed by reading and
type-checked against the real `objc2`/`objc2-app-kit` bindings by compiling
the adapter and its test for `aarch64-apple-darwin` in a scratch crate with
the bindings from this lock file (checking only, no linking or running).
CI's macOS leg is the compile and runtime evidence:
`clipboard_adapter_macos.rs` (opt-in through `PANE_TEST_REAL_CLIPBOARD=1`,
set for the macOS runner) runs against the runner's pasteboard, and the
next green macOS run of the smoke is the native-capture evidence. A
password manager's own copies (1Password, Strongbox) have not been run
anywhere; the convention's type name is what the test checks.

## Clipboard history expiry (#36)

Covered by the same smoke phase (screenshots 400 to 405, above) since the
command now runs on macOS: records expire after their package's retention
whether or not the extension runs, and the phase backdates and deletes
records as the Linux one does, plus a copied file (#167), with the
pasteboard checked directly (`pbpaste`). **Not run on macOS yet.**


## Installing Pane and acquiring its calculator (#52)

A final phase, after the clipboard-expiry one, proves the whole outcome of
[#52](https://github.com/pane-app/pane/issues/52)
([installer](../installer.md)), the macOS half of what
[#53](linux.md#installing-pane-and-acquiring-its-calculator-53) proved on
Linux and [#51](windows.md#installing-pane-and-acquiring-its-calculator-51)
on Windows. `cargo xtask package-macos --dev` builds the macOS package —
a zip, like the Windows one, opened with one double-click in Finder and
written by the same fixed-bytes writer — holding the `pane` program, the
bash `install.sh`, a README and the bundle's `Info.plist` (the development
profile, so its program accepts the controlled artifact source) and the
default extensions' payloads; the smoke serves `target/dist/artifacts`
from 127.0.0.1 with `scripts/artifact_server.py` (nothing reaches the
network or Pane's published downloads). The package is unzipped into a
folder of its own and its install script runs with a **clean machine's**
environment: a fresh home folder and `PATH=/usr/bin:/bin`, so the home
holds no data and no development tool is configured. It builds the
`Pane.app` bundle in that home's `~/Applications` (the user's own folder,
so no administrator rights) from the program and the `Info.plist`, and
runs `pane --version` to check what it installed. The installed Pane then
starts — the bundle's `Contents/MacOS/pane` — with a PATH that holds
nothing at all (an empty folder, checked with `command -v` of cargo,
rustc, node, npm, git, cc, clang and make), pointed at the controlled
source with `PANE_ARTIFACTS`. It acquires the five default extensions
by itself (`installed.json` must record each under `"default"`, and no
sample: the helper sample left the default set with #162, and a helper
running from an acquired payload is
`crates/pane-core/tests/installer.rs`'s), root search lists their
commands, "6*7" answers 42 and Enter copies it, with no developer tool
reachable. The acquired payloads must be cached and the downloads folder
empty. The program files are removed again at the phase's end, so the
uploaded evidence is the screenshots and records (frames 500 to 502, and
the `clean-home-records` folder), not the program. CI builds the release-profile package after the smoke and
uploads it with the artifacts of the job (`macos-package`). Nothing is
signed (no Apple Developer credentials exist): the smoke's binaries are
built on the runner, so they carry no Gatekeeper quarantine mark; a
package downloaded from the internet would, and its first launch would
be blocked until allowed in System Settings — recorded as an execution
prerequisite in [the installer's limits](../installer.md#limits-and-prerequisites).

**Recorded 2026-09-30, this branch's machine (headless aarch64 Linux —
no Apple toolchain, so the macOS `pane` program and its package cannot
be built here):** what ran locally is everything that machine can run:
`cargo check`, `clippy`, `fmt` and the unit tests of `xtask`; `cargo
xtask package-macos` far enough to assemble the whole artifact tree
(the helper sample's payload manifest rewritten to this machine's
target, `linux-aarch64`, by the same shared code a macOS run rewrites
to `macos-aarch64`), before it refuses with the message that only a
macOS checkout builds the program — the same refusal `package-windows`
makes here; and `cargo xtask package-linux` end-to-end as the shared
packaging's regression. The install script is bash, so it was linted
(`bash -n`) and dry-run on this machine with a fake `pane` binary
through its logic paths: the per-user `Pane.app` install with a clean
environment, the `--app-dir` override, the missing-file and usage
errors, and the `--version` check propagating the program's exit code;
its one macOS-specific claim — that Gatekeeper does not stop the
script's direct run — is read-reviewed. The acquisition the installed
Pane does is the same platform-independent code
`crates/pane-core/tests/installer.rs` checks (rerun here, all passing,
including the prebuilt helper running from the managed copy). The
`Info.plist` the package ships was generated and parsed back
(`plistlib`) with the keys Launch Services reads. The bash smoke phase
is syntax-checked and reviewed only; the zip package itself, the
release-profile package and the smoke phase are **pending CI**: they
need the macOS build and interactive desktop only the `macos-15`
runner provides.

| Step | Evidence |
| --- | --- |
| The clean machine's Pane acquired the five default extensions, and no sample (#162), and lists their commands | pending CI (frame 500) |
| "6*7" answers 42 | pending CI (frame 501) |
| Enter copies the answer | pending CI (frame 502) |

## Installing a Pane application update by the user's choice (#55)

A final phase, after the installer one, proves the whole outcome of
[#55](https://github.com/pane-app/pane/issues/55)
([installer](../installer.md)), the macOS half of what
[#54](windows.md#installing-a-pane-application-update-by-the-users-choice-54)
proved on Windows. The same `cargo xtask package-macos --dev` builds a
**second** package with `--package-version 99.0.0`: a program that
reports 99.0.0, a package named by it, and an index whose `application`
entry names that package for `macos-aarch64` — the two runnable builds
an update goes between. The 0.1.0 package the installer phase built is
installed on another clean home (its install script, its empty PATH, its
own data under `~/Library/Application Support/Pane`), and the smoke
serves the 99.0.0 artifacts from 127.0.0.1 with
`scripts/artifact_server.py` (nothing reaches the network or Pane's
published downloads). The installed 0.1.0 Pane, started with
`PANE_ARTIFACTS`, acquires its default extensions and, in the same
background, checks the index for a newer version of itself: the offer
appears as **Update Pane to 99.0.0** in root search (frame 601; the
status line tells what was found, frame 600). The artifact server's log
must hold **no request for the package** until the row is chosen —
nothing is downloaded, installed or restarted automatically. Clipboard
History is disabled first (the Helper sample was, until #162 took it out
of the default set), so an extension the user disabled before the update
must stay disabled after it. Choosing the row with a **damaged
package** is explained (its bytes do not match the sha512 its entry
gives, frame 602) with the program, the data and the bundle untouched
and the row ready to try again; then the real choice downloads the
package, checks it, unpacks it and swaps the running binary **inside the
bundle** — the old `Contents/MacOS/pane` renamed `pane.old` beside it,
the new one in its place, the staging folder gone (frame 603, and byte
comparisons of both binaries against the two packages' own files). The
next start runs the new version: it reports `Pane 99.0.0`, removes
`pane.old` at start, the calculator still answers "6*7" with 42 from
the old version's acquired payload (frames 604 and 605, and the
pasteboard holds 42), and the disabled Clipboard History stays disabled —
Pane's data was never touched. The bundle itself is never replaced: the
same `Pane.app` keeps its identity, and what the swap does not update
(the `Info.plist` version keys) is a provisional limit recorded in the
[installer](../installer.md#updating-pane-itself). Pane itself was never
restarted by the update: the smoke stops the old process and starts the
new program itself, exactly as the user would.

**Recorded 2026-09-30, this branch's machine (headless aarch64 Linux —
no Apple toolchain, so the macOS `pane` program and its package cannot
be built here):** what ran locally is everything that machine can run:
the platform-independent half (`crates/pane-core/tests/application_update.rs`,
all passing, including the swap, the failures and the data kept),
`cargo check` and the unit tests of `pane-core` and `xtask`, and `cargo
xtask package-macos --dev --package-version 99.0.0` far enough to
assemble the whole artifact tree before it refuses with the message
that only a macOS checkout builds the program — the same refusal the
installer phase records. The macOS wiring itself is one `cfg` widened in
`pane`'s `main.rs` around the identical call the Windows build makes,
so it was compile-checked by reading (the Linux checkout compiles the
file without the macOS half, exactly as without the Windows one). The
bash smoke phase is syntax-checked (`bash -n`) and reviewed only; the
99.0.0 package, the bundle-binary swap and the smoke phase are **pending
CI**: they need the macOS build and interactive desktop only the
`macos-15` runner provides. The running-binary rename the swap depends
on (renaming a running program's file is allowed; overwriting one is
not, and the running process keeps its open file) is a POSIX behavior
read-reviewed, only provable end to end on the runner.

| Step | Evidence |
| --- | --- |
| The check at start tells the user; nothing is downloaded until they choose | pending CI (frames 600, 601) |
| A damaged package is explained, everything untouched, the row retried | pending CI (frame 602) |
| The user's choice swaps the running binary in the bundle; the new one is used the next start | pending CI (frame 603) |
| The new version reports itself; the old version's data and enablement are kept | pending CI (frames 604, 605) |

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
  chosen color over at least 3000 pixels. In run [36378453278](https://github.com/wasimysaid/pane/actions/runs/36378453278) (commit `1487dc8`, macOS 15.7.9, arm64) every step passed: the picker opened on blue (#1E88E5), Right moved to purple (#8E24AA) and the click chose dark green (#1B5E20); posting the Quartz event needed no permission beyond the one System Events has. Accessibility: see
  [custom views](../custom-views.md#accessibility).
- **Operations (#22):** the operations phase (screenshots 31 and 32) installs
  the JavaScript operations sample, then the Rust one, opens the Rust
  sample's command and fills its form with the JavaScript package's identity
  and the name "Rust"; the Rust guest calls that package's `greet` operation
  through Pane, the same steps as on
  [Linux](linux.md#operations-22). In run
  [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
  (commit `1848494`) every step passed: the Rust guest's call into the
  JavaScript package answered "Hello, Rust, from JavaScript"
  ([32-operation-answer.png](evidence/macos/32-operation-answer.png)).
- **Reload (#11):** after the calculator and operations phases, the smoke installs a package from
  `<output-dir>/dev`, replaces its component with the JavaScript sample and
  reloads it in Manage extensions, then reloads it without its component (the
  checks fail and the old code keeps answering) and with the `failing-start`
  fixture (a startup failure, then Retry); screenshots 33 to 39, the same
  steps as on [Linux](linux.md#reloading-a-package-11). In run
  [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
  (commit `1848494`) every step passed: reloaded with the JavaScript build,
  Dev answered from the new code
  ([35-dev-after.png](evidence/macos/35-dev-after.png)); reloading the
  `failing-start` fixture showed "Reloaded Dev, but it failed to start…
  Retry…" ([38-start-failed.png](evidence/macos/38-start-failed.png)), and
  Retry then showed "Started Dev"
  ([39-retried.png](evidence/macos/39-retried.png)).
- **Clearing an extension's cache (#39):** after the reload phase, the smoke
  restarts Pane, saves one value of each kind of
  [extension data](../extension-data.md) for the Settings sample (style,
  content, cache and credential), then chooses "Clear cache of Settings
  sample" in Manage extensions and confirms, the same steps as on
  [Linux](linux.md#clearing-an-extensions-cache-39). In run
  [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
  (commit `1848494`) every step passed: Manage extensions showed "Cleared the
  cache of Settings sample"
  ([42-cache-cleared.png](evidence/macos/42-cache-cleared.png)), and "Show
  what Pane keeps" then read "Style: formal · Note: Water the plants ·
  Signed in: yes · Cached greeting: none", with the cached greeting gone and
  the other three values kept
  ([43-kept-after-clear.png](evidence/macos/43-kept-after-clear.png)).

## Disabling an extension and keeping its settings (#10)

In run [36378453278](https://github.com/wasimysaid/pane/actions/runs/36378453278) (commit `1487dc8`) the disable phase passed: the Settings sample
saved the formal greeting, was disabled in Manage extensions, stayed disabled
and absent from root search after a restart (the root screenshot matches the
one taken before the package was installed), was enabled again, and "Greet me"
answered "Good day to you" from the kept setting.

| Step | Evidence |
| --- | --- |
| Formal greeting saved | [16-setting-saved.png](evidence/macos/16-setting-saved.png) |
| Disabled in Manage extensions | [17-disabled.png](evidence/macos/17-disabled.png) |
| After a restart: Greeting absent from root | [18-restarted-disabled.png](evidence/macos/18-restarted-disabled.png) |
| Enabled again | [19-enabled.png](evidence/macos/19-enabled.png) |
| The kept setting answers | [20-greeted.png](evidence/macos/20-greeted.png) |

Custom view screenshots from the same run: [21-color.png](evidence/macos/21-color.png),
[22-color-key.png](evidence/macos/22-color-key.png),
[23-color-click.png](evidence/macos/23-color-click.png).

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

Since #162 Pane registers no sample command: the first phase installs the
three samples into a data folder of their own, and this phase's data folder
lists the installed Rust sample alone, so the screenshots show that copy. In these screenshots the root list is taller than the window and its
last row is cut off; since #19 the list scrolls to keep the selected row
visible.

## Remaining limits

- Only a CI runner was used; no one has run it on a contributor's own Mac.
- The #4 resource and latency workload exists for Linux only; this
  platform's measurements and targets are not started
  ([the measurement record](../research/resource-measurements.md)).
- Intel Macs, other macOS versions and the JS/TS toolchain build on macOS are
  untested. The wasi-sdk checksum for macOS in `tools/componentize-js/pins.json`
  has not been checked against a real download.
- The smoke confirms that text appears in the expected colors and that the
  three results differ. Which command and guest each screenshot shows was
  checked by inspection.
