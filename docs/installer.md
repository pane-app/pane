# Pane's packages and first setup

Added for [#53](https://github.com/pane-app/pane/issues/53) (Linux; US15,
US16, US18, US42; T23; contributions to G6, not a claim that it passes),
[#51](https://github.com/pane-app/pane/issues/51) (Windows) and
[#52](https://github.com/pane-app/pane/issues/52) (macOS): a clean
machine installs Pane from one package, and Pane sets its default
extensions up itself over the network — [#60](https://github.com/pane-app/pane/issues/60)'s
five, fetched at first setup from the commits of their own repositories'
release tags that the Pane release pins
([#278](https://github.com/pane-app/pane/issues/278),
[ADR 0045](adr/0045-official-extensions-live-in-their-own-repositories.md)) —
with retries and the rows that try a failed one again, while the core (the
window, root search, the install rows, Settings › Extensions) stays usable. This is
the internet-first setup the specification chose
([decision 20](launcher-design-interview.md)); the installer carries no
payloads and installs no runtime, and the user installs no Node, Rust,
npm, Git or compiler: Pane's extension runtime is part of Pane's own
process (Wasmtime), so nothing is acquired for it.

[#54](https://github.com/pane-app/pane/issues/54) adds what the artifact
source serves: Pane's own updates. Pane checks the artifact source for a
newer version of itself when it starts and tells the user, who alone
chooses whether to download and install it — Pane never downloads,
installs or restarts itself unprompted
([decision 17](https://github.com/pane-app/pane/issues/1), [Q38](current-decisions.md)).
[#55](https://github.com/pane-app/pane/issues/55) wires the macOS half:
the Windows and macOS installs are below; the check, the download, the
verification and the swap are platform-independent and live in
`pane-core`, ready for another system's updater to wire to its own
program.

The acquisition itself is the same on every system (it is
platform-independent code, tested by `crates/pane-core/tests/installer.rs`);
what each system has of its own is the package, the install script and
the smoke that proves the whole outcome there.

## The package

Each task builds, under `target/dist/`, one package for the system it
runs on and the artifacts an artifact source serves (below):

- **`pane-<version>-linux-<arch>.tar.gz`** — the Linux package
  (`cargo xtask package-linux`): the `pane` program (release profile), the
  install script, a `README.txt` and a `pane.desktop` entry, under `pane/`.
  With `--dev`, the program is the development profile and the name ends
  `-dev`; the native smokes install that one, because only a development
  build takes its default extensions' pins from `PANE_DEFAULTS` and its
  artifact source from `PANE_ARTIFACTS` (a release build uses the
  committed pins and Pane's published downloads, which no controlled source
  may replace).
- **`pane-<version>-windows-<arch>.zip`** — the Windows package
  (`cargo xtask package-windows`): the `pane.exe` program (release
  profile), the PowerShell install script and a `README.txt`, under
  `pane/`. With `--package-version <version>` the program is built
  reporting that version and the package and its index entry are named by
  it — a build-tool option for the smokes, which need a newer version
  than the one installed to offer; a release build simply builds the
  workspace's own version (nothing gates the option: it names the
  package, and anyone building one can name it).
  A zip, because a Windows user unzips with whatever is at hand
  and Windows has no tar of its own a user can rely on; `--dev` names the
  development profile's package the same way. The task assembles the
  artifacts anywhere, but builds the package only on Windows: `pane.exe`
  needs a Windows checkout (no cross toolchain is set up), so anywhere
  else it explains so and stops — the one thing a Windows build alone
  provides.
- **`artifacts/`** — what an artifact source serves (below): the index
  `pane-defaults.json`, naming the application package a Pane application
  update downloads, and the package itself. Since #278 no
  default-extension payload is written: the default extensions are
  fetched from their own repositories, and this folder serves Pane's own
  updates alone. A real deployment serves this folder at Pane's published downloads; the
  tests and smokes serve it from this computer instead. The package is
  built for the system the task ran on, its index entry naming that
  target, so each system's run of its own task serves its own.
- **`pane-<version>-<os>-<arch>.<zip|tar.gz>.sha256`** — each package's
  digest.

Each package is the same bytes wherever it is built (fixed time, owner
and mode in the Linux tar; the fixed time, entry order and deflate of the
Windows and macOS zips), and its program is built for the system the task
ran on: this machine builds `linux-aarch64`, CI's `ubuntu-24.04` runner
builds `linux-x86_64`, its `windows-2025` runner `windows-x86_64` and its
`macos-15` runner `macos-aarch64`. A release for several systems builds
one package per system. **Nothing is signed** — no
signing credentials exist, on Windows no Authenticode certificate — and
the `.sha256` file says only what was packed; signing the package, and
deploying the artifact source, are execution prerequisites recorded
[below](#limits-and-prerequisites).

The package holds no default extension: first setup fetches them from
their repositories' pinned commits, so
the installer stays small and every default extension (also the ones
later slices add) is a normal, individually disableable extension rather
than something baked into the program.

### Installing on Linux

```sh
tar -xzf pane-<version>-linux-<arch>.tar.gz
bash pane/install.sh                # installs to ~/.local
bash pane/install.sh --prefix DIR   # or somewhere else
pane --version                      # what the install script runs to check
```

The script installs for one user: `pane` to `<prefix>/bin` and the
desktop entry to `<prefix>/share/applications`. No root is needed. It
checks the program's libraries with `ldd` and, if one is missing, names
it and the Ubuntu package that provides it, rather than installing
anything. The declared baseline is Ubuntu 24.04 (x86_64, X11), the
combination [CI runs](platforms/linux.md#ci-result); the libraries
`libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libxcb1
libfontconfig1 libfreetype6 libvulkan1` and a Vulkan driver
(`mesa-vulkan-drivers` works without a GPU) are the prerequisites. Uninstalling
is removing the two files; Pane's own data stays in `~/.local/share/pane`.

### Installing on Windows

```powershell
Expand-Archive pane-<version>-windows-<arch>.zip        # or unzip in Explorer
cd pane
powershell -ExecutionPolicy Bypass -File install.ps1    # installs to %LOCALAPPDATA%\Pane
pane --version                                          # what the install script runs to check
```

The script (`scripts/install-windows.ps1`, mirrored by the Linux one)
installs for one user: `pane.exe` to `%LOCALAPPDATA%\Pane` — an
environment variable of the user's own, so no administrator rights are
needed — and a `Pane` shortcut to the user's Start menu
(`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Pane.lnk`), which is
the Start-menu entry the [applications](applications.md) extension finds.
No desktop entry is made, because an installer that asks no questions
puts nothing on the desktop. `-InstallDir <folder>` installs somewhere
else. Pane keeps its own data in `%LOCALAPPDATA%\Pane\data` (and its
caches in `%LOCALAPPDATA%\Pane\cache`), inside the same per-user folder
the program is installed to; uninstalling is removing `pane.exe` and the
shortcut, and that data folder if the user wants it gone too. The
declared baseline is Windows Server 2025 (x86_64), the system
[CI runs](platforms/windows.md#tested-combination); the program needs
nothing beyond Windows itself (no Node, Rust, npm, Git, compiler, and no
administrator rights), and the install script checks what it installed by
running `pane --version` (its exit code: Pane is a window-subsystem
program, so the version text is read from a redirected stream).

Because nothing is signed, PowerShell may refuse `install.ps1` as a
script that came over the internet (a zip downloaded from the web and
extracted can carry that mark): the `-ExecutionPolicy Bypass` above
answers that for the one script, or `Unblock-File install.ps1` once, and
the package's `.sha256` file says what was packed. Windows itself may
warn about an unknown publisher when `pane.exe` runs; that is what absent
Authenticode credentials mean, recorded plainly
[below](#limits-and-prerequisites).

### Installing on macOS

```sh
unzip pane-<version>-macos-<arch>.zip        # or double-click it in Finder
cd pane
bash install.sh                # installs Pane.app to ~/Applications
bash install.sh --app-dir DIR  # or into another folder
```

The script (`scripts/install-macos.sh`) installs for one user: it builds
the `Pane.app` bundle in `$HOME/Applications` — a folder of the user's
own, so no administrator rights are needed — from the package's `pane`
program and `Info.plist` (`Contents/MacOS/pane` and
`Contents/Info.plist`), and checks what it installed by running that
program's `--version`. Nothing is put on the PATH: the bundle opens with
a double-click in Finder or `open ~/Applications/Pane.app`, and the
program for a terminal is `~/Applications/Pane.app/Contents/MacOS/pane`
(the one `--version` answers, as the install script runs it).
`~/Applications` is one of the folders the
[applications](applications.md) extension searches, so installed Pane
finds itself; no Dock or desktop entry is made beyond the bundle itself.
Uninstalling is removing the bundle; Pane keeps its own data in
`~/Library/Application Support/Pane` and its caches in
`~/Library/Caches/Pane`. The declared baseline is macOS 15 on Apple
silicon (arm64), the combination [CI runs](platforms/macos.md#tested-combination);
the program needs nothing beyond macOS itself (no Node, Rust, npm, Git,
compiler, or administrator rights).

Because nothing is signed (no Apple Developer ID certificate exists,
nothing notarized), Gatekeeper matters only where a package came over the
internet: a browser or mail program marks what it downloads, and macOS
blocks the first launch of an app it cannot check until the user allows
it in System Settings (Privacy & Security). A zip built on the machine
itself — a CI runner's, as the smoke's — carries no quarantine mark and
runs at once. The zip stores no execute permission (the same plain zip
as the Windows one), so the install script's copy is what sets the
program's; check the package's `.sha256` file when it reached you over
the internet. Signing and notarizing the program are execution
prerequisites recorded [below](#limits-and-prerequisites).

## Acquiring the default extensions

A default extension ([glossary](../CONTEXT.md)) is identified by its id —
`calculator` — which is also its package identity
(`default:calculator`, recorded as `"default": "calculator"` in
`installed.json`, with the Git source of the revision it was fetched
from), whatever version is installed. The release's default extensions
are the calculator, applications, quicklinks, files and clipboard history
([#60](https://github.com/pane-app/pane/issues/60), the user's recorded
choice): all five set up at first setup and each individually
disableable, with clipboard history recording from the first start
([ADR 0042](adr/0042-clipboard-history-records-from-the-first-start.md)).
Every build sets up the same five on every system — the Windows power
features' Run, System Commands and Switch Windows
([ADR 0040](adr/0040-default-extensions-grow-to-include-windows-tools.md))
are pinned with `"platform": "windows"` and set up on Windows alone:
the pins file's platform field is read before any repository is fetched
(the launcher's platform gate), so a Linux or macOS first setup never
fetches them — and no sample is a default extension
([#162](https://github.com/pane-app/pane/issues/162)). Until #162 a
development build also acquired the prebuilt-helper sample; an install
that acquired it keeps it as an ordinary installed package (Pane removes
nothing it acquired), which the user can uninstall, and no later first
setup acquires it again. The samples stay installable by hand
(`pane --install target/guests/packages/<name>`).

Since [#278](https://github.com/pane-app/pane/issues/278) each default is
fetched from its own repository, at the commit of the release tag this
Pane release pins
([ADR 0045](adr/0045-official-extensions-live-in-their-own-repositories.md)):
the eight live in public repositories of their own in the
[`pane-app`](https://github.com/pane-app) organization
([#279](https://github.com/pane-app/pane/issues/279),
[#301](https://github.com/pane-app/pane/issues/301)), and the pins are
committed to the build
([`crates/pane/defaults.json`](../crates/pane/defaults.json)), each naming
the default's id, title, repository, release tag, that tag's commit and,
for the Windows-only three, the platform they are set up on; a newer Pane
release moves the pins forward, as for the five. (Before #278 the
defaults were
acquired from Pane's artifact source as tarballs an index named; that
machinery is gone, and the artifact source remains for Pane's own
application updates alone.) A development build can replace the pins
with a file of its own through `PANE_DEFAULTS` (its repositories must be
reachable as a Git address is: HTTPS, or a loopback address in these
builds alone), so the tests and smokes serve the repositories on this
computer and no check ever reaches a real Git host; a release build has
no override.

At first setup, and whenever a default extension is missing, Pane
acquires each in turn in the background:

1. **The pin.** The fetch names the repository, at the pinned commit: a
   commit id pins the bytes
   ([ADR 0021](adr/0021-pane-fetches-git-packages-itself.md)), so the tag
   is recorded but never asked for — a tag the repository moved does not
   move what this release installs.
2. **The fetch.** Pane's own Git client (ADR 0021: Git's smart HTTP
   protocol version 2, over the same HTTPS stack npm and Git packages
   use — no `git` program, library or configuration) fetches exactly the
   pinned commit, checks every object against its id and writes out only
   the revision's files and folders, into a download folder of its own
   under `downloads/`, removed once the package read from it is dropped.
   An interrupted fetch — the connection failing partway, or a server
   error (403, 500, 502, 503, 504) — is tried again, up to three times,
   after 0.5 s and 1 s; a revision the repository refuses, or one whose
   files fail their checks, is explained, not retried.
3. **The install.** The fetched revision is read, checked, planned and
   installed exactly as a package from a folder is — into a managed
   copy, with the default extension's identity, and its Git source
   (repository, tag, commit, pinned) recorded for
   [#269](https://github.com/pane-app/pane/issues/269)'s updates. Its
   compatibility is what any package's is: the
   manifest version and extension API this Pane reads, the platforms it
   declares, the components it names present, and, where it ships
   helpers, the file for this system a real program for it. A default
   extension the user uninstalled, whose data Pane keeps, is not acquired
   again — the user's choice, with disabling the documented opt-out; one
   never installed is.

### What the user sees

The status line says what is happening — "Acquiring the Calculator…" —
while the window, root search, the install rows and Settings › Extensions
stay usable: acquisition never blocks anything. (A Git fetch answers with
one pack, so there is no byte progress to follow and no percentage.)
When every default extension is set up, the status
line says "Set up the Calculator" (or "Set up Pane's default extensions").
A default extension that could not be acquired is explained there ("Could
not set up the Calculator: Could not reach the Git repository …
(Pane tried 3 times)") and offered as a row in root search after the
install rows, **Set up Calculator**, which tries again; the row goes once
what it asked for is there. Starting Pane tries again by itself, so a
Pane stopped mid-setup recovers, and disabling a default extension (in
Settings › Extensions) is the opt-out: a disabled default extension is
installed, so it is never re-acquired or re-enabled.

## Updating the default extensions

Between Pane releases a default extension updates from its repository's
newer release tags ([#269](https://github.com/pane-app/pane/issues/269),
[ADR 0045](adr/0045-official-extensions-live-in-their-own-repositories.md)):
once Pane is up and running (a minute after it starts, then daily — see
[npm's updates](npm.md#updating-by-itself)), it reads the repository its
record names and lists that repository's `v<semver>` tags, comparing each
tag's version with the version the record's `defaultVersion` says is
installed. The newest release above that version is fetched — at the
commit its tag points to, which pins the bytes — and staged, checked and
applied exactly as the update of an npm or Git package is, keeping the
default identity, the saved data, the disabled state, the hotkeys and the
aliases, waiting for the same safe activation boundary. Nothing is
fetched while the newest tag names the version installed, and a tag older
than it is never followed down. An update never re-enables a disabled
default, and one the user uninstalled is never re-acquired (acquisition's
rule); one whose record keeps no repository — an older Pane acquired it
from Pane's own downloads — is skipped with that said and never updated.

A default extension is eligible on the same terms as an unpinned npm
package: enabled, not paused, not turned off — its `pinned` on record is
what first setup installed (the release tag this Pane release pinned),
never a choice of the user's. The same controls govern it: the global
"Update extensions automatically" choice, and the per-extension switch on
its page in Settings and in the extension list ("a newer release of its
repository replaces it once no command of it runs"). A payload whose
`apiVersion` this Pane cannot run is skipped with "needs a newer Pane"
(ADR 0046), the row saying so too when Pane's own application update
exists; a release that passes its checks but fails to start pauses the
extension with Retry and is recorded under Failed, not rolled back; an
update that installs required dependencies installs them all or changes
nothing. Every default-extension check's outcome lands in the pass's
[update results](npm.md#updating-by-itself): an Updated row saying the old
and the new version, a Skipped row with why, a Failed row with what
failed.

## Updating Pane itself

An application update ([glossary](../CONTEXT.md)) is the artifact
source's own half: the index holds an `application` entry — the Pane package for
one target, named by its version, file, sha512 integrity, size and
`target` (`windows-x86_64`, `macos-aarch64`, written as a [helper
target](../CONTEXT.md) is) — and the source serves the package the
entry names. `cargo xtask package-windows`, `-macos` (and `-linux`) put
the package they built into the artifacts folder beside its index entry,
so one deployment serves everything from one place; the index names
nothing else (no default-extension payload since #278), and the entry is
parsed only where an update is checked, so an application entry one Pane
cannot take changes nothing else — a default extension is set up from its
repository whether the index is readable or not.

1. **The check.** Once, when Pane starts (a cadence that is provisional:
no interval is checked meanwhile), Pane reads the index in the background
and compares the entry's version with the version it runs — dotted
numbers, compared by number; an entry for another target, a version Pane
cannot read, or an entry missing what it needs is explained, and so is a
source that cannot be reached (tried three times, like an interrupted
acquisition). An equal or older version says nothing: no downgrades are
offered or picked. The check reads only the index: **nothing is
downloaded until the user chooses**, and Pane does nothing else — no
download, no install, no restart (US76, [Q38](current-decisions.md)). The
status line says what was found ("Pane 99.0.0 is available"), and root
search lists the offer, after the install rows and before Manage
extensions: **Update Pane to 99.0.0**, its subtitle saying what
installing does — the user's
extensions and settings are kept, and the new version is used the next
time Pane starts. A check that failed lists **Check for a Pane update**
with why, which tries again.
2. **The install.** Choosing the row downloads the package with progress
   ("Downloading Pane 99.0.0: 34% of 186 MiB") and the same retries an
   interrupted acquisition gets (three attempts; a package that is not
   there, or whose bytes do not match the sha512 its entry gives, is
   explained and never retried), checks it, unpacks it — in the format
   the package's system packs: the zip a Windows or macOS package is,
   read as strictly as an npm package's tarball (only files and folders
   inside the package, one plain name per part on every system, every
   entry checked against the central directory and the file's own header
   and CRC32), or the gzipped tarball the Linux package is, read with
   the npm tarball's own strictness (only files and folders, extension
   headers read raw, an ambiguous size refused) — and stages it in the
   install folder's `update` folder. Then the swap: the running program
   is renamed out of its way — `pane.exe` to `pane.exe.old` on Windows,
   the bundle's `pane` to `pane.old` on macOS, `pane` to `pane.old` on
   Linux (every system allows renaming a running program; only
   overwriting one is refused) — the staged program takes its name and
   place, and the staging folder goes. **The new version is used the
   next time Pane starts** — the user's next start, whenever they
   choose; Pane itself never restarts. A start removes what earlier
   updates left: the renamed old program (best effort — another Pane may
   still run it) and a staging folder a Pane stopped mid-install left.
   Installing while Pane is being used is
   fine: the download runs off the thread, so commands and services keep
   answering while it goes, and only the last renames touch the program's
   folder, between two of the user's actions; a command still running
   when the user closes Pane ends as any command does, and the update it
   left staged is applied (or cleaned up) by the next start.
3. **What a failure leaves.** A failed check or install explains itself
   on the status line and leaves everything untouched: the program still
   the one running, no staging, nothing renamed. The row stays — the
   offer, or the check — and the user can try again. The old version's
   data is never touched: Pane's data and caches live where each system
   keeps them (`data\` and `cache\` under the install folder on
   Windows; `~/Library/Application Support/Pane` and
   `~/Library/Caches/Pane` on macOS; `~/.local/share/pane` on Linux,
   which the install folder does not even hold), and the swap changes
   only the program, so extensions, their settings, pins and enablement
   are exactly what they were.
4. **Where the user reaches it.** Root search's rows are one entry point
   and the Settings window's About page ([#82](https://github.com/pane-app/pane/issues/82))
   is the other: the page reads the same state the rows come from —
   `Launcher::application_update` — so the two cannot disagree, and its
   "Check for updates" and "Update Pane to <version>" rows run the same
   check and the same install the root rows run. The page also says the
   states the rows stay quiet about: that Pane is up to date (after a
   check the user asked for), that no artifact source is configured at
   all (a development build without `PANE_ARTIFACTS`, whose page explains
   the state rather than promising a release), and why installing the
   offer last failed, since the offer stays, ready to be chosen again.
   A check the user asks for from either entry point answers even when
   there is nothing to offer. The same page's **Log** row and root
   search's **Pane quit unexpectedly last time** row, which follows the
   update's rows, work the same way for Pane's local crash record (#133,
   [pausing](pausing.md#when-pane-itself-ends-its-log-and-the-crash-notice)),
   and the diagnostics the page copies name the log's folder.

The Windows install of an update is this whole path with the program at
`%LOCALAPPDATA%\Pane\pane.exe` (the install script's target, and the
shortcut's, which the swap keeps pointing at the right file) — the zip
package its entry names unpacked by the zip reader. The Linux install
([#56](https://github.com/pane-app/pane/issues/56)) runs the same path
with the program at `~/.local/bin/pane` — the install script's target,
which the desktop entry the script put in `~/.local/share/applications`
keeps naming (the swap changes only the program, so the entry never
points anywhere else) — unpacking the tarball the Linux package is with
the tar reader npm tarballs are read by, and Pane's data staying in
`~/.local/share/pane`, which the install folder does not even hold.
Each is one call in `pane`'s `main.rs` giving the program's own path.

The macOS install ([#55](https://github.com/pane-app/pane/issues/55))
is the same call with the program at
`~/Applications/Pane.app/Contents/MacOS/pane`, so the swap replaces
**the binary inside the bundle** and the bundle itself stays: replacing a
whole `Pane.app` under a running Pane would break it, and would take the
`Pane.app` the user sees in Finder and Launch Services knows away from
them — the binary the bundle's `CFBundleExecutable` already names is the
one an update replaces, in place. The old binary is renamed `pane.old`
and the staging folder is `update/`, both beside the binary in
`Contents/MacOS`, and a later start removes them; Finder and Launch
Services keep opening the same bundle, whose program answers the new
version. What the swap does **not** update, as a provisional limit: the
bundle's `Info.plist` stays the file the install script wrote, so its
`CFBundleShortVersionString` and `CFBundleVersion` still name the
installed version while the program itself reports the new one (`pane
--version`), and Finder's "Get Info" shows the plist's version — the
two disagree until the bundle is reinstalled from a package. Writing
the new version's keys into the plist with the swap is an open choice
recorded for the user. The binary an update installs is one Pane wrote
itself, so it carries no Gatekeeper quarantine mark and launches as the
old one did; a signed bundle's signature would not survive a binary
replaced inside it, which is one more reason signing is a release
prerequisite (recorded [below](#limits-and-prerequisites)).

## The artifact source

The artifact source serves Pane's own application updates alone (since
#278, nothing about a default extension comes from it: the defaults are
fetched from their repositories). Release builds read it at
`https://downloads.pane.sh/` only. Tests and
development builds can name a source on this computer instead
(`PANE_ARTIFACTS`, read as [`Registry::local` in
npm](npm.md#the-registry) is: an `http://` or `https://` address on a
literal loopback address — `127.0.0.1`, any `127.x.y.z`, `[::1]` — with an
optional port and path; `localhost` and anything else is refused), so no
check ever reaches the network. **A release build has no way to replace
the published source.** The smokes serve `target/dist/artifacts` with
[scripts/artifact_server.py](../scripts/artifact_server.py) on 127.0.0.1;
the tests serve their own index and application package in process
(`crates/pane-core/tests/support/artifacts.rs`).

A development build with no `PANE_ARTIFACTS` checks for no application
update, so a checkout runs nothing over the network by itself.

## Trying a first setup by hand

A development build fetches the default extensions from the repositories
its pins name. The committed pins point at the real repositories on
GitHub; to watch a first setup without reaching them, serve clones of the
pinned revisions on this computer and point `PANE_DEFAULTS` at the pins
that name them:

```sh
mkdir -p /tmp/pane-repositories
python3 scripts/repository_server.py serve /tmp/pane-repositories /tmp/port &
while [ ! -s /tmp/port ]; do sleep 0.1; done
python3 scripts/repository_server.py clone-defaults crates/pane/defaults.json \
  /tmp/pane-repositories /tmp/pins.json "http://127.0.0.1:$(cat /tmp/port)/"
PANE_DATA_DIR=/tmp/fresh PANE_DEFAULTS=/tmp/pins.json cargo run -p pane
```

A fresh data folder (`PANE_DATA_DIR=/tmp/fresh`) shows the acquisition
and the calculator's answer to "6*7". (The clone step reaches GitHub
once, as the smokes' own setup does; the Pane under test fetches only
from 127.0.0.1. A development build with no `PANE_ARTIFACTS` checks for
no application update.)

On Windows, the same with `python` instead of `python3` (the server in
another terminal, or started in the background):

```powershell
python scripts/repository_server.py serve $env:TEMP\pane-repositories $env:TEMP\port
python scripts/repository_server.py clone-defaults crates/pane/defaults.json `
  $env:TEMP\pane-repositories $env:TEMP\pins.json "http://127.0.0.1:$((Get-Content $env:TEMP\port).Trim())"
$env:PANE_DATA_DIR = "$env:TEMP\fresh"
$env:PANE_DEFAULTS = "$env:TEMP\pins.json"
cargo run -p pane
```

On macOS, the same as on Linux (`python3`, the server backgrounded with
`&`).

## Checks

- `crates/pane-core/tests/application_update.rs`: the application update
  through
  the launcher's public interface, against the same loopback artifact
  source — a newer version offered as a row in root search with nothing
  downloaded until the user chooses it and nothing changed when they do
  not; choosing it downloading the package, checking it and swapping the
  running program (the staged outcome observable: the new program in
  place, the old one renamed away, the staging gone, the offer's row
  gone, and a later start removing what the update left); the Linux
  package's tarball installing through the same path as the zip the
  Windows one is (the suite runs on every system, so both formats are
  installed wherever the tests run); a damaged
  package, an unreachable source, a source that answers an error and a
  replacement that cannot be made each explained with everything
  untouched and the row ready to try again, and the retry that installs
  once the source works; an index whose application entry names another
  system, or an older version, and one whose format version Pane does not
  read, explained without stopping a default extension being set up from
  its repository; progress on the status line while the core stays
  usable; and Pane's data — an extension set up at first setup, its
  record and cache —
  untouched by an install, still installed and answering after the
  update. The zip Pane unpacks is checked by `pane-core`'s own unit
  tests (both storage methods, and every refusal), and the tarball the
  Linux package is, by the npm tarball reader's own unit tests.
- `crates/pane-core/tests/installer.rs`: the acquisition through the
  launcher's public interface, over repositories served on 127.0.0.1 by
  Git's smart HTTP protocol (`support/repo_server.rs`), with a default
  set of its own (the icons sample and the helper sample, a revision
  carrying a real helper program)
  — a first setup installing both as managed copies with the default
  identity and the Git source recorded in `installed.json`; what is being
  set up said on the status line while the
  core stays usable; an interrupted fetch recovered by retry; an
  unreachable repository leaving
  the core usable with the row that tries again, and the row setting the
  extension up once the repository works; the helper running from the managed
  copy with mode 0755; a revision without a `pane.json`, a source-only
  revision, an incompatible package and one without its declared helper
  file, explained and not installed; a restart
  fetching nothing; a default with retained data (one the user
  uninstalled) not re-acquired, and a disabled one not either; a
  default extension that left the build's default set (the helper
  sample, #162) kept installed, fetched for nothing and uninstallable,
  and not brought back once uninstalled; and an install that acquired a
  default from the artifact source (an older Pane) keeping it, its
  identity and data unchanged. These
  run on every system, so the Windows and macOS acquisitions need no
  test of their own: it is the same code (the one Windows-only piece is
  the `.exe` helper-name rule, checked by the runner's unit tests).
- `crates/pane-core/tests/update.rs`: the default extensions' updates
  through the launcher's public interface, over the same loopback
  repositories — a newer release tag updating a default by itself with
  its identity, data, disabled state, hotkeys and aliases kept (and a
  newest tag that names the version installed fetching nothing); one
  disabled, turned off or uninstalled-with-kept-data not updated
  automatically, and a pass the user asked for updating the turned-off,
  disabled and paused ones (a disabled one stays disabled, a paused one
  is unpaused); a payload needing a newer Pane skipped with that reason;
  a new version that fails to start paused with Retry and recorded under
  Failed, not rolled back; an update whose required dependency cannot be
  installed changing nothing; the per-extension switch and the global
  choice covering defaults; and an unreachable repository recorded and
  leaving the installed copy alone. Every repository is served on
  127.0.0.1, so nothing reaches a real one.
- The [Linux smoke](platforms/linux.md#installing-pane-and-acquiring-its-calculator-53),
  the [Windows smoke](platforms/windows.md#installing-pane-and-acquiring-its-calculator-51)
  and the [macOS smoke](platforms/macos.md#installing-pane-and-acquiring-its-calculator-52):
  the package is built and installed on a clean machine — a fresh home
  folder on Linux and macOS, a fresh user profile on Windows — and the
  installed Pane, started with a PATH that holds nothing at all, fetches
  the five default extensions' pinned commits from their repositories
  (cloned at those commits by the smoke's own setup and served on
  127.0.0.1; the Pane under test reaches no network address) and
  answers "6*7" with 42. (A helper running from an acquired revision is
  `installer.rs`'s, above; the smokes run the helper sample installed
  with `--install`.) (The install script itself runs
  with `/usr/bin:/bin` on Linux and macOS, so the fresh home stays clean
  while the script's tools resolve; on Windows it runs with the empty
  PATH, its PowerShell script needing nothing from one. What is checked,
  and how, is each platform page's own record.)
- `xtask`'s packaging is checked by building it: `cargo xtask
  package-linux [--dev]` must produce the tarball, its sha256 and the
  artifact tree, `cargo xtask package-windows [--dev]` and
  `cargo xtask package-macos [--dev]` the zip, its sha256 and the same
  tree; the release-profile package is built the same way the
  development-profile one the smoke installs is. The zip's bytes are
  pinned by a unit test (`xtask/src/zip.rs`), first decoded with Python's
  `zipfile`; the Linux machine that writes most of this repository cannot
  build `pane.exe` or the macOS `pane` program, so there the Windows and
  macOS tasks assemble the artifacts and explain that only a checkout of
  their own system builds the package.

## Limits and prerequisites

- **The artifact source is not deployed.** `downloads.pane.sh` does not
  exist, so a Pane installed from today's package is told its check for
  an update of its own failed and offered the row that tries again; a
  development build checks only where `PANE_ARTIFACTS` names a source.
  First setup is unaffected: it fetches the default extensions' pinned
  commits from their repositories. Deploying the source (serving
  `artifacts/` above) is an execution prerequisite, like the signing
  credentials below.
- **Nothing is signed.** No signing credentials exist, so the package is
  not signed: its `.sha256` says only what was packed, and the
  application package an update downloads is checked only against the
  sha512 its index entry gives, over HTTPS. (The connection trusts the
  system's certificates.)
  Signing the package, and serving the index over an authenticated
  channel a release trusts, are prerequisites for a release. On Windows
  this means no Authenticode certificate exists either, so `pane.exe`
  and `install.ps1` are unsigned: Windows may warn about an unknown
  publisher when the program runs, and PowerShell may refuse the install
  script as one that came over the internet (the README and the doc above
  say how a user answers that for this one script). Acquiring a
  certificate, and signing the program and the script with it, are
  execution prerequisites like the others here. On macOS there is no
  Apple Developer ID certificate either, so `pane` and `install.sh` are
  unsigned and nothing is notarized: Gatekeeper blocks the first launch
  of a Pane.app a browser downloaded (the README and the doc above say
  how the user answers that), while a package built on the machine runs
  at once. Acquiring the certificate, signing and notarizing are
  prerequisites for a release, like the others here.
- **The artifact this build serves is built for the system it ran on.**
  Its `application` entry names the one target the task built for
  (`linux-x86_64`, `windows-x86_64` and `macos-aarch64` on CI,
  `linux-aarch64` on an arm64 checkout; a Pane takes only the entry for
  its own target, and the index writer names one entry); a real
  deployment must build every supported target and serve one package per
  target, an index holding one entry for each.
- **One package per system**, built where it runs; cross-building and
  packaging for a system this task cannot build on is out of scope (the
  evidence for each is per-system, as the specification requires). On
  Windows and macOS the task says so and stops rather than packing
  another system's program; on Linux the task builds whatever program
  the checkout builds (#53's behavior, kept).
- **The Windows baseline is one system.** `windows-2025` (Windows Server
  2025, x86_64) is the declared baseline, the system CI builds, packages,
  installs and smokes on; no Windows 10 or 11 client edition, no ARM64
  and no per-user install on a real desktop has been tried, and the
  smoke's clean machine is a fresh profile on that runner, not a fresh
  machine. The install script and the smoke phase were written without
  PowerShell on the machine that wrote them, so their runtime evidence is
  CI's Windows leg (recorded in
  [platforms/windows.md](platforms/windows.md#installing-pane-and-acquiring-its-calculator-51)).
- **The macOS baseline is one system.** `macos-15` (macOS 15, arm64) is
  the declared baseline, the system CI builds, packages, installs and
  smokes on; no Intel Mac, no other macOS version and no install on a
  Mac of a user's own has been tried, and the smoke's clean machine is a
  fresh home folder on that runner, not a fresh machine. The install
  script and the smoke phase were written without a Mac on the machine
  that wrote them (the script was linted and dry-run with a fake program
  instead), so their runtime evidence is CI's macOS leg (recorded in
  [platforms/macos.md](platforms/macos.md#installing-pane-and-acquiring-its-calculator-52)).
- **The update's cadence is provisional.** Pane checks when it starts and
  at no interval; how often a running Pane rechecks (and whether a check
  that failed retries quietly) is a choice recorded for the user.
  Extension updates and application updates stay separate, as the
  specification requires: extension updates have their own controls
  (npm packages update automatically since #49, tracked Git branches since #50), and the application update has none —
  only the user's choice, every time.
- **Every system's wiring exists.** The check, download, verification
  and swap are platform-independent `pane-core` code, and the Windows,
  macOS and Linux builds each wire them to their own program. The swap
  is exercised by the tests wherever they run; the running-program
  rename it depends on is proven on Windows itself by the smoke, and
  the macOS and Linux smokes prove their own when they run (pending
  their CI, as their
  [platform pages](platforms/macos.md#installing-a-pane-application-update-by-the-users-choice-55)
  record).
- **Nothing about an update is signed either**, and the source it comes
  from is the same not-yet-deployed one: a package is checked only
  against the sha512 its index gives, over HTTPS. An application package
  is at most 512 MiB packed and 2 GiB unpacked (the program is large; a
  development build of it much more), and it is not cached between
  attempts: an interrupted download starts over, and only the retries
  within one install attempt keep it cheap.
- **Concurrent Panes** on one install folder: both may check and offer;
  two installs race by failing honestly (the swap's renames cannot both
  happen), and a Pane starting removes a staging folder another Pane may
  be installing from — the same small warts the shared data folder
  already records, left as they are.
- **A default extension's repository must tag its releases.** Its
  updates read the repository's `v<semver>` release tags
  ([#269](https://github.com/pane-app/pane/issues/269), ADR 0044): a
  repository that never tags a newer release — one that only moves its
  default branch — never updates the extension, and neither does a tag
  that names the version installed. The extension's author chooses when
  to release; a Pane release moves its pins when it tests one.
- **Concurrent Panes** on one data folder both acquire; each fetches its
  revision into a download folder of its own under `downloads/`, removed
  once its package is dropped. One that installs the same
  default extension while the other is installing it is told the install's
  own wording, "Already installed …; use Update to replace the installed
  copy" — wording a default extension has no row for (a restarted Pane
  lists it): a small wart of the shared install path, left as it is.
