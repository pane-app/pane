# Pane's packages and first setup

Pane installs from one package per system. You unpack it, run its install
script, and start Pane; Pane then sets up its default extensions itself
over the network. This page covers installing on each system, that first
setup, how Pane and its default extensions update, and the tests and
limits behind all of it.

The installer carries no payloads and installs no runtime, and you install
no Node, Rust, npm, Git or compiler. Pane's extension runtime (Wasmtime)
is part of Pane's own process, so nothing is acquired for it. This is the
internet-first setup the specification chose
([decision 20](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/launcher-design-interview.md)).

## Installing Pane

Each system has its own package: a `.tar.gz` for Linux and a `.zip` for
Windows and macOS. Windows also has a setup, `Pane-Setup-<version>.exe`,
which installs Pane as other Windows apps install; the zip stays as the
portable download. macOS and Linux have an installer that works like any
other app's too ([#464](https://github.com/pane-app/pane/issues/464)): a
disk image (`Pane-<version>.dmg`) to drag Pane from, and an AppImage
(`Pane-<version>-<arch>.AppImage`) to run as it is. Every install is for
one user and needs no root or administrator rights. Nothing is signed yet,
so Windows and macOS may warn or block at first; each section below says
how to answer that.

### Installing on Linux

The AppImage is one file holding the `pane` program, its desktop entry
and its icons. Nothing is installed:

```sh
chmod +x Pane-<version>-x86_64.AppImage
./Pane-<version>-x86_64.AppImage
```

It runs where it is saved. An AppImage integration tool (Gear Lever,
AppImageLauncher, appimaged) adds it to the applications menu with its
icon, from the desktop entry and icons the AppImage carries. Pane itself
writes no menu entry for it. The AppImage's runtime is the static type 2
one, so the system needs no `libfuse2`. It does need FUSE to mount the
image; without FUSE, `--appimage-extract-and-run` runs it extracted
instead. The libraries the tarball's prerequisites below name are needed
the same way: the AppImage bundles none of them. Launch at login names the
AppImage file, and an update replaces that file
([below](#on-each-system)). To uninstall, delete the file. Pane's own data
stays in `~/.local/share/pane`.

The tarball installs the same program the traditional way:

```sh
tar -xzf pane-<version>-linux-<arch>.tar.gz
bash pane/install.sh                # installs to ~/.local
bash pane/install.sh --prefix DIR   # or somewhere else
pane --version                      # what the install script runs to check
```

The script installs for one user, and no root is needed:

- `pane` goes to `<prefix>/bin`.
- The desktop entry goes to `<prefix>/share/applications`.
- Its icon goes to `<prefix>/share/icons/hicolor/<size>/apps/pane.png`,
  in every size from 16 to 512 pixels. The entry names it (`Icon=pane`),
  and its `StartupWMClass=pane` matches the class Pane's windows carry,
  so the desktop shows the icon for them.

It checks the program's libraries with `ldd`. If one is missing, it names
it and the Ubuntu package that provides it, rather than installing
anything.

The declared baseline is Ubuntu 24.04 (x86_64, X11), the combination
[CI runs](platforms/linux.md#ci-result). The prerequisites are these
libraries and a Vulkan driver (`mesa-vulkan-drivers` works without a GPU):

```text
libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libxcb1
libfontconfig1 libfreetype6 libvulkan1
```

To uninstall, remove the program, the entry and the icons. Pane's own
data stays in `~/.local/share/pane`.

### Installing on Windows with the setup

Run `Pane-Setup-<version>.exe` from the release
([#463](https://github.com/pane-app/pane/issues/463)). It is an Inno Setup
installer (`scripts/setup-windows.iss`) and installs for the user who runs
it, with no administrator prompt (`PrivilegesRequired=lowest`):

- `pane.exe`, `LICENSE.txt` and `THIRD-PARTY-NOTICES.txt` go to
  `%LOCALAPPDATA%\Pane`, the folder `install.ps1` has always installed to,
  so a setup run over an install the script made replaces it in place. The
  first install lets the user choose another folder; an upgrade keeps the
  folder of the install it upgrades.
- A `Pane` shortcut goes to the user's Start menu, the same
  `Pane.lnk` the script makes.
- An entry in Settings → Apps: Pane, by pane-app, with its version and
  Pane's icon, from which Pane is uninstalled. It is the current user's
  `Uninstall` key for the setup's AppId,
  `{0737054E-4FDB-4FAD-8AD1-20C980B0C16E}_is1`. The AppId never changes:
  every version's setup finds an earlier install by it.
- An optional task, **Start Pane when I sign in**, unticked by default.
  Ticked, the setup starts Pane with `--launch-at-login`, which turns on
  Settings' **Launch Pane at login** exactly as its switch does: Pane
  records the choice and registers itself in the `Run` key (see
  [autostart](../crates/pane-core/src/autostart.rs)). The setup writes no
  registration of its own, which Pane's start-time reconciliation would
  remove as one the user did not choose. The task is never carried over
  from an earlier install, since the choice is Pane's setting from then on.
- When it finishes, the setup starts Pane (a checkbox on its last page).

A setup run over a running Pane closes it first, through Windows' Restart
Manager: Pane takes the end of its session as a clean quit, and one that
does not end is ended (`CloseApplications=force`). That includes a Pane
still running as `pane.exe.old` after an in-app update. Pane's data is in
`%LOCALAPPDATA%\Pane\data` whichever folder the program is in, and the
setup never writes there, so an upgrade keeps the extensions, their
settings and Pane's own settings.

Uninstalling (Settings → Apps, or `unins000.exe` in the install folder)
closes a Pane running from the install, removes the program, the shortcut,
the Apps entry and Pane's sign-in registration when it starts this
install, and then asks whether to remove Pane's data too
(`%LOCALAPPDATA%\Pane\data`, `cache` and `logs`). No is the default, and a
silent uninstall's answer, so the data is kept for a later install.

For a silent install, as an administrator's script would run it:

```powershell
Pane-Setup-<version>.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART [/DIR="<folder>"] [/TASKS=startatsignin]
"%LOCALAPPDATA%\Pane\unins000.exe" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
```

A silent install starts Pane only with the sign-in task.

### Installing on Windows from the zip

```powershell
Expand-Archive pane-<version>-windows-<arch>.zip        # or unzip in Explorer
cd pane
powershell -ExecutionPolicy Bypass -File install.ps1    # installs to %LOCALAPPDATA%\Pane
pane --version                                          # what the install script runs to check
```

The script (`scripts/install-windows.ps1`, mirrored by the Linux one)
installs for one user:

- `pane.exe` goes to `%LOCALAPPDATA%\Pane`. That is an environment
  variable of the user's own, so no administrator rights are needed.
  `-InstallDir <folder>` installs somewhere else.
- A `Pane` shortcut goes to the user's Start menu
  (`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Pane.lnk`). It is the
  Start-menu entry the [applications](applications.md) extension finds.

No desktop entry is made, because an installer that asks no questions
puts nothing on the desktop. The script checks what it installed by
running `pane --version` (its exit code: Pane is a window-subsystem
program, so the version text is read from a redirected stream).

Pane keeps its own data in `%LOCALAPPDATA%\Pane\data` and its caches in
`%LOCALAPPDATA%\Pane\cache`, inside the same per-user folder the program
is installed to. To uninstall, remove `pane.exe` and the shortcut, and
that data folder if you want it gone too.

The declared baseline is Windows Server 2025 (x86_64), the system
[CI runs](platforms/windows.md#tested-combination). The program needs
nothing beyond Windows itself: no Node, Rust, npm, Git, compiler, and no
administrator rights.

Because nothing is signed, PowerShell may refuse `install.ps1` as a
script that came over the internet (a zip downloaded from the web and
extracted can carry that mark). The `-ExecutionPolicy Bypass` above
answers that for the one script; so does `Unblock-File install.ps1` run
once. The package's `.sha256` file says what was packed.

Windows itself may warn about an unknown publisher when `pane.exe` runs.
That is what absent Authenticode credentials mean, recorded plainly
[below](#limits-and-prerequisites).

### Installing on macOS

Open `Pane-<version>.dmg` and drag Pane onto the Applications folder
beside it. The disk image holds the whole `Pane.app`: the program, its
`Info.plist` and icon, and the licence and notices in
`Contents/Resources`. Dragging it to `~/Applications` instead needs no
administrator rights. Pane is an accessory app
([#458](https://github.com/pane-app/pane/issues/458)): it shows no Dock
icon and no Cmd+Tab entry. It lives in the menu bar and opens with its
hotkey. Eject the disk image once Pane is copied. Pane run straight from
the image works, but cannot update itself, since the image is read only.

The first open of a downloaded Pane.app is blocked, as
[below](#limits-and-prerequisites) explains. macOS says it cannot check
the app for malicious software. Choose **Done**, then open **System
Settings → Privacy & Security**, find the message about Pane near the
bottom, and choose **Open Anyway**. macOS asks once more, then opens it,
and never asks again for that copy. `xattr -dr com.apple.quarantine
/Applications/Pane.app` in Terminal does the same.

The zip installs the same bundle with a script:

```sh
unzip pane-<version>-macos-<arch>.zip        # or double-click it in Finder
cd pane
bash install.sh                # installs Pane.app to ~/Applications
bash install.sh --app-dir DIR  # or into another folder
```

The script (`scripts/install-macos.sh`) installs for one user. It builds
the `Pane.app` bundle in `$HOME/Applications`, a folder of the user's own,
so no administrator rights are needed. The bundle is made from the
package's `pane` program, `Info.plist` and the icon it names
(`Contents/MacOS/pane`, `Contents/Info.plist` and
`Contents/Resources/pane.icns`), and the script checks what it installed
by running that program's `--version`.

Nothing is put on the PATH. The bundle opens with a double-click in
Finder or `open ~/Applications/Pane.app`, and the program for a terminal
is `~/Applications/Pane.app/Contents/MacOS/pane` (the one `--version`
answers, as the install script runs it). `~/Applications` is one of the
folders the [applications](applications.md) extension searches, so
installed Pane finds itself. No Dock or desktop entry is made beyond the
bundle itself.

To uninstall, remove the bundle. Pane keeps its own data in
`~/Library/Application Support/Pane` and its caches in
`~/Library/Caches/Pane`.

The declared baseline is macOS 15 on Apple silicon (arm64), the
combination [CI runs](platforms/macos.md#tested-combination). The program
needs nothing beyond macOS itself: no Node, Rust, npm, Git, compiler, or
administrator rights.

Nothing is signed (no Apple Developer ID certificate exists, and nothing
is notarized), so Gatekeeper matters where a package came over the
internet. A browser or mail program marks what it downloads, and macOS
blocks the first launch of an app it cannot check until the user allows
it in System Settings (Privacy & Security). A zip built on the machine
itself, such as a CI runner's for the smoke, carries no quarantine mark
and runs at once.

The zip stores no execute permission (the same plain zip as the Windows
one), so the install script's copy is what sets the program's. Check the
package's `.sha256` file when it reached you over the internet. Signing
and notarizing the program are execution prerequisites recorded
[below](#limits-and-prerequisites).

### What happens on first start

Pane asks which of its official extensions to set up, then fetches them
in the background while the rest of Pane stays usable. See
[Acquiring the default extensions](#acquiring-the-default-extensions).

The acquisition is the same on every system: it is platform-independent
code, tested by `crates/pane-core/tests/installer.rs`. What each system
has of its own is the package, the install script and the smoke that
proves the whole outcome there.

## Building the packages

Each task builds, under `target/dist/`, one package for the system it
runs on, and the assets a release of Pane carries for it:

- `pane-<version>-linux-<arch>.tar.gz` is the Linux package, built by
  `cargo xtask package-linux`. Under `pane/` it holds the `pane` program
  (release profile), the install script, a `README.txt`, a
  `pane.desktop` entry and its icons (`icons/hicolor/<size>/apps/pane.png`).
- `Pane-<version>-<arch>.AppImage` is the Linux installer, built by the
  same task from the same staged files when `APPIMAGETOOL` and
  `APPIMAGE_RUNTIME` name an `appimagetool` and the AppImage type 2
  runtime to build it with. Without them the task says it built none.
  CI's Package job downloads both, pinned to a release and checked against
  their digests (`appimagetool` 1.9.1, runtime 20251108). Its AppDir holds
  `AppRun` (a link to `usr/bin/pane`), the desktop entry and the 256-pixel
  icon at the top (with `.DirIcon`), the entry and every icon size under
  `usr/share`, and the licence and notices in `usr/share/doc/pane`.
- `pane-<version>-windows-<arch>.zip` is the Windows package, built by
  `cargo xtask package-windows`. Under `pane/` it holds the `pane.exe`
  program (release profile), the PowerShell install script and a
  `README.txt`. It is a zip because a Windows user unzips with whatever is
  at hand, and Windows has no tar of its own a user can rely on.
  `pane.exe` carries Pane's icon and version information, which the pane
  crate's build script compiles in (`crates/pane/build.rs`).
- `pane-<version>-macos-<arch>.zip` is the macOS package, built by
  `cargo xtask package-macos`. Under `pane/` it holds the `pane` program
  (release profile), the install script, a `README.txt`, and the
  `Info.plist` and icon (`pane.icns`) of the `Pane.app` bundle the script
  builds.
- `Pane-<version>.dmg` is the macOS installer, built by the same task with
  `hdiutil`, which every Mac has. It is a compressed (UDZO) HFS+ image of
  a volume named Pane, holding that same bundle, already built, beside a
  link to `/Applications`.
- Every package also holds Pane's licence (`LICENSE.txt`) and the
  third-party notices (`THIRD-PARTY-NOTICES.txt`), and its README says
  where the source of its version is ([licensing](licensing.md)). The
  icons are drawn from Pane's mark by `scripts/icons/app-icons.py` and
  committed (`crates/pane/assets/app/`).
- `<file>.sha256` is each package's and each installer's digest.
- `release/` holds the assets a GitHub release of Pane carries for this
  system ([below](#the-release-source)): the package, named
  `pane-<version>-<os>-<arch>.<zip|tar.gz>` (a development package takes
  that name here too, standing in for a release's), and its `.sha256`,
  and the installer with its own. These names are what Pane's updater
  looks for in a release. The release
  workflow uploads this folder; the smokes serve it from this computer
  instead. Each system's run of its own task builds its own.
- On Windows, `release/` also holds the setup, `Pane-Setup-<version>.exe`,
  and its `.sha256`. `package-windows` runs Inno Setup 6's compiler
  (`ISCC.exe`: the one the `ISCC` variable names, else the one on the
  PATH or where Inno Setup installs itself) over
  `scripts/setup-windows.iss` and the program, licence and notices staged
  for the zip, stamping the setup with the package's version. Without the
  compiler the task stops and says how to get it (`winget install
  JRSoftware.InnoSetup`). CI installs Inno Setup 6.7.1, pinned by its
  digest. A `--dev` package has no setup: the smokes install the zip, and
  a development program is too large to compress for nothing. The setup
  is not the same bytes from build to build (the compiler stamps it), so
  its `.sha256` is for checking a download, not for reproducing one.

### Build options

- `--dev` builds the development profile, and the package name ends
  `-dev` (on Windows too). The native smokes install that one, because
  only a development build takes its default extensions' pins from
  `PANE_DEFAULTS` and its releases from `PANE_RELEASES`. A release build
  uses the committed pins and Pane's published releases, which no
  controlled source may replace.
- `--package-version <version>` builds the program reporting that
  version and names the package and its release assets by it. It is a
  build-tool option for the smokes, which need a newer version than the
  one installed to offer. A release build simply builds the workspace's
  own version. Nothing gates the option: it names the package, and anyone
  building one can name it.

The Windows task builds the package only on Windows. `pane.exe` needs a
Windows checkout (no cross toolchain is set up), so anywhere else the task
explains so and stops. The macOS task does the same for macOS.

### Same bytes everywhere, nothing signed

Each package is the same bytes wherever it is built: a fixed time, owner
and mode in the Linux tar, and a fixed time, entry order and deflate in
the Windows and macOS zips. The installers are not: `hdiutil` and
`mksquashfs` write times of their own, so their `.sha256` says what one
build wrote. Each program is built for the system the task ran on:

| Where the task runs | Builds |
| --- | --- |
| this machine | `linux-aarch64` |
| CI's `ubuntu-24.04` runner | `linux-x86_64` |
| CI's `windows-2025` runner | `windows-x86_64` |
| CI's `macos-15` runner | `macos-aarch64` |

A release for several systems builds one package per system.

Nothing is signed. No signing credentials exist (on Windows, no
Authenticode certificate), and the `.sha256` file says only what was
packed. Signing the package is an execution prerequisite recorded
[below](#limits-and-prerequisites).

The package holds no default extension. First setup fetches them from
their repositories' pinned commits, so the installer stays small, and
every default extension (also the ones later slices add) is a normal,
individually disableable extension rather than something baked into the
program.

## Acquiring the default extensions

A default extension ([glossary](../CONTEXT.md)) is identified by its id,
such as `calculator`. The id is also its package identity:
`default:calculator`, recorded as `"default": "calculator"` in
`installed.json` together with the Git source of the revision it was
fetched from, whatever version is installed.

The release's default extensions are the calculator, applications,
quicklinks, files and clipboard history
([#60](https://github.com/pane-app/pane/issues/60), the user's recorded
choice). All five are set up at first setup and each is individually
disableable. Clipboard history records from the first start
([ADR 0042](adr/0042-clipboard-history-records-from-the-first-start.md)).

Every build sets up the same five on every system. The Windows power
features' Run, System Commands and Switch Windows
([ADR 0040](adr/0040-default-extensions-grow-to-include-windows-tools.md))
are pinned with `"platform": "windows"` and set up on Windows alone. The
pins file's platform field is read before any repository is fetched (the
launcher's platform gate), so a Linux or macOS first setup never fetches
them.

No sample is a default extension
([#162](https://github.com/pane-app/pane/issues/162)). Until #162 a
development build also acquired the prebuilt-helper sample. An install
that acquired it keeps it as an ordinary installed package (Pane removes
nothing it acquired), which the user can uninstall, and no later first
setup acquires it again. The samples stay installable by hand
(`pane --install target/guests/packages/<name>`).

### Where they come from

Since [#278](https://github.com/pane-app/pane/issues/278) each default is
fetched from its own repository, at the commit of the release tag this
Pane release pins
([ADR 0045](adr/0045-official-extensions-live-in-their-own-repositories.md)).
The eight live in public repositories of their own in the
[`pane-app`](https://github.com/pane-app) organization
([#279](https://github.com/pane-app/pane/issues/279),
[#301](https://github.com/pane-app/pane/issues/301)).

The pins are committed to the build
([`crates/pane/defaults.json`](../crates/pane/defaults.json)). Each names
the default's id, title, repository, release tag, that tag's commit and,
for the Windows-only three, the platform they are set up on. A newer Pane
release moves the pins forward, as for the five. Before #278 the defaults
came from Pane's artifact source; that machinery is gone (ADR 0045 records
why), and Pane's own application updates come from its GitHub releases
(ADR 0048).

A development build can replace the pins with a file of its own through
`PANE_DEFAULTS`. Its repositories must be reachable as a Git address is:
HTTPS, or a loopback address in these builds alone. That way the tests
and smokes serve the repositories on this computer and no check ever
reaches a real Git host. A release build has no override.

### How each one is acquired

At first setup, and whenever a default extension is missing, Pane
acquires each in turn in the background:

1. The fetch names the repository at the pinned commit. A commit id pins
   the bytes ([ADR 0021](adr/0021-pane-fetches-git-packages-itself.md)),
   so the tag is recorded but never asked for: a tag the repository moved
   does not move what this release installs.
2. Pane's own Git client fetches exactly the pinned commit. It uses Git's
   smart HTTP protocol version 2 (ADR 0021), over the same HTTPS stack npm
   and Git packages use, with no `git` program, library or configuration.
   It checks every object against its id and writes out only the
   revision's files and folders, into a download folder of its own under
   `downloads/`, removed once the package read from it is dropped.

   An interrupted fetch (the connection failing partway, or a server
   error: 403, 500, 502, 503, 504) is tried again, up to three times,
   after 0.5 s and 1 s. A revision the repository refuses, or one whose
   files fail their checks, is explained, not retried.
3. The fetched revision is read, checked, planned and installed exactly
   as a package from a folder is: into a managed copy, with the default
   extension's identity, and with its Git source (repository, tag,
   commit, pinned) recorded for
   [#269](https://github.com/pane-app/pane/issues/269)'s updates. Its
   compatibility is what any package's is: the manifest version and
   extension API this Pane reads, the platforms it declares, the
   components it names present, and, where it ships helpers, the file for
   this system a real program for it.

A default extension the user declined (unticked at first setup, or
uninstalled, whether its data was kept or not) is not acquired again.
That is the user's choice, with disabling the other opt-out. One never
installed is acquired, once first setup's choice is made.

### First setup's choice

Before anything is fetched, first setup asks
([#283](https://github.com/pane-app/pane/issues/283),
[ADR 0045](adr/0045-official-extensions-live-in-their-own-repositories.md)).
On a data folder whose first setup has not chosen, the launcher opens on
**Pane's Official Extensions**. It lists each default extension this
system sets up by its pin's title and description (the pins file's
`description`; nothing is fetched to list them) and the release it sets
up. Every one is ticked, and the **Set Up** row after them is selected.

- The arrows move the selection.
- Space, the row itself or its check mark ticks or unticks one.
- Enter on Set Up records the choice in `installed.json`
  (`"firstSetupChosen": true`, and each unticked default's id under
  `"declinedDefaults"`, the record an uninstall of a default writes too)
  and returns to root search, where the ticked ones are acquired as
  described above. Unticking every one sets up none.
- Leaving the screen (Escape) chooses nothing. Root search offers **Set Up
  Official Extensions…** to open it again, and the next start asks again.

Once the choice is made it is never asked again. Each default that is
missing and not declined (one whose acquisition failed, or one a later
Pane adds) is acquired by itself at every start. An install that set its
defaults up before the choice existed holds a default already, and is
never asked.

Settings' Extensions group lists, under **Official Extensions**, each
official extension this system sets up that is not installed (declined,
or never acquired), with what it does and an **Install** button. Install
acquires it from its pin as the row that tries again does, and drops it
from `"declinedDefaults"`. The choice holds across restarts.

### What the user sees

The status line says what is happening ("Acquiring the Calculator…")
while the window, root search, the install rows and Settings › Extensions
stay usable: acquisition never blocks anything. A Git fetch answers with
one pack, so there is no byte progress to follow and no percentage. When
every default extension is set up, the status line says "Set up the
Calculator" (or "Set up Pane's default extensions").

When a default extension could not be acquired:

- The status line explains it ("Could not set up the Calculator: Could
  not reach the Git repository … (Pane tried 3 times)").
- Root search offers it as a row after the install rows, **Set up
  Calculator**, which tries again. The row goes once what it asked for is
  there.
- Starting Pane tries again by itself, so a Pane stopped mid-setup
  recovers.

Disabling a default extension (in Settings › Extensions) is an opt-out
beside declining it. A disabled default extension is installed, so it is
never re-acquired or re-enabled.

## Updating the default extensions

Between Pane releases a default extension updates from its repository's
newer release tags ([#269](https://github.com/pane-app/pane/issues/269),
[ADR 0045](adr/0045-official-extensions-live-in-their-own-repositories.md)).
Once Pane is up and running (a minute after it starts, then daily; see
[npm's updates](npm.md#updating-by-itself)), it reads the repository its
record names and lists that repository's `v<semver>` tags. It compares
each tag's version with the version the record's `defaultVersion` says is
installed.

The newest release above that version is fetched at the commit its tag
points to, which pins the bytes. It is staged, checked and applied exactly
as the update of an npm or Git package is: it keeps the default identity,
the saved data, the disabled state, the hotkeys and the aliases, and waits
for the same safe activation boundary. Nothing is fetched while the
newest tag names the version installed, and a tag older than it is never
followed down.

Some defaults are left alone:

- An update never re-enables a disabled default.
- One the user declined is never re-acquired (acquisition's rule).
- One whose record keeps no repository (an older Pane acquired it from
  Pane's own downloads) is skipped with that said and never updated.

### Which defaults update, and the controls

A default extension is eligible on the same terms as an unpinned npm
package: enabled, not paused, not turned off. Its `pinned` on record is
what first setup installed (the release tag this Pane release pinned),
never a choice of the user's.

The same controls govern it: the global "Update extensions
automatically" choice, and the per-extension switch on its page in
Settings and in the extension list ("a newer release of its repository
replaces it once no command of it runs").

### Refusals and failures

- A payload whose `apiVersion` this Pane cannot run is skipped. The
  refusal names the extension's version, the versions this Pane runs and
  the remedy, a newer Pane (ADR 0046), the row pointing at Pane's own
  update too when one exists.
- A release that passes its checks but fails to start pauses the
  extension with Retry and is recorded under Failed, not rolled back.
- An update that installs required dependencies installs them all or
  changes nothing.

Every default-extension check's outcome lands in the pass's
[update results](npm.md#updating-by-itself): an Updated row saying the old
and the new version, a Skipped row with why, a Failed row with what
failed.

## Updating Pane itself

Pane checks its GitHub releases for a newer version of itself when it
starts and tells the user, who alone chooses whether to download and
install it. Pane never downloads, installs or restarts itself unprompted
([decision 17](https://github.com/pane-app/pane/issues/1),
[Q38](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/current-decisions.md)).
[#54](https://github.com/pane-app/pane/issues/54) added this, and
[#55](https://github.com/pane-app/pane/issues/55) wired the macOS half.
The check, the download, the verification and the swap are
platform-independent and live in `pane-core`, ready for another system's
updater to wire to its own program.

An application update ([glossary](../CONTEXT.md)) comes from the
[release source](#the-release-source): the GitHub releases of
`pane-app/pane` ([ADR 0048](adr/0048-pane-updates-itself-from-github-releases.md)).
A release `v<version>` serves every system. Its assets are one package
per system, named `pane-<version>-<target>.zip` or `.tar.gz` (the target
is written as a [helper target](../CONTEXT.md) is: `windows-x86_64`,
`macos-aarch64`, `linux-x86_64`), each with a `<package>.sha256` beside
it, and the two installers, `Pane-<version>.dmg` and
`Pane-<version>-<arch>.AppImage`, with theirs. `cargo xtask
package-windows`, `-macos` and `-linux` write exactly those assets into
`target/dist/release/`. A Pane updates from its system's package, except a
Pane running from an AppImage, which updates from the AppImage
([ADR 0049](adr/0049-an-appimage-updates-from-the-releases-appimage.md)).
A `Pane.app` dragged out of the disk image is the bundle the zip's script
builds, so it updates from the zip.

The releases are read only where an update is checked, so a release one
Pane cannot take changes nothing else: a default extension is set up from
its repository whether the releases can be read or not.

### The check

Once, when Pane starts, Pane lists the newest releases in the background
through GitHub's API (`api.github.com/repos/pane-app/pane/releases`, ten
at a time), drafts left out and prereleases kept while every release of
Pane is a preview. It takes the newest release, by version, that carries
this system's package, and compares that version with the version it
runs: dotted numbers, compared by number. A release whose tag is not
`v` and dotted numbers is not one of Pane's versions, and is passed over.
That cadence is provisional; no interval is checked meanwhile.

The check reads only the list of releases, one API request. **Nothing is
downloaded until the user chooses**, and Pane does nothing else: no download, no install, no
restart (US76,
[Q38](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/current-decisions.md)).
An equal or older version says nothing: no downgrades are offered or
picked.

A release that carries only other systems' packages is passed over
silently. A newer package with no `.sha256` beside it, a listing Pane
cannot read, or a package whose address is not one of Pane's releases is
explained. So is a source that cannot be reached (tried three times, like
an interrupted acquisition). GitHub allows an address that does not sign
in 60 API requests an hour; when they are used up, the check says so and
when the limit lifts, and does not try again at once.

When there is a newer version, the status line says what was found
("Pane 99.0.0 is available"), and root search lists the offer after the
install rows and before Manage extensions: **Update Pane to 99.0.0**. Its
subtitle says what installing does: the user's extensions and settings
are kept, and the new version is used the next time Pane starts. A check
that failed lists **Check for a Pane update** with why, which tries
again.

### The install

Choosing the row:

1. Downloads the package's `.sha256`, then the package with progress
   ("Downloading Pane 99.0.0: 34% of 186 MiB"), with the same retries an
   interrupted acquisition gets (three attempts). Both come from
   `github.com`, which redirects to the host it serves release assets
   from; Pane follows that redirect only to GitHub's own hosts
   (`github.com`, `release-assets.githubusercontent.com`,
   `objects.githubusercontent.com`), only over HTTPS and at most five
   times. A redirect anywhere else, a package that is not there, or one
   whose bytes do not match the sha256 its `.sha256` gives, is explained
   and never retried.
2. Checks it and unpacks it, in the format the package's system packs:
   - the zip a Windows or macOS package is, read as strictly as an npm
     package's tarball: only files and folders inside the package, one
     plain name per part on every system, every entry checked against the
     central directory and the file's own header and CRC32;
   - the gzipped tarball the Linux package is, read with the npm
     tarball's own strictness: only files and folders, extension headers
     read raw, an ambiguous size refused;
   - an AppImage is not unpacked: it must be an ELF file with the
     AppImage type 2 mark, or it is refused.
3. Stages it in the install folder's `update` folder, or for an AppImage
   in a hidden `.<name>.update` folder beside it: an AppImage often sits
   in a folder of the user's own, where an `update` folder may be theirs.
4. Swaps. The running program is renamed out of its way: `pane.exe` to
   `pane.exe.old` on Windows, the bundle's `pane` to `pane.old` on macOS,
   `pane` to `pane.old` on Linux, and an AppImage to `<name>.old`. Every system allows renaming a running
   program; only overwriting one is refused. The staged program takes its
   name and place, and the staging folder goes.

**The new version is used the next time Pane starts**, the user's next
start, whenever they choose. Pane itself never restarts. A start removes
what earlier updates left: the renamed old program (best effort, since
another Pane may still run it) and a staging folder a Pane stopped
mid-install left.

Installing while Pane is being used is fine. The download runs off the
thread, so commands and services keep answering while it goes, and only
the last renames touch the program's folder, between two of the user's
actions. A command still running when the user closes Pane ends as any
command does, and the update left staged is applied (or cleaned up) by
the next start.

### What a failure leaves

A failed check or install explains itself on the status line and leaves
everything untouched: the program is still the one running, nothing is
staged, nothing is renamed. The row stays (the offer, or the check), and
the user can try again. The offer's row leaves root search while its
install runs, so it cannot be started twice, and comes back selected once
the install failed, so Enter tries again
([#424](https://github.com/pane-app/pane/issues/424)).

The old version's data is never touched. Pane's data and caches live
where each system keeps them:

- Windows: `%LOCALAPPDATA%\Pane\data` and `%LOCALAPPDATA%\Pane\cache`,
  which is the install folder too unless another was chosen.
- macOS: `~/Library/Application Support/Pane` and `~/Library/Caches/Pane`.
- Linux: `~/.local/share/pane`, which the install folder does not even
  hold.

The swap changes only the program, so extensions, their settings, pins
and enablement are exactly what they were.

### Where the user reaches it

There are two entry points: root search's rows, and the Settings window's
About page ([#82](https://github.com/pane-app/pane/issues/82)). The page
reads the same state the rows come from (`Launcher::application_update`),
so the two cannot disagree, and its "Check for updates" and "Update Pane
to <version>" rows run the same check and the same install the root rows
run.

The page also says the states the rows stay quiet about:

- that Pane is up to date (after a check the user asked for);
- that no release source is configured at all (a development build
  without `PANE_RELEASES`, whose page explains the state rather than
  promising a release);
- why installing the offer last failed, since the offer stays, ready to
  be chosen again.

A check the user asks for from either entry point answers even when there
is nothing to offer.

The same page's **Log** row and root search's **Pane quit unexpectedly
last time** row, which follows the update's rows, work the same way for
Pane's local crash record (#133,
[pausing](pausing.md#when-pane-itself-ends-its-log-and-the-crash-notice)),
and the diagnostics the page copies name the log's folder.

### On each system

On every system the install is one call in `pane`'s `main.rs` giving the
program's own path.

- Windows: the program is at `%LOCALAPPDATA%\Pane\pane.exe`, the setup's
  and the install script's target and the shortcut's, which the swap keeps
  pointing at the right file. The zip package its entry names is unpacked
  by the zip reader. See below for the setup's Apps entry.
- Linux ([#56](https://github.com/pane-app/pane/issues/56)): the program
  is at `~/.local/bin/pane`, the install script's target. The desktop
  entry the script put in `~/.local/share/applications` keeps naming it
  (the swap changes only the program, so the entry never points anywhere
  else). The tarball the Linux package is unpacks with the tar reader npm
  tarballs are read by, and Pane's data stays in `~/.local/share/pane`,
  which the install folder does not even hold.
- Linux from an AppImage ([#464](https://github.com/pane-app/pane/issues/464),
  [ADR 0049](adr/0049-an-appimage-updates-from-the-releases-appimage.md)):
  the program the system reports is inside the AppImage's read-only
  mount, so `main.rs` gives the AppImage file instead. As it starts,
  Pane reads the runtime's `APPIMAGE` and `APPDIR` (`pane_core::appimage`).
  It counts as an AppImage only when its own program is inside `APPDIR`.
  It then takes those variables, and the runtime's `ARGV0` and `OWD`, out
  of its environment, so the programs it starts do not think they run
  from Pane's AppImage. The update downloads the release's
  `Pane-<version>-<arch>.AppImage` and puts it at the same path, so the
  menu entry an integration tool made, the autostart entry (which names
  the AppImage too) and any shortcut keep starting it. The file keeps its
  name, even though the name says the old version. The AppImage's folder
  must be writable by the user. An AppImage in a folder only root can
  write fails to update, and the failure is explained.
- macOS ([#55](https://github.com/pane-app/pane/issues/55)): the program
  is at `Pane.app/Contents/MacOS/pane`, in `/Applications` or
  `~/Applications` (wherever the disk image's bundle was dragged, or the
  install script put it). See below. The bundle needs to be writable by
  the user. A copy the user dragged into `/Applications` is theirs, so it
  is. A Pane run from the mounted disk image, or from a quarantined
  download that macOS runs from a randomized read-only path (App
  Translocation), cannot replace itself. Its update fails and says the
  file system is read only, until the bundle is moved into an
  Applications folder.

On macOS the swap replaces **the binary inside the bundle**, and the
bundle itself stays. Replacing a whole `Pane.app` under a running Pane
would break it, and would take away the `Pane.app` the user sees in
Finder and Launch Services knows. The binary the bundle's
`CFBundleExecutable` already names is the one an update replaces, in
place.

The old binary is renamed `pane.old` and the staging folder is `update/`,
both beside the binary in `Contents/MacOS`, and a later start removes
them. Finder and Launch Services keep opening the same bundle, whose
program answers the new version.

The binary an update installs is one Pane wrote itself, so it carries no
Gatekeeper quarantine mark and launches as the old one did. A signed
bundle's signature would not survive a binary replaced inside it, which
is one more reason signing is a release prerequisite (recorded
[below](#limits-and-prerequisites)).

Once the new binary is in place, the swap also writes the new version
into the bundle's `Info.plist`
([#455](https://github.com/pane-app/pane/issues/455)): its
`CFBundleShortVersionString` and `CFBundleVersion` name the version the
program reports (`pane --version`), which is the one Finder's "Get Info"
shows. Only those two values change; the rest of the plist stays as the
install script wrote it. The new plist is written beside the old one and
renamed over it. If it cannot be written, the update still counts (the
program is already replaced), and the plist keeps the old version until
the bundle is reinstalled from a package.

On Windows, a Pane the setup installed is updated by the same swap of
`pane.exe` from the release's zip, not by running the new setup
([#463](https://github.com/pane-app/pane/issues/463)). Once the new
program is in place, the swap gives the setup's Apps entry the new version,
as macOS's plist is given it: `DisplayVersion`, which Settings → Apps
shows, and the `MajorVersion` and `MinorVersion` Inno Setup writes beside
it. Only an entry whose `InstallLocation` is the folder of the program
being replaced is changed, so a zip copy elsewhere leaves the setup's
entry alone, and a Pane installed from the zip or by `install.ps1` has no
entry and changes none. If the entry cannot be written, the update still
counts, and the entry keeps the old version until the next setup runs.
Everything else in the entry (where Pane is, how it is uninstalled) stays
true across a swap. The uninstaller also removes the `pane.exe.old` and
`update\` a swap may have left.

The setup is not what an update runs because running it would mean
quitting Pane and starting it again, which Pane never does by itself (the
new version is used the next time the user starts Pane), and would give
Windows a second update path that installs made from the zip could not
take. The swap already works for every install, and the entry's version is
all that the setup knows and the swap would otherwise leave stale.

## The release source

The release source serves Pane's own application updates alone: the
GitHub releases of `pane-app/pane`
([ADR 0048](adr/0048-pane-updates-itself-from-github-releases.md)).
Nothing about a default extension comes from it: the defaults are
fetched from their repositories.

Release builds read `https://api.github.com/repos/pane-app/pane/releases`
and download from `https://github.com/pane-app/pane/releases/download/`
only. **A release build has no way to replace the published releases.**

Tests and development builds can name a stand-in on this computer
instead, through `PANE_RELEASES`, so no check ever reaches the network.
It is read as [`Registry::local` in npm](npm.md#the-registry) is: an
`http://` or `https://` address on a literal loopback address
(`127.0.0.1`, any `127.x.y.z`, `[::1]`) with an optional port and path.
`localhost` and anything else is refused. The stand-in answers the same
API path and serves the downloads under the same paths, and a redirect
from it is followed only to itself.

- The smokes serve `target/dist/release` with
  [scripts/release_server.py](../scripts/release_server.py) on
  127.0.0.1.
- The tests serve their own releases in process, each download behind a
  redirect as GitHub's are (`crates/pane-core/tests/support/releases.rs`).

A development build with no `PANE_RELEASES` checks for no application
update, so a checkout runs nothing over the network by itself.

## Trying a first setup by hand

A development build fetches the default extensions from the repositories
its pins name. The committed pins point at the real repositories on
GitHub. To watch a first setup without reaching them, serve clones of the
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

A fresh data folder (`PANE_DATA_DIR=/tmp/fresh`) opens on first setup's
choice of the official extensions. Enter on Set Up shows the acquisition,
and then the calculator's answer to "6*7".

The clone step reaches GitHub once, as the smokes' own setup does; the
Pane under test fetches only from 127.0.0.1. A development build with no
`PANE_RELEASES` checks for no application update.

On Windows, do the same with `python` instead of `python3`, with the
server in another terminal or started in the background:

```powershell
python scripts/repository_server.py serve $env:TEMP\pane-repositories $env:TEMP\port
python scripts/repository_server.py clone-defaults crates/pane/defaults.json `
  $env:TEMP\pane-repositories $env:TEMP\pins.json "http://127.0.0.1:$((Get-Content $env:TEMP\port).Trim())"
$env:PANE_DATA_DIR = "$env:TEMP\fresh"
$env:PANE_DEFAULTS = "$env:TEMP\pins.json"
cargo run -p pane
```

On macOS, do the same as on Linux (`python3`, the server backgrounded
with `&`).

## Checks

- `crates/pane-core/tests/application_update.rs` runs the application
  update through the launcher's public interface, against a loopback
  stand-in for Pane's releases:
  - a newer version offered as a row in root search, with nothing
    downloaded until the user chooses it and nothing changed when they do
    not;
  - choosing it downloading the package, checking it and swapping the
    running program, with the staged outcome observable: the new program
    in place, the old one renamed away, the staging gone, the offer's row
    gone, and a later start removing what the update left;
  - the Linux package's tarball installing through the same path as the
    zip the Windows one is (the suite runs on every system, so both
    formats are installed wherever the tests run);
  - a damaged package, an unreachable source, a source that answers an
    error and a replacement that cannot be made, each explained with
    everything untouched and the row ready to try again, and the retry
    that installs once the source works;
  - every download going through a redirect, as GitHub's do, and a
    redirect away from the releases refused and not retried;
  - a used-up rate limit explained with when it lifts, and not retried at
    once; a release without its `.sha256` explained and not offered;
  - a release that carries another system's package, or an older
    version, and a listing Pane cannot read, explained without stopping a
    default extension being set up from its repository;
  - progress on the status line while the core stays usable;
  - Pane's data (an extension set up at first setup, its record and
    cache) untouched by an install, the extension still installed and
    answering after the update.
- `pane-core`'s own unit tests check the choice of release (the newest
  that carries this system's package, by version, a `-dev` package never
  taken), the hosts a download may be redirected to, the listing as
  GitHub's API writes it, and the rate limit's explanation; and the zip
  Pane unpacks (both storage methods, and every refusal). The npm tarball reader's own unit tests
  check the tarball the Linux package is.
- `crates/pane-core/tests/installer.rs` runs the acquisition through the
  launcher's public interface, over repositories served on 127.0.0.1 by
  Git's smart HTTP protocol (`support/repo_server.rs`). It uses a default
  set of its own: the icons sample and the helper sample, a revision
  carrying a real helper program. It checks:
  - a first setup installing both as managed copies with the default
    identity and the Git source recorded in `installed.json`;
  - what is being set up said on the status line while the core stays
    usable;
  - an interrupted fetch recovered by retry;
  - an unreachable repository leaving the core usable with the row that
    tries again, and the row setting the extension up once the repository
    works;
  - the helper running from the managed copy with mode 0755;
  - a revision without a `pane.json`, a source-only revision, an
    incompatible package and one without its declared helper file,
    explained and not installed;
  - a restart fetching nothing;
  - a default with retained data (one the user uninstalled) not
    re-acquired, and a disabled one not either;
  - a default extension that left the build's default set (the helper
    sample, #162) kept installed, fetched for nothing and uninstallable,
    and not brought back once uninstalled;
  - an install that acquired a default from the artifact source (an
    older Pane) keeping it, its identity and data unchanged.

  These run on every system, so the Windows and macOS acquisitions need
  no test of their own: it is the same code. The one Windows-only piece
  is the `.exe` helper-name rule, checked by the runner's unit tests.
- `crates/pane-core/tests/first_setup.rs` runs first setup's choice (#283)
  through the launcher's public interface, over the same loopback
  repositories:
  - the official extensions listed by their pins' titles and
    descriptions, every default ticked and Set Up selected, nothing
    fetched before the choice;
  - an unticked default not fetched and recorded as declined, the choice
    holding across a restart;
  - a default left out offered by Settings' Install operation and
    installed from it, no longer declined;
  - an uninstalled default declined whether its data was kept or not;
  - without a connection, the failure explained and each default offered
    again as a row, with nothing asked again at the next start;
  - leaving the screen choosing nothing, with root search's row opening
    it again;
  - every default unticked setting up none;
  - an install set up before the choice existed, or a build with no
    defaults, never asked.
- `crates/pane/tests/first_setup.rs` draws the screen in the native
  window: the rows with their descriptions and check marks, Space, the row
  and the check mark ticking, and Enter on Set Up setting up what stays
  ticked.
- `crates/pane/tests/settings.rs` installs a default left out from the
  Extensions group's Official Extensions section.
- `crates/pane-core/tests/update.rs` runs the default extensions' updates
  through the launcher's public interface, over the same loopback
  repositories. Every repository is served on 127.0.0.1, so nothing
  reaches a real one. It checks:
  - a newer release tag updating a default by itself with its identity,
    data, disabled state, hotkeys and aliases kept (and a newest tag that
    names the version installed fetching nothing);
  - one disabled, turned off or uninstalled-with-kept-data not updated
    automatically, and a pass the user asked for updating the turned-off,
    disabled and paused ones (a disabled one stays disabled, a paused one
    is unpaused);
  - a payload needing a newer Pane skipped with that reason;
  - a new version that fails to start paused with Retry and recorded
    under Failed, not rolled back;
  - an update whose required dependency cannot be installed changing
    nothing;
  - the per-extension switch and the global choice covering defaults;
  - an unreachable repository recorded and leaving the installed copy
    alone.
- The [Linux smoke](platforms/linux.md#installing-pane-and-acquiring-its-calculator-53),
  the [Windows smoke](platforms/windows.md#installing-pane-and-acquiring-its-calculator-51)
  and the [macOS smoke](platforms/macos.md#installing-pane-and-acquiring-its-calculator-52)
  build the package and install it on a clean machine: a fresh home
  folder on Linux and macOS, a fresh user profile on Windows. The
  installed Pane, started with a PATH that holds nothing at all, fetches
  the default extensions' pinned commits from their repositories (the
  five that run everywhere on Linux and macOS, all eight on Windows) and
  answers "6*7" with 42. The smoke's own setup clones the repositories at
  those commits and serves them on 127.0.0.1, so the Pane under test
  reaches no network address.

  A helper running from an acquired revision is `installer.rs`'s, above;
  the smokes run the helper sample installed with `--install`. The
  install script itself runs with `/usr/bin:/bin` on Linux and macOS, so
  the fresh home stays clean while the script's tools resolve; on Windows
  it runs with the empty PATH, its PowerShell script needing nothing from
  one. What is checked, and how, is each platform page's own record.
- `xtask`'s packaging is checked by building it. `cargo xtask
  package-linux [--dev]` must produce the tarball, its sha256 and the
  release assets; `cargo xtask package-windows [--dev]` and
  `cargo xtask package-macos [--dev]` the zip, its sha256 and the same
  assets. The release-profile package is built the same way as the
  development-profile one the smoke installs.
- The Package job (`.github/workflows/package.yml`, which the release
  matrix and the release workflow both run) checks each installer the way
  a user meets it, right after building it
  (`scripts/check-installer.sh`):
  - On `macos-15`, the disk image's digest is checked, and the image is
    mounted read-only. It must hold the link to `/Applications` and a
    whole `Pane.app`, whose `Info.plist` lints and names the version and
    `pane`. The bundle is copied with `ditto` into an Applications folder
    of the check's own, as a drag in Finder copies it, and must answer
    `--version` with the version.
  - On `ubuntu-24.04`, the AppImage's digest and its type 2 mark are
    checked. It must answer `--version` run extracted
    (`--appimage-extract-and-run`, since a runner may have no FUSE), and
    mounted too where FUSE is there. Extracted, it must hold `AppRun`
    linking its program, the desktop entry (`Exec=pane`, `Icon=pane`,
    `StartupWMClass=pane`, passing `desktop-file-validate`), `.DirIcon`,
    every icon size, and the licence and notices.
- `pane-core`'s unit tests check the AppImage's own choice of release:
  the AppImage asset, passing over a release that carries only the
  tarball. They check the swap that puts the new AppImage at the old
  one's path, refusing a download that is not an AppImage and leaving the
  user's own `update` folder alone, and the runtime variables that make a
  Pane count as running from an AppImage.
  `crates/pane-core/tests/application_update.rs` installs an AppImage
  update through the launcher on Linux, with the tarball beside it never
  downloaded.
- `xtask/src/zip.rs` has a unit test that pins the zip's bytes, first
  decoded with Python's `zipfile`. The Linux machine that writes most of
  this repository cannot build `pane.exe` or the macOS `pane` program, so
  there the Windows and macOS tasks explain that only a checkout of their
  own system builds the package.
- The Windows setup is checked on CI's `windows-2025` runner, in the
  Package job that builds it (`package.yml`, which the release matrix and
  the release workflow both run), by `scripts/check-windows-setup.ps1`:
  a silent install into a scratch folder with the sign-in task ticked,
  then `pane.exe --version`, the Start menu shortcut's target, the Apps
  entry's name, publisher, version, icon and install folder, and the
  launch-at-login registration and choice the started Pane makes; a
  second silent install over that running Pane, which must close it and
  keep the data, the choice and the registration; and a silent uninstall
  from under a Pane started again, after which the program, the shortcut,
  the entry and the registration must be gone and a file in
  `%LOCALAPPDATA%\Pane\data` kept. It changes the user's Start menu, Apps
  list and sign-in list, so it runs on CI only. pane-core's
  `application_update` tests check, on Windows, that an update gives a
  scratch stand-in for the entry the new version only when it names the
  program's folder, and that the key it names is the one the setup's
  AppId makes; `pane`'s settings tests check that `--launch-at-login`'s
  choice is the General page switch's.

## Limits and prerequisites

### Not released yet, and not signed

- No release of Pane is published yet, so a Pane installed from today's
  package finds no newer version and offers none. A development build
  checks only where `PANE_RELEASES` names a stand-in. First setup is
  unaffected: it fetches the default extensions' pinned commits from
  their repositories. Publishing a release is an execution step, like
  the signing credentials below: pushing a `v<version>` tag runs the
  release workflow (`.github/workflows/release.yml`), which drafts a
  prerelease with the three packages, the disk image and the AppImage,
  their `.sha256` files, the source archive and the notes, and a person
  publishes the draft.
- Nothing is signed. No signing credentials exist, so the package is not
  signed: its `.sha256` says only what was packed, and the application
  package an update downloads is checked only against the sha256 of the
  `.sha256` from the same release, over HTTPS from GitHub. (The
  connection trusts the system's certificates.) That guards against a
  damaged download, not against a release someone who controls the
  repository published. Signing the package is a prerequisite for a
  stable release.
- On Windows no Authenticode certificate exists either, so `pane.exe`, the
  setup and `install.ps1` are unsigned. Windows may warn about an unknown
  publisher when the setup or the program runs (SmartScreen's "More info",
  then "Run anyway"), and PowerShell may refuse the install script as
  one that came over the internet (the README and the
  [Windows section](#installing-on-windows-from-the-zip) above say how a user answers
  that for this one script). Acquiring a certificate, and signing the
  program and the script with it, are execution prerequisites like the
  others here.
- On macOS there is no Apple Developer ID certificate either, so `pane`,
  `install.sh` and the disk image are unsigned and nothing is notarized
  (the program carries only the ad-hoc signature the linker gives every
  Apple silicon program). Gatekeeper
  blocks the first launch of a Pane.app a browser downloaded (the README
  and the [macOS section](#installing-on-macos) above say how the user
  answers that), while a package built on the machine runs at once.
  Acquiring the certificate, signing and notarizing are prerequisites for
  a release, like the others here.

### Builds and baselines

- The release assets this build writes are for the system it ran on
  (`linux-x86_64`, `windows-x86_64` and `macos-aarch64` on CI,
  `linux-aarch64` on an arm64 checkout). A Pane takes only the package for
  its own target. A release must carry one package per supported target,
  each built on its own system, as the release workflow does.
- One package per system, built where it runs. Cross-building, and
  packaging for a system this task cannot build on, are out of scope (the
  evidence for each is per-system, as the specification requires). On
  Windows and macOS the task says so and stops rather than packing
  another system's program; on Linux the task builds whatever program the
  checkout builds (#53's behavior, kept).
- The Windows baseline is one system. `windows-2025` (Windows Server
  2025, x86_64) is the declared baseline, the system CI builds, packages,
  installs and smokes on. No Windows 10 or 11 client edition, no ARM64 and
  no per-user install on a real desktop has been tried, and the smoke's
  clean machine is a fresh profile on that runner, not a fresh machine.
  The install script and the smoke phase were written without PowerShell
  on the machine that wrote them, so their runtime evidence is CI's
  Windows leg (recorded in
  [platforms/windows.md](platforms/windows.md#installing-pane-and-acquiring-its-calculator-51)).
- The macOS baseline is one system. `macos-15` (macOS 15, arm64) is the
  declared baseline, the system CI builds, packages, installs and smokes
  on. No Intel Mac, no other macOS version and no install on a Mac of a
  user's own has been tried, and the smoke's clean machine is a fresh
  home folder on that runner, not a fresh machine. The install script and
  the smoke phase were written without a Mac on the machine that wrote
  them (the script was linted and dry-run with a fake program instead),
  so their runtime evidence is CI's macOS leg (recorded in
  [platforms/macos.md](platforms/macos.md#installing-pane-and-acquiring-its-calculator-52)).

### Updates

- The update's cadence is provisional. Pane checks when it starts and at
  no interval; how often a running Pane rechecks (and whether a check
  that failed retries quietly) is a choice recorded for the user.
  Extension updates and application updates stay separate, as the
  specification requires. Extension updates have their own controls (npm
  packages update automatically since #49, tracked Git branches since
  #50); the application update has none, only the user's choice, every
  time.
- Every system's wiring exists. The check, download, verification and
  swap are platform-independent `pane-core` code, and the Windows, macOS
  and Linux builds each wire them to their own program. The swap is
  exercised by the tests wherever they run. The running-program rename it
  depends on is proven on Windows itself by the smoke, and the macOS and
  Linux smokes prove their own when they run (pending their CI, as their
  [platform pages](platforms/macos.md#installing-a-pane-application-update-by-the-users-choice-55)
  record).
- Nothing about an update is signed either: a package is checked only
  against the sha256 its release's `.sha256` gives, over HTTPS. An application package is at most
  512 MiB packed and 2 GiB unpacked (the program is large; a development
  build of it much more). It is not cached between attempts: an
  interrupted download starts over, and only the retries within one
  install attempt keep it cheap.
- A default extension's repository must tag its releases. Its updates
  read the repository's `v<semver>` release tags
  ([#269](https://github.com/pane-app/pane/issues/269), ADR 0044). A
  repository that never tags a newer release (one that only moves its
  default branch) never updates the extension, and neither does a tag
  that names the version installed. The extension's author chooses when
  to release; a Pane release moves its pins when it tests one.

### Concurrent Panes

- On one install folder, both Panes may check and offer. Two installs
  race by failing honestly (the swap's renames cannot both happen), and a
  Pane starting removes a staging folder another Pane may be installing
  from. These are the same small warts the shared data folder already
  records, left as they are.
- On one data folder, both acquire. Each fetches its revision into a
  download folder of its own under `downloads/`, removed once its package
  is dropped. One that installs the same default extension while the
  other is installing it is told the install's own wording, "Already
  installed …; use Update to replace the installed copy", wording a
  default extension has no row for (a restarted Pane lists it). That is a
  small wart of the shared install path, left as it is.

## Tracking

Linux packaging is [#53](https://github.com/pane-app/pane/issues/53)
(US15, US16, US18, US42; T23; contributions to G6, not a claim that it
passes), Windows is [#51](https://github.com/pane-app/pane/issues/51) and
macOS is [#52](https://github.com/pane-app/pane/issues/52). The default
extensions are [#60](https://github.com/pane-app/pane/issues/60)'s five,
fetched from their own repositories since
[#278](https://github.com/pane-app/pane/issues/278). Pane's own updates
are [#54](https://github.com/pane-app/pane/issues/54),
[#55](https://github.com/pane-app/pane/issues/55) (macOS) and
[#56](https://github.com/pane-app/pane/issues/56) (Linux). The disk image
and the AppImage, and the AppImage's own updates, are
[#464](https://github.com/pane-app/pane/issues/464).
