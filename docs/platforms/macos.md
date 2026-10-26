# macOS native baseline (#7)

This page records what Pane has been checked to do on macOS, how it was
checked, and what has not run there yet. It is for contributors and anyone
who wants to know how far to trust Pane on a Mac. Most of it follows the
native GUI smoke test, `scripts/smoke-macos.sh`, one phase at a time.

## Summary

Pane builds and passes its checks on macOS 15 on Apple silicon. The core
launcher works there natively: rendering, keyboard focus, selection,
running guests and showing results. All of it ran on a GitHub-hosted
runner; no one has run it on a contributor's own Mac.

### What to install

- To build: the Xcode Command Line Tools (the README's macOS prerequisite)
  and the Rust toolchain that `rustup toolchain install` reads from
  `rust-toolchain.toml`.
- To run the smoke: Python 3 with Pillow, and the Accessibility permission
  for the terminal that runs it.
- To install Pane itself: the macOS package is a zip whose `install.sh`
  builds `Pane.app` in your own `~/Applications`, with no administrator
  rights ([installing Pane](#installing-pane-and-acquiring-its-calculator-52)).
  Nothing is signed, so a package downloaded from the internet would be
  blocked at first launch until you allow it in System Settings. The
  package itself is still pending CI.

### What has run

| Feature | On macOS |
| --- | --- |
| [Build and `cargo xtask ci`](#fresh-checkout-build-and-checks) | Passed |
| [Native GUI smoke](#native-gui-smoke) | Passed |
| [Platform availability (#19)](#platform-availability-19) | Passed; the later #19 fixes have not run |
| [Root search (#23)](#root-search-23) | Passed; input-method composition unverified |
| [Calculator (#27)](#calculator-27) | By-hand phase passed; installer phase pending CI |
| [Text input (#20), custom view (#21), operations (#22), reload (#11), clearing a cache (#39)](#text-input-and-accessibility-findings) | Passed; IME composition unverified, VoiceOver not run |
| [Disabling an extension (#10)](#disabling-an-extension-and-keeping-its-settings-10) | Passed |
| [Local extension package (#9)](#local-extension-package-9) | Passed |
| [Opening a web link (#28)](#opening-a-web-link-28) | Only through the tests' recording opener |
| [Applications (#25)](#applications-25), [global hotkeys (#33)](#global-hotkeys-33), [deleting retained data (#41)](#deleting-retained-data-41), [aliases (#31)](#aliases-and-fallbacks-31), [dependencies (#42)](#dependencies-42), [native helpers (#15)](#native-helpers-15), [development mode (#12, #13)](#development-mode-12-13) | Not run on macOS yet |
| [Disabling (#43)](#disabling-required-dependents-43) and [uninstalling (#44)](#uninstalling-required-dependents-44) required dependents, [runtime crashes (#17)](#runtime-crashes-17), [extensions that stop responding (#18)](#extensions-that-stop-responding-18) | Not run on macOS yet |
| [npm (#45)](#npm-packages-45) and [Git (#46)](#git-packages-46) packages, [files (#29)](#files-29), [searching inside a command (#30)](#searching-inside-a-command-30), [clipboard history (#35, #37)](#clipboard-history-35-37) and [its expiry (#36)](#clipboard-history-expiry-36) | Not run on macOS yet |
| [Installing Pane (#52)](#installing-pane-and-acquiring-its-calculator-52), [updating it (#55)](#installing-a-pane-application-update-by-the-users-choice-55) | Pending CI |
| [System notifications and the ringing sound (#559)](#alerts-system-notifications-and-the-ringing-sound-559) | The sound and the refusals passed on CI; an unsigned bundle is refused notifications; not seen or heard by a person |

## Tested combination

The baseline was recorded 2026-09-28 from GitHub Actions run
[36366760796](https://github.com/wasimysaid/pane/actions/runs/36366760796) on the
fork `wasimysaid/pane`, commit `572d629`.

| | |
| --- | --- |
| OS | macOS 15.7.9 (build 24G830), from the smoke's `sw_vers` |
| Architecture | arm64 (Apple silicon) |
| Machine | GitHub-hosted runner, image `macos-15-arm64` version 20260907.0337.1 |
| Session | the runner's logged-in GUI session: WindowServer, Finder, Dock, real display surface |
| Rust | 1.98.1 (`rust-toolchain.toml`) |

Not tested, and so not claimed: Intel (x86_64) Macs, macOS 14 or earlier
and 26 or later, a physical Mac with a real display, Retina scaling other
than the runner's, and any signed or notarized build.

## Fresh checkout build and checks

The CI job checks out the repository and runs only the documented
commands. The README's macOS prerequisite, the Xcode Command Line Tools,
is preinstalled on the runner.

```sh
rustup toolchain install
cargo xtask ci          # guests, prebuilt JS/TS check, fmt, clippy, all tests
cargo build --locked -p pane
```

`cargo xtask ci` passed (13 window, 9 launcher-model, 1 runtime-cache and
25 sample-contract tests), and the launcher built. No Windows machine or
PowerShell was involved.

The Rust guests are built natively; the JS and TS samples use the
committed prebuilt components. Rebuilding those from source on macOS
(`cargo xtask js-guests`) has not been run. The Cargo cache
(`Swatinem/rust-cache`) was warm, so this is a fresh checkout but not a
cold `target/`.

## Native GUI smoke

The smoke forces the dark theme and the opaque material, so its
screenshots do not depend on the desktop
([smoke appearance](smoke-appearance.md)).

```sh
cargo build -p pane
scripts/smoke-macos.sh smoke            # needs Python 3 with Pillow
```

The script launches `target/debug/pane`, brings it to the front and sends
real key events through System Events (`osascript`). That needs the
Accessibility permission for the calling terminal, which GitHub's macOS
runners grant. For each of the Rust, JavaScript and TypeScript sample
commands, it presses Return to open it, Down and Return to run "Wait
briefly" (an async WASI 0.3 clock import inside the guest), then Escape.

### Driving Settings

Extensions are managed in Settings since #168
([ADR 0043](../adr/0043-extensions-are-managed-in-settings-one-page-per-extension.md)),
where switches, menus and an alert's buttons answer the pointer only. The
smoke opens Settings with root search's Manage Extensions command. It
finds each control by its accessible name in the Settings window through
System Events and presses it, clicking its center where it offers no
press. Pane's tree comes from AccessKit, and the same Accessibility
permission covers it.

The smoke checks an operation's outcome by the name of the page's status
line rather than by a color. It closes Settings with Command+W before it
goes back to the launcher.

A few details come from failed runs:

- A switch is the `AXCheckBox` of that name. AccessKit reports a heading's
  role as `Heading`, not `AXHeading`, so release run 37689872256, which
  only ruled out the roles a switch is not, pressed the page's heading
  "Settings sample" instead of its switch and waited in vain for "Disabled
  Settings sample".
- Lines of plain text (a preview's details) have no accessible name, so
  the smoke waits for a preview by its row (Update, Install).
- A command's answer is a toast since #141, which leaves the footer 3
  seconds after it appears. Run 37612772185 missed Echo's at frame 72, so
  such answers are now captured until their color shows (`capture_until`).
- Release run 37698693722 reached the helper phase, where the disable came
  after the helper's ten-second wait had ended (the note said "finished").
  Settings is now opened there the shortest way, without
  `open_extension`'s pauses, so the switch is reached within the wait.

### What the smoke checks

The smoke fails if Pane exits. Three checks also run against the
`screencapture` screenshots (`scripts/check_screenshot.py`):

- The root screen draws text in Pane's hint color.
- Each result screen draws text in the result color.
- The three result screens are all different. This catches a smoke that
  opens the same command twice.

Screenshots, cropped to the window (and inspected):

| Step | Evidence |
| --- | --- |
| Root search lists the three samples, installed first into a data folder of their own (#162) | [1-root.png](evidence/macos/1-root.png) |
| Rust command opened; action result "Waited 50 ms inside the Rust guest" | [2-command-0.png](evidence/macos/2-command-0.png), [2-result-0.png](evidence/macos/2-result-0.png) |
| JavaScript command; "Waited 50 ms inside the JavaScript guest" | [3-command-1.png](evidence/macos/3-command-1.png), [3-result-1.png](evidence/macos/3-result-1.png) |
| TypeScript command; "Waited 50 ms inside the TypeScript guest" | [4-command-2.png](evidence/macos/4-command-2.png), [4-result-2.png](evidence/macos/4-result-2.png) |
| Escape returns to root search | [5-back-to-root.png](evidence/macos/5-back-to-root.png) |

Rendering, keyboard focus, selection, guest execution and result display
all worked natively.

## macOS-specific fixes made for this sample

- Text rendering. GPUI CE's macOS text system is behind the `font-kit`
  feature of `gpui_platform`, which was off. The first run
  ([36363402510](https://github.com/wasimysaid/pane/actions/runs/36363402510))
  drew backgrounds but no text. The feature is now enabled in
  `crates/pane/Cargo.toml`.
- Smoke script portability. BSD `seq` prints `1 0` for `seq 0`, so an
  earlier smoke opened the TypeScript command where it meant to open Rust,
  and passed because it only checked that text was drawn. The key loop now
  counts in bash arithmetic, and the smoke asserts that the result screens
  differ.

No process or path changes were needed. On every OS the launcher reads
guests from `PANE_EXTENSIONS_DIR`, or else from `target/guests` in the
source tree it was built from. That is a development-build assumption,
and the installer tickets will replace it.

## Platform availability (#19)

The smoke also runs the platform-availability steps (screenshots 13 to 15,
[platform availability](../platform-availability.md#checks)): the Rust
command's Windows-only and macOS-and-Linux actions, then a package listing
only the two other systems.

In run [36372625940](https://github.com/wasimysaid/pane/actions/runs/36372625940)
(commit `38a95cb`, macOS 15.7.9, arm64) every step passed:

- The Windows-only action was listed with "Not available on macOS: this
  action supports only Windows" and did not run.
- The macOS-and-Linux action answered.
- The package for Windows and Linux was refused with "Not available on
  macOS: this package supports only Windows and Linux".
- The list scrolled to keep the selected row visible.

The later #19 fixes (per-command platforms, re-focusing before these
steps) have not run here yet.

| Step | Evidence |
| --- | --- |
| Windows-only action | [13-windows-only.png](evidence/macos/13-windows-only.png) |
| macOS-and-Linux action | [14-not-windows.png](evidence/macos/14-not-windows.png) |
| Package for the other two systems | [15-no-compatible-package.png](evidence/macos/15-no-compatible-package.png) |

## Root search (#23)

Root search has a query field with focus ([root search](../root-search.md)).
The smoke's search phase (screenshots 24 to 26) types "typescr" with
System Events `keystroke`, opens the only match and runs "Wait briefly",
which must look exactly like step 4. Then it types "zzz" and presses
Return on no results.

In run [36420611977](https://github.com/wasimysaid/pane/actions/runs/36420611977)
(commit `6d73d18`) every step passed. Typing "typescr" left only
TypeScript sample and Enter ran it, and "zzz" showed No results
([24-search.png](evidence/macos/24-search.png),
[26-no-results.png](evidence/macos/26-no-results.png)). Input-method
composition in the query field is still unverified here.

## Keycaps (#117)

On macOS the keycaps show the keys as Raycast and macOS's own menus do:
the modifiers' symbols in macOS's order, ⌃ Control, ⌥ Option, ⇧ Shift,
⌘ Command, and symbols for the named keys, ↵ Return, ⎋ Escape, ⇥ Tab,
⌫ Delete (the key Pane's bindings call Backspace), ⌦ Forward Delete,
⇞ Page Up, ⇟ Page Down, ↖ Home, ↘ End and the arrows. Space, Fn, the
letters, digits and function keys show their names. The user chose
this ("do like Raycast"), replacing the spelled-out caps #93 drew,
which outgrew a narrow launcher's footer. Each cap keeps its key's
spelled-out name for assistive technology: a hint reads
"Control+Option+Shift+Page Down" while it shows ⌃ ⌥ ⇧ ⇟. A hotkey
recorder writes the caps with spaces between them ("⌃ ⌥ G") where
Windows and Linux write "Ctrl + Alt + G". Windows and Linux keep their
words (Ctrl, Alt, Shift, Win, Esc). The mapping is
`crates/pane/src/keyboard.rs`'s `binding_keys_on`, unit-tested for both
spellings on every system. The bundled Geist Mono has no ⌃, ⌥, ⌘, ⎋, ⇞
or ⇟ glyph, so macOS's own font fallback draws those.

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
clipboard held "42" ([27-answer.png](evidence/macos/27-answer.png),
[30-pasted.png](evidence/macos/30-pasted.png)).

## Applications (#25)

Not run on macOS yet. This branch was not pushed, so the phase, the
native tests and the `open`/Launch Services path are unverified here,
including whether Launch Services runs an unsigned script bundle on the
runner.

[Applications](../applications.md) finds the `.app` bundles in
`/Applications`, `/System/Applications` and `~/Applications` (and their
subfolders such as `Utilities`) and opens one with `/usr/bin/open`.

The smoke's last phase (screenshots 44 and 45):

1. Makes a bundle "Pane Smoke App" in `~/Applications` of a HOME given to
   Pane only. Its program is a shell script that writes a marker file.
2. Installs the JavaScript applications sample, which supplies the host's
   applications to root search as the [Applications](../applications.md)
   default extension does.
3. Types "pane smoke", checks the selected row and presses Return.
4. Checks the marker; the launcher closed (#468), which the screenshot
   shows.

The adapter tests also open such a bundle and require Calculator among
the system's applications.

## Opening a web link (#28)

Opening a link on macOS is checked only through the tests' recording
opener.

The smoke's quicklinks phase installed the Quicklinks package and created
a quicklink in its form. It left with the extension's sources (#285), its
quicklinks being its repository's to test. The host's link opening is
covered by the Linux smoke's web-link phase, which runs the actions
sample's Open Website through the system's handler.

## Global hotkeys (#33)

Not run on macOS yet. The adapter code was only compile- and lint-checked
for `x86_64-apple-darwin` from Linux, so registration, delivery on the
main run loop and the focus transition are unverified natively.

The smoke's hotkey phase (screenshots 52 to 58,
[global hotkeys](../hotkeys.md#checks)):

1. Assigns Control+Option+G to Greeting in the hotkey recorder on the
   settings sample's page in Settings (#168; it was the launcher's hotkey
   screen before).
2. Brings Finder to the front, presses the hotkey through System Events
   and checks that Pane is the frontmost process with Greeting open.
3. Does the same again after a restart.
4. With the extension disabled, checks that pressing it leaves Finder in
   front and Pane's window unchanged.

The hotkey is a Carbon hot key (`RegisterEventHotKey`). It needs no
Accessibility or Input Monitoring permission of Pane's own. System
Events, which sends the keys, needs the Accessibility permission the
runners grant.

## Deleting retained data (#41)

Not run on macOS yet.

The smoke's retained-data phase (screenshots 63 to 65,
[deleting retained data](../extension-data.md#deleting-retained-data))
uses a data folder of its own. It saves a note with the settings sample
and uninstalls it, keeping its saved data. Then it deletes that retained
data from its row on the Extensions group's page in Settings and confirms
(#168). It checks that `installed.json` and `content.json` no longer hold
it, and reinstalls the same folder, which must show nothing kept.

## Aliases and fallbacks (#31)

Not run on macOS yet. Nothing in it is specific to macOS (no system API
is involved).

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

Not run on macOS yet.

The dependencies phase (screenshots 75 to 77,
[dependencies](../dependencies.md#checks)) uses a data folder of its own.
It previews the dependencies sample, which lists its required JavaScript
operations sample and optional Rust one. It installs it with the
JavaScript sample and runs "Greet through the required greeter", which
must answer from the JavaScript guest. `installed.json` must then hold
exactly two packages and the recorded dependency.

The logic is platform-independent except path resolution, which uses the
same `canonicalize` as package identity.

## Native helpers (#15)

Not run on macOS yet. The runner was only compile- and lint-checked for
`x86_64-apple-darwin` from Linux. So starting, ending and reaping a
helper natively, and the executable permission of the copied file, are
unverified there. Whether GPUI runs Pane's quit handler for a quit Apple
event is unverified until the smoke runs. No macOS x86-64 build was made.

The first native-helper phase (screenshots 90 to 93, data folder
`helper-data`, [native helpers](../helpers.md#checks)) installs the
helper sample. `cargo xtask guests` builds its `pane-echo` for the runner
(`macos-aarch64` on `macos-15`). The phase then:

- runs it (the answer must name macOS arm64);
- cancels a slow run after a second;
- starts the ten-second run and checks with `pgrep` that the helper runs
  from the managed copy;
- disables the package and checks that the process is gone, that the
  saved "started" note is kept, and that no helper outlives Pane.

A second phase (screenshot 94, `helper-quit-data`) starts the waiting
helper and asks Pane to quit with a quit Apple event
(`NSRunningApplication.terminate`, through Python's ctypes). It checks
that Pane exits, no helper runs and its heartbeat file stops growing.

The tests in `crates/pane-core/tests/helpers.rs` (Rust, JavaScript and
TypeScript samples) and the runner's unit tests (Mach-O headers) run in
`cargo xtask ci` there, against the `pane-echo` built natively on the
runner.

## Development mode (#12, #13)

Not run on macOS yet. The platform code (FSEvents through notify, with
the folder and event paths made canonical, and a process group killed
with `SIGKILL`) was only compile- and lint-checked for
`x86_64-apple-darwin` from Linux. So the file watcher's events, the
build's processes being killed and the whole phase are unverified
natively.

The smoke's last phase (screenshots 110 to 136,
[development mode](../development-mode.md#checks)) builds a copy of each
development sample and develops it from its page in Settings (Develop in
the Actions menu, #168). It saves an edit, a change that does not build,
two saves in a row and, after stopping, one more. It checks the answers,
the error, and that nothing is built after stopping. The TypeScript and
JavaScript samples run only where the JS toolchain is built, so CI's
smoke runs the Rust one.

macOS has no parent death signal, so a build outlives a Pane that is
killed or crashes. A Pane that quits kills it.

## Disabling required dependents (#43)

Not run on macOS yet. Nothing in it is specific to macOS: no system API
is involved, and the closure reuses the dependency identities recorded at
install.

The disable-dependents phase (screenshots 140 to 143,
[disabling a required dependency](../dependencies.md#disabling-a-required-dependency))
uses a data folder of its own. It installs the dependencies sample and
turns off the JavaScript operations sample's switch on its page in
Settings (#168). Pane must ask first (its alert's Disable All 2 shown). The
smoke cancels, then chooses Disable All (the page's status says so) and
enables the JavaScript sample again alone. `installed.json` must then
record exactly one disabled package.

## Runtime crashes (#17)

Not run on macOS yet. Nothing in it is specific to macOS: the runtime is
a thread, and helpers are ended as for a disable.

The runtime-crash phase (screenshots 200 to 209, data folder
`runtime-crash-data`,
[runtime crashes](../pausing.md#when-the-extension-runtime-itself-crashes))
installs the helper and settings samples and starts Pane with
`PANE_TEST_RUNTIME_FAULTS` naming a fault file. Then:

1. It runs the settings sample's Count, starts the waiting helper and has
   the runtime crash. The helper must be gone (`pgrep`, and its heartbeat
   must stop growing), the note it saved kept, and the status line the
   error color.
2. A second crash, injected before Count's answer, must leave the count
   at 2 and the runtime stopped. The pointer leaves the window before
   that Count, so no toast is held under it, and the return to root key
   then waits for root search before the next query is typed (#527).
3. Root search explains it, and the Extensions group's page in Settings
   shows why (its runtime rows, and the details behind one).
4. A disable works, and Restart the extension runtime runs extensions
   again. Count then counts 3.

No package may be recorded as paused.

## Extensions that stop responding (#18)

Not run on macOS yet. Epoch interruption and the watchdog are Wasmtime's
and Pane's own, with nothing specific to macOS.

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

Not run on macOS yet.

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
uninstall. A folder still in use is listed and removed at the next start,
reported against its own package. Nothing else in it is specific to
macOS.

## npm packages (#45)

Not run on macOS yet, and the automatic-update frames have not been
captured. The packing, in `cargo xtask guests`, runs in CI on macOS.

A phase of the smoke (screenshots 260 to 268, [npm packages](../npm.md))
uses a data folder of its own. It starts `scripts/npm_registry.py` on
127.0.0.1, serving the npm sample that `cargo xtask guests` packed, and
points the development build at it with `PANE_NPM_REGISTRY`. Then:

1. It installs the local Dependencies from npm sample, which downloads
   and installs the npm package it requires, and calls its `greet`
   operation.
2. It names the npm package in the npm field that "Install extension from
   npm…" opens in Settings (#168; Show Package previews it there).
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

The real registry would be reached through the HTTP client guests'
requests use, trusting the certificates rustls-native-certs reads from
the system keychains. No check exercises that: the smoke never reaches
the network, and the [by-hand](../npm.md#trying-the-real-registry-by-hand)
check has not been run. Nothing else in it is specific to macOS.

### The first CI run

The phase's first run, in CI (run 36536816781 of the fork
`wasimysaid/pane`, at ca9563b), stopped before Pane started: "the local
npm registry did not start", with nothing in `npm-registry.log` and no
port file after the smoke's 5 s wait.

The likely cause, not confirmed on macOS: Python's
`HTTPServer.server_bind` looks the address up in reverse DNS
(`socket.getfqdn`) before it listens, which is reported to take many
seconds on macOS runners. Both local servers (`npm_registry.py` and
`repository_server.py`) now skip that lookup, whose name nothing uses.
The smoke waits up to 60 s for the port file, stopping early if the
server exits.

The fix was checked by reading only, and on Linux with reverse lookups
slowed to 8 s (the old servers then failed the same way). It has not run
on macOS yet.

## Git packages (#46)

Not run on macOS yet.

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

`installed.json`, read as JSON (`scripts/check_git_record.py` on macOS),
must then record one package from Git with
`"gitRef": "refs/tags/v0.1.0"`, `pinned` and the `gitCommit` the tag
points to (`repository_server.py commit`). The downloads folder must be
empty.

Pane itself runs no `git`. The address is plain `http://` on a loopback
address, which only a development build fetches, so the smoke never
reaches the network. The HTTPS path to a real host
([by hand](../git.md#trying-a-real-host-by-hand)) is not exercised. Tree
names are refused alike on every system: the same rules as npm's, plus
`.git`, `git~1` and names differing only in case.

The phase types the address with System Events. Its repository server
skips the reverse DNS lookup the npm phase's first run stopped on
([above](#the-first-ci-run)) and is waited for as long. This was checked
by reading only.

### A tracked branch (#50)

#50 extends the phase with a second repository of the same sample
(`make-sample` again, `greeter-tracked` beside `greeter`, served by the
same server). It is installed in a data folder of its own from its
tracked `release` branch (`--install git:<address>@release`). The
preview says "Revision: branch release, tracked: an update fetches that
branch again" (305), then it is installed (306).

`repository_server.py move-sample` then commits a 0.2.0 on the branch
while Pane is stopped. The check a second after the restart replaces the
installed copy by itself, saying "Updated Greeter from Git to 0.2.0"
(307), and the new copy's command answers as before (308).
`installed.json` must record the branch's new commit, tracked and
unpinned.

Written and checked by reading only; not run on macOS yet.

## Files (#29)

Not run on macOS yet.

The files phase (screenshots 220 to 224, [files](../files.md)) uses a
data folder of its own. It installs Files (220, "Installed Files"). Its
file index (#175) covers a fixture folder "Pane smoke files" (spaces) in
the system's temporary folder, which a debug build's
`PANE_TEST_FILE_INDEX_HOME` names instead of the home folder. There is no
folder to choose any more.

The smoke types "plan" in root search, which must list "Résumé plan
ü.txt" under "Files", selected, and presses Return. Files installed from
its folder is not the registered default, so its Search Files command
keeps the generic list (#177), and the smoke does not open it.

A debug build's `PANE_TEST_OPEN_FILE_LOG` makes the opener record the
path in a file instead of running `/usr/bin/open`, so no application of
the user's opens it. The recorded path, resolved, must be the fixture
file's, resolved. Last, the smoke types "runner" and presses Return on an
executable script, which Pane must show in Finder without recording or
running it
([ADR 0037](../adr/0037-a-command-declares-its-mode-and-host-functions-decide-what-happens-after-it-runs.md)).
So the smoke runs no positive native open on macOS.

Listing the folder and the scan policy use only `std::fs`.
Case-insensitive APFS sorts names by bytes like the other systems, and
links are skipped. The policy tests with symbolic links and the
executable bit run on macOS in CI.

## Searching inside a command (#30)

Not run on macOS yet.

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
5. The service's 503, then the service stopped, are errors.
6. Restarted, a search lists results again.

## Clipboard history (#35, #37)

Not run on macOS yet.

### The pasteboard adapter

macOS gets the pasteboard adapter of clipboard history
([`macos.rs`](../../crates/pane-core/src/clipboard/macos.rs)) behind the
same `ClipboardSystem` trait as the Windows and Linux ones.

A thread of Pane's own polls the pasteboard's `changeCount` every 250 ms.
macOS' `NSPasteboardDidChangeNotification` needs a run loop that Pane's
own threads do not run, and the count is the established mechanism
without one. When the count moved, the thread reads the pasteboard's
types first. It reads the text (`NSPasteboardTypeString`) only if no type
is `org.nspasteboard.ConcealedType`, the nspasteboard.org convention that
password managers such as 1Password and Strongbox set. The convention is
honored: the text is never read when the type is present.

The pasteboard never names the program that copied. So every report's
owner is unknown, and no program can be excluded by name (an excluded
name never matches, as the contract says of an unknown owner).

`write_text` puts the text in the pasteboard server (`clearContents` +
`setString:forType:`), which holds it for whoever pastes beyond the
watch. No permission is needed. What is kept is decided by the same
`clipboard::accept`, unchanged, and the retention and storage contract is
reused as is (the fake-clipboard tests cover it).

### The smoke's clipboard phase

The smoke's clipboard phase (screenshots 280 to 287, on the pattern of
the Windows and Linux ones) uses a data folder of its own and runs after
the #52 phase. Since #166 only Pane's registered Clipboard History
records from the first start
([ADR 0042](../adr/0042-clipboard-history-records-from-the-first-start.md)).
So the phase sets the default set up from the pinned repositories that
phase serves, with the smoke's own build (Files' index on an empty
folder, `PANE_TEST_FILE_INDEX_HOME`). It checks `clipboard-history.json`
at each step:

- It keeps the smoke's own copies with nothing turned on. They are put on
  the pasteboard with AppleScript's `set the clipboard to`.
- It pauses and resumes recording in the split view's Actions panel
  (Command+K).
- It copies a kept record again (Return on it, the view's filter leaving
  it) and checks the pasteboard with `pbpaste` and by pasting into root
  search.
- It disables Clipboard History (the switch on its page in Settings),
  restarts disabled, then enables it and keeps again across a restart.

The expiry and deletion phase (400 to 405) then:

- backdates the kept records through `scripts/clipboard_history.py` and
  checks that the expired one is gone after the restart;
- deletes one record (Delete Entry);
- keeps records for 1 Hour, which deletes the older one;
- copies a file as Finder does (a file URL, kept as a file, #167);
- clears the history once confirmed (Clear History) with recording going
  on, and checks with `pbpaste` that the pasteboard still holds what was
  copied last.

No marked copy is made in the smoke, because AppleScript cannot set a
custom type. The adapter test checks the concealed marker instead.

### How it was checked

The adapter was written on a headless Linux machine, where macOS code
cannot build. It was reviewed by reading and type-checked against the
real `objc2`/`objc2-app-kit` bindings, by compiling the adapter and its
test for `aarch64-apple-darwin` in a scratch crate with the bindings from
this lock file (checking only, no linking or running).

CI's macOS leg is the compile and runtime evidence.
`clipboard_adapter_macos.rs` (opt-in through
`PANE_TEST_REAL_CLIPBOARD=1`, set for the macOS runner) runs against the
runner's pasteboard, and the next green macOS run of the smoke is the
native-capture evidence. A password manager's own copies (1Password,
Strongbox) have not been run anywhere; the test checks the convention's
type name.

## Clipboard history expiry (#36)

Not run on macOS yet.

The same smoke phase covers it (screenshots 400 to 405,
[above](#the-smokes-clipboard-phase)), since the command now runs on
macOS. Records expire after their package's retention whether or not the
extension runs. The phase backdates and deletes records as the Linux one
does, plus a copied file (#167), with the pasteboard checked directly
(`pbpaste`).

## Installing Pane and acquiring its calculator (#52)

The package, the release-profile package and this smoke phase are
pending CI. They need the macOS build and interactive desktop only the
`macos-15` runner provides.

A final phase, after the clipboard-expiry one, proves the whole outcome
of [#52](https://github.com/pane-app/pane/issues/52)
([installer](../installer.md)). It is the macOS half of what
[#53](linux.md#installing-pane-and-acquiring-its-calculator-53) proved on
Linux and [#51](windows.md#installing-pane-and-acquiring-its-calculator-51)
on Windows.

### The package

`cargo xtask package-macos --dev` builds the macOS package. It is a zip,
like the Windows one, opened with one double-click in Finder and written
by the same fixed-bytes writer. It holds:

- the `pane` program;
- the bash `install.sh`;
- a README;
- the bundle's `Info.plist` (the development profile, so its program
  takes its pins from `PANE_DEFAULTS`);
- the release assets (the package under the name a release gives it, and its `.sha256`).

No default-extension payload is written. First setup fetches the five
defaults from the commits this release pins.

### What the phase does

The smoke clones the defaults' repositories at those commits from their
real addresses on GitHub (its own setup, on the runner). It serves the
clones on 127.0.0.1 over Git's smart HTTP protocol
(`scripts/repository_server.py`), named by the pins file the development
build reads through `PANE_DEFAULTS`. Nothing the Pane under test does
reaches the network or Pane's published releases.

1. The package is unzipped into a folder of its own. Its install script
   runs with a clean machine's environment: a fresh home folder and
   `PATH=/usr/bin:/bin`, so the home holds no data and no development tool
   is configured.
2. The script builds the `Pane.app` bundle in that home's
   `~/Applications` (the user's own folder, so no administrator rights)
   from the program and the `Info.plist`. It runs `pane --version` to
   check what it installed.
3. The installed Pane starts (the bundle's `Contents/MacOS/pane`) with a
   PATH that holds nothing at all: an empty folder, checked with
   `command -v` of cargo, rustc, node, npm, git, cc, clang and make. It
   takes its pins from the override, and its releases from the
   local stand-in the smoke serves them on.
4. Pane fetches the five default extensions by itself, with its own Git
   client. `installed.json` must record each under `"default"`, with the
   repository, release tag, commit and pinned state of its pin (checked by
   `scripts/check_git_record.py`), and no sample. The helper sample left
   the default set with #162, and a helper running from an acquired
   revision is `crates/pane-core/tests/installer.rs`'s to test.
5. Root search lists their commands, "6*7" answers 42 and Enter copies
   it, with no developer tool reachable.
6. The downloads folder the fetches used must end empty.

The program files are removed again at the phase's end. So the uploaded
evidence is the screenshots and records (frames 500 to 502, and the
`clean-home-records` folder), not the program. CI builds the
release-profile package after the smoke and uploads it with the
artifacts of the job (`macos-package`).

Nothing is signed, since no Apple Developer credentials exist. The
smoke's binaries are built on the runner, so they carry no Gatekeeper
quarantine mark. A package downloaded from the internet would, and its
first launch would be blocked until allowed in System Settings. That is
recorded as an execution prerequisite in
[the installer's limits](../installer.md#limits-and-prerequisites).

### What ran locally

Recorded 2026-09-30 on this branch's machine, a headless aarch64 Linux
with no Apple toolchain, so the macOS `pane` program and its package
cannot be built there. What ran is everything that machine can run:

- `cargo check`, `clippy`, `fmt` and the unit tests of `xtask`.
- `cargo xtask package-macos`, far enough to assemble the whole artifact
  tree, before it refuses with the message that only a macOS checkout
  builds the program (the same refusal `package-windows` makes there). The
  helper sample's payload manifest was rewritten to that machine's target,
  `linux-aarch64`, by the same shared code a macOS run rewrites to
  `macos-aarch64`. The default-extension payloads the task then wrote are
  gone since #278.
- `cargo xtask package-linux` end to end, as the shared packaging's
  regression.
- The install script is bash, so it was linted (`bash -n`) and dry-run
  with a fake `pane` binary through its logic paths: the per-user
  `Pane.app` install with a clean environment, the `--app-dir` override,
  the missing-file and usage errors, and the `--version` check propagating
  the program's exit code. Its one macOS-specific claim, that Gatekeeper
  does not stop the script's direct run, is read-reviewed.
- The acquisition the installed Pane does is the same
  platform-independent code `crates/pane-core/tests/installer.rs` checks.
  Those tests were rerun there, all passing, including the prebuilt helper
  running from the managed copy.
- The `Info.plist` the package ships was generated and parsed back
  (`plistlib`) with the keys Launch Services reads.
- The bash smoke phase is syntax-checked and reviewed only.

| Step | Evidence |
| --- | --- |
| The clean machine's Pane acquired the five default extensions, and no sample (#162), and lists their commands | pending CI (frame 500) |
| "6*7" answers 42 | pending CI (frame 501) |
| Enter copies the answer | pending CI (frame 502) |

## Installing a Pane application update by the user's choice (#55)

The 99.0.0 package, the bundle-binary swap and this smoke phase are
pending CI. They need the macOS build and interactive desktop only the
`macos-15` runner provides.

A final phase, after the installer one, proves the whole outcome of
[#55](https://github.com/pane-app/pane/issues/55)
([installer](../installer.md)). It is the macOS half of what
[#54](windows.md#installing-a-pane-application-update-by-the-users-choice-54)
proved on Windows.

### Setting up the two versions

The same `cargo xtask package-macos --dev` builds a second package with
`--package-version 99.0.0`: a program that reports 99.0.0, a package
named by it for `macos-aarch64`, and the release assets naming that
package as a release does, with its `.sha256`. Those are the two runnable
builds an update goes between.

The 0.1.0 package the installer phase built is installed on another clean
home, with its install script, its empty PATH and its own data under
`~/Library/Application Support/Pane`. The smoke serves the 99.0.0
release from 127.0.0.1 with `scripts/release_server.py`. Nothing
reaches the network or Pane's published releases.

### What the phase checks

Since [#502](https://github.com/pane-app/pane/issues/502)
([ADR 0050](../adr/0050-pane-downloads-its-updates-by-itself-and-restarts-into-them.md))
Pane downloads an update by itself, and the phase checks that instead of
"nothing is downloaded until the row is chosen":

1. The package served first is damaged: its bytes do not match the
   sha256 its `.sha256` gives. The installed 0.1.0 Pane is started with
   `PANE_RELEASES` and the same pinned repositories. It fetches its
   default extensions and, in the same background, lists its releases and
   downloads 99.0.0 by itself: the release server's log holds the
   package's request before anything is chosen.
2. That download fails quietly (frame 600, evidence only): nothing is
   staged, and the program and the bundle's `Info.plist` are the ones
   installed.
3. Clipboard History is disabled first (the Helper sample was, until #162
   took it out of the default set), so an extension the user disabled
   before the update must stay disabled after it.
4. With the package whole again, Check for Pane Updates downloads it and
   stages it, and its toast says "Pane 99.0.0 is ready" (frame 601); the
   marker is written in `Contents/MacOS/update/`, and the package
   downloaded twice in all. A blank root search then lists Restart to
   Update Pane first, selected (frame 602).
5. Enter restarts into the update without downloading again
   ([#504](https://github.com/pane-app/pane/issues/504)). The old
   `Contents/MacOS/pane` is renamed `pane.old` beside it and the new one
   takes its place; the new one is started directly (not through Launch
   Services) with `--updated-from` and `--wait-pid`, and the old process
   exits. A new `pane` runs from the same binary (frame 603, evidence),
   reports `Pane 99.0.0`, and the staging folder is gone (byte
   comparisons of both binaries against the two packages' own files).
6. The calculator still answers "6*7" with 42 from the old version's
   install (the calculator set up at first setup, from the pinned
   repositories; frames 604 and 605), and the pasteboard holds 42. The
   disabled Clipboard History stays disabled, since Pane's data was never
   touched.
7. The new version settled, so the next start removes `pane.old`.

The bundle itself is never replaced: the same `Pane.app` keeps its
identity. Since [#455](https://github.com/pane-app/pane/issues/455) the
swap also writes the new version into the bundle's `Info.plist`, and the
smoke checks that the plist is then the new package's own and that
`plutil` reads `99.0.0` from it (see the
[installer](../installer.md#updating-pane-itself)).

Pane itself was never restarted by the update. The smoke stops the old
process and starts the new program itself, exactly as the user would.

### What ran locally

Recorded 2026-09-30 on this branch's machine, a headless aarch64 Linux
with no Apple toolchain, so the macOS `pane` program and its package
cannot be built there. What ran is everything that machine can run:

- The platform-independent half
  (`crates/pane-core/tests/application_update.rs`), all passing,
  including the swap, the failures and the data kept.
- `cargo check` and the unit tests of `pane-core` and `xtask`.
- `cargo xtask package-macos --dev --package-version 99.0.0`, far enough
  to assemble the whole artifact tree before it refuses with the message
  that only a macOS checkout builds the program (the same refusal the
  installer phase records).
- The macOS wiring itself is one `cfg` widened in `pane`'s `main.rs`
  around the identical call the Windows build makes. It was
  compile-checked by reading: the Linux checkout compiles the file without
  the macOS half, exactly as without the Windows one.
- The bash smoke phase is syntax-checked (`bash -n`) and reviewed only.

The swap depends on renaming the running binary. Renaming a running
program's file is allowed and overwriting one is not, and the running
process keeps its open file. That POSIX behavior was read-reviewed and
can only be proven end to end on the runner.

| Step | Evidence |
| --- | --- |
| The check at start tells the user; nothing is downloaded until they choose | pending CI (frames 600, 601) |
| A damaged package is explained, everything untouched, the row retried | pending CI (frame 602) |
| The user's choice swaps the running binary in the bundle; the new one is used the next start | pending CI (frame 603) |
| The new version reports itself; the old version's data and enablement are kept | pending CI (frames 604, 605) |

## Alerts: system notifications and the ringing sound (#559)

An extension's `notify` and `ring` (`wit/alerts.wit`,
[ADR 0053](../adr/0053-extensions-get-the-users-attention-through-a-notification-a-ringing-sound-and-the-launcher.md))
on macOS:

- **The notification** goes to UserNotifications' notification center
  (`UNUserNotificationCenter`): its title, the message as its body, the
  extension's title as its subtitle, and no sound. macOS gives a
  notification center only to an app with a bundle identifier, and asking
  for it from a program outside a bundle ends the process with an
  Objective-C exception. So Pane checks its bundle first: the bare `pane`
  program (a development build, the smoke's Pane) answers `not-available`,
  "macOS posts notifications only for an app, and this Pane runs outside
  Pane.app". `Pane.app` (`dev.pane.launcher`, which `install.sh` builds)
  asks the user once, at the first post, waiting 3 seconds for the answer;
  while the question is open, after a refusal, and when notifications are
  turned off for Pane, the post answers `not-available` saying so. The
  click comes to the center's delegate, a class of Pane's own, which also
  has macOS show a notification while Pane is the frontmost app. The last
  32 notifications' clicks are kept, as on Windows.
- **What an unsigned Pane gets.** On CI's `macos-15` runner, a copy of the
  test program in an app bundle of its own (signed only by the linker, ad
  hoc, and not notarized, as `Pane.app` is) asked for permission and was
  refused at once with `UNErrorDomain` error 1, "notifications not
  allowed", without any question shown. Pane answers that as
  `not-available`: "macOS does not allow notifications from this app
  (UNErrorDomain 1, notifications not allowed); Pane is not signed, which
  macOS may require". So an unsigned `Pane.app` most likely cannot post
  notifications; whether a real `Pane.app` on a person's Mac is asked
  instead is the first hand check on
  [#559](https://github.com/pane-app/pane/issues/559). Signing Pane needs
  Apple Developer credentials, which do not exist
  ([installing Pane](#the-package)).
- **The ringing sound** plays through an AudioToolbox audio queue on the
  default output device: Core Audio mixes it with other applications'
  sound at the output's own volume and converts its rate. The frameworks
  are declared in Pane's own code, with no audio crate. No default output
  device answers `not-available`. A `file` sound must be a WAV file (Pane
  decodes MP3 on Windows only). The `system` sound is the alert sound the
  user chose in Sound settings (`com.apple.sound.beep.sound`), an AIFF
  file Pane reads itself, else Tink.
- **An attention launch** waits for the user to stop typing, from Core
  Graphics' `CGEventSourceSecondsSinceLastEventType` for any input of the
  login session, which needs no permission
  (`crates/pane/src/input_idle.rs`).

What CI checks, on its `macos-15` runner, with `PANE_TEST_REAL_ALERTS`
set by `.github/actions/setup`:

- `system_notifications::macos::tests`: the bare test program answers
  `not-available` without touching the center, and the bundled copy
  above records what the center answers. `ringing::macos::tests`: the
  audio queue opens on the runner's default output device, takes half a
  second of silence, hands its buffers back as they play, and stops at
  once when dropped; the alert sound is a file.
  `ringing::tests::the_system_output_plays_a_ringing_and_dropping_it_stops_it`
  rings the alarm through the real audio queue and checks that dropping
  the handle closes it and nothing more is written. The integration test
  `ringing::the_system_sound_plays` plays the alert sound (Tink on the
  runner) through the stand-in sink. The stand-in sink's tests check every
  way the extension's code ends stops the sound, on every system.
- The smoke's alerts phase (`scripts/smoke-macos.sh`, frames 210 to 212)
  installs the alerts fixture as a package whose activation's timer posts
  each second: the bare program's posts answer `not-available` as above.
  It rings the alarm, which plays or answers that there is no output
  device, and drops it; what each call answered is kept in
  `alerts-answers.txt` with the evidence.

Not checked by CI, and so by hand on a Mac (the steps are on
[#559](https://github.com/pane-app/pane/issues/559)): what `Pane.app`
itself is answered, that a person sees the notification and hears the
sound, and that an attention launch waits while they type.

## Text input and accessibility findings

### Text input and IME (#20)

Extension forms now have a text field. The smoke also opens the Rust
command's form and submits it empty, and the error color must appear.
Then it types "Ada" through System Events `keystroke`, then Tab, Down and
Return. The result color must appear, which only happens if the typed
text reached the name field.

In run [36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770)
(macOS 15.7.9, arm64) every step passed, and the result read "Good
morning, Ada, from the Rust guest".

Composition with a macOS input method (for example Japanese Kana) through
`NSTextInputClient` is unverified. The window tests cover composition
only on the field's editing state
([what that proves](../forms.md#checks)). Editing bindings follow the
element's macOS defaults (Cmd-A/C/V/X/Z, Option-arrow words); only
typing, Tab and the arrow keys ran natively.

Screenshots from run
[36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770)
(commit `949e35d`), cropped to the window:

| Step | Evidence |
| --- | --- |
| Form opened; focus in the name field | [6-form.png](evidence/macos/6-form.png) |
| Submitted empty; "Enter a name" on the field and status | [7-form-error.png](evidence/macos/7-form-error.png) |
| Typed "Ada", Tab, Down to "Good morning", submitted | [8-form-result.png](evidence/macos/8-form-result.png) |

### Accessibility

The window exposes a `ListBox` labelled with the view title,
`ListBoxOption` rows with label, description and selected state, the
selected row as the active descendant, and a `Status` node for the
result. This is verified through GPUI's accessibility tree in the window
tests (`assistive_technology_sees_the_list_the_selection_and_the_result`),
which pass on macOS but are platform-independent.

VoiceOver was not run, so how the tree reaches NSAccessibility and what
VoiceOver announces are unverified. For forms, see
[accessibility of forms](../forms.md#accessibility).

### Custom view (#21)

After a final restart, the smoke opens the Rust command's color picker
and presses Right (key code 124). Then it clicks the dark green swatch
with a Quartz mouse event posted through Python `ctypes`, converting the
screenshot's pixels to points (half on Retina). Each step must show the
chosen color over at least 3000 pixels.

In run [36378453278](https://github.com/wasimysaid/pane/actions/runs/36378453278)
(commit `1487dc8`, macOS 15.7.9, arm64) every step passed. The picker
opened on blue (#1E88E5), Right moved to purple (#8E24AA) and the click
chose dark green (#1B5E20). Posting the Quartz event needed no
permission beyond the one System Events has. For accessibility, see
[the canvas](../designed-tree.md#the-canvas).

Screenshots from the same run:
[21-color.png](evidence/macos/21-color.png),
[22-color-key.png](evidence/macos/22-color-key.png),
[23-color-click.png](evidence/macos/23-color-click.png).

### Operations (#22)

The operations phase (screenshots 31 and 32) installs the JavaScript
operations sample, then the Rust one. It opens the Rust sample's command
and fills its form with the JavaScript package's identity and the name
"Rust". The Rust guest then calls that package's `greet` operation
through Pane, the same steps as on [Linux](linux.md#operations-22).

In run [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
(commit `1848494`) every step passed: the Rust guest's call into the
JavaScript package answered "Hello, Rust, from JavaScript"
([32-operation-answer.png](evidence/macos/32-operation-answer.png)).

### Reload (#11)

After the operations phase, the smoke installs a package from
`<output-dir>/dev`, replaces its component with the JavaScript sample and
reloads it in Manage extensions. It then reloads it without its component
(the checks fail and the old code keeps answering) and with the
`failing-start` fixture (a startup failure, then Retry). These are
screenshots 33 to 39, the same steps as on
[Linux](linux.md#reloading-a-package-11).

In run [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
(commit `1848494`) every step passed:

- Reloaded with the JavaScript build, Dev answered from the new code
  ([35-dev-after.png](evidence/macos/35-dev-after.png)).
- Reloading the `failing-start` fixture showed "Reloaded Dev, but it
  failed to start… Retry…"
  ([38-start-failed.png](evidence/macos/38-start-failed.png)).
- Retry then showed "Started Dev"
  ([39-retried.png](evidence/macos/39-retried.png)).

### Clearing an extension's cache (#39)

After the reload phase, the smoke restarts Pane and saves one value of
each kind of [extension data](../extension-data.md) for the Settings
sample (style, content, cache and credential). Then it chooses "Clear
cache of Settings sample" in Manage extensions and confirms, the same
steps as on [Linux](linux.md#clearing-an-extensions-cache-39).

In run [36429153309](https://github.com/wasimysaid/pane/actions/runs/36429153309)
(commit `1848494`) every step passed. Manage extensions showed "Cleared
the cache of Settings sample"
([42-cache-cleared.png](evidence/macos/42-cache-cleared.png)). "Show what
Pane keeps" then read "Style: formal · Note: Water the plants · Signed
in: yes · Cached greeting: none": the cached greeting was gone and the
other three values kept
([43-kept-after-clear.png](evidence/macos/43-kept-after-clear.png)).

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
| Formal greeting saved | [16-setting-saved.png](evidence/macos/16-setting-saved.png) |
| Disabled in Manage extensions | [17-disabled.png](evidence/macos/17-disabled.png) |
| After a restart: Greeting absent from root | [18-restarted-disabled.png](evidence/macos/18-restarted-disabled.png) |
| Enabled again | [19-enabled.png](evidence/macos/19-enabled.png) |
| The kept setting answers | [20-greeted.png](evidence/macos/20-greeted.png) |

## Local extension package (#9)

`scripts/smoke-macos.sh` also installs
`target/guests/packages/sample-rust` with `pane --install <folder>` (with
`PANE_DATA_DIR` pointing at a fresh folder), runs its command, and
restarts Pane. In run
[36371205770](https://github.com/wasimysaid/pane/actions/runs/36371205770)
(commit `949e35d`) every step passed.

The `packages` identity tests also passed there: folder paths with spaces
and Unicode, letter case and Unicode normalization as this file system
treats them, and symbolic links.

| Step | Evidence |
| --- | --- |
| Package screen: source, version, commands, compatibility | [9-package.png](evidence/macos/9-package.png) |
| Installed; the new command is selected in root search | [10-installed.png](evidence/macos/10-installed.png) |
| The installed command answers ("Hello from the Rust guest") | [11-installed-result.png](evidence/macos/11-installed-result.png) |
| Still listed after a restart | [12-restarted.png](evidence/macos/12-restarted.png) |

Since #162 Pane registers no sample command. The first phase installs the
three samples into a data folder of their own, and this phase's data
folder lists the installed Rust sample alone, so the screenshots show
that copy. In these screenshots the root list is taller than the window
and its last row is cut off; since #19 the list scrolls to keep the
selected row visible.

## Remaining limits

- Only a CI runner was used; no one has run it on a contributor's own
  Mac.
- The #4 resource and latency workload exists for Linux only. This
  platform's measurements and targets are not started
  ([the measurement record](../research/resource-measurements.md)).
- Intel Macs, other macOS versions and the JS/TS toolchain build on macOS
  are untested. The wasi-sdk checksum for macOS in
  `tools/componentize-js/pins.json` has not been checked against a real
  download.
- The smoke confirms that text appears in the expected colors and that
  the three results differ. Which command and guest each screenshot shows
  was checked by inspection.
