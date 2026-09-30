# Pane's packages and first setup

Added for [#53](https://github.com/hoangvu12/pane/issues/53) (Linux; US15,
US16, US18, US42; T23; contributions to G6, not a claim that it passes),
[#51](https://github.com/hoangvu12/pane/issues/51) (Windows) and
[#52](https://github.com/hoangvu12/pane/issues/52) (macOS): a clean
machine installs Pane from one package, and Pane acquires its default
extensions itself over the network — the calculator, the feature this
proves — with progress, retries and a cache, while the core (the window,
root search, the install rows, Manage extensions) stays usable. This is
the internet-first setup the specification chose
([decision 20](../launcher-design-interview.md)); the installer carries no
payloads and installs no runtime, and the user installs no Node, Rust,
npm, Git or compiler: Pane's extension runtime is part of Pane's own
process (Wasmtime), so nothing is acquired for it.

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
  build takes its artifact source from `PANE_ARTIFACTS` (a release build
  uses Pane's published downloads, which no controlled source may
  replace).
- **`pane-<version>-windows-<arch>.zip`** — the Windows package
  (`cargo xtask package-windows`): the `pane.exe` program (release
  profile), the PowerShell install script and a `README.txt`, under
  `pane/`. A zip, because a Windows user unzips with whatever is at hand
  and Windows has no tar of its own a user can rely on; `--dev` names the
  development profile's package the same way. The task assembles the
  artifacts anywhere, but builds the package only on Windows: `pane.exe`
  needs a Windows checkout (no cross toolchain is set up), so anywhere
  else it explains so and stops — the one thing a Windows build alone
  provides.
- **`artifacts/`** — what an artifact source serves (below): the index
  `pane-defaults.json` and one tarball per default extension's payload.
  A real deployment serves this folder at Pane's published downloads; the
  tests and smokes serve it from this computer instead. The payloads are
  built for the system the task ran on, so each system's run of its own
  task serves its own (`windows-x86_64`'s helper file, for instance, from
  the Windows task's artifacts).
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

The package holds no default extension: first setup downloads them, so
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
`installed.json`, with `"defaultVersion"`), whatever version is
installed. A development build of Pane acquires the calculator and, with
it, the helper sample (`helper-sample`), so a payload carrying a native
helper is acquired and its prebuilt helper runs with no developer tool; a
release build acquires the calculator alone — which default extensions a
release ships is a product choice as each lands its slice.

At first setup, and whenever a default extension is missing, Pane
acquires each in turn in the background:

1. **The index.** `GET https://downloads.pane.sh/pane-defaults.json`:
   `formatVersion` (this Pane reads 1) and one entry per default
   extension: its `id`, `version`, `file` (a plain name, so the address
   stays on the source), `integrity` (`sha512-<base64>`, which the
   payload's bytes must match) and `size` (for progress). An index with
   another format version, an entry without a sha512, or a duplicate id
   is explained, and so is an entry Pane's build does not ask for.
2. **The payload.** `GET <source>/<file>`, through Pane's own HTTP client
   (hyper, rustls, the system's certificates, one connection per request,
   no proxy, no redirect — the one npm and Git packages use), at most
   64 MiB, with the bytes so far reported as they arrive. An interrupted
   download — the connection closing partway, or a server error (403,
   500, 502, 503, 504) — is tried again, up to three times, after 0.5 s
   and 1 s; a payload that is simply not there, or whose bytes do not
   match the integrity the index gives, is explained, not retried.
3. **The cache.** The downloaded payload is kept under
   `extensions/acquired/<id>/<version>-<integrity's first 16 hex
   digits>.tgz` (written through a `.part` file, so a Pane stopped
   mid-download leaves no half-written payload under the name a later one
   looks for; one a day old is removed as abandoned, as a young one may
   belong to another Pane on the same data folder). Acquiring again finds
   it there and reuses it — only if its bytes still match the integrity
   its index gives: a damaged entry is downloaded again and replaces it,
   and another version of the same default extension removes the older
   one's entry. Nothing else of the payload is kept: what is installed is
   the managed copy.
4. **The install.** The payload is a gzipped tar of a `package/` folder
   holding the extension package, unpacked with the same checks an npm
   package's tarball gets (files and folders only, every path inside,
   the same limits), then read, checked, planned and installed exactly as
   a package from a folder is — into a managed copy, with the default
   extension's identity. Its compatibility is what any package's is: the
   manifest version and extension API this Pane reads, the platforms it
   declares, the components it names present, and, where it ships
   helpers, the file for this system a real program for it. A default
   extension the user uninstalled, whose data Pane keeps, is not acquired
   again — the user's choice, with disabling the documented opt-out; one
   never installed is.

### What the user sees

The status line says what is happening — "Acquiring the Calculator…" then
"Acquiring the Calculator: 34% of 116 KiB" — while the window, root
search, the install rows and Manage extensions stay usable: acquisition
never blocks anything. When every default extension is set up, the status
line says "Set up the Calculator" (or "Set up Pane's default extensions").
A default extension that could not be acquired is explained there ("Could
not set up the Calculator: Pane's downloads at … could not be reached:
… (Pane tried 3 times)") and offered as a row in root search after the
install rows, **Set up Calculator**, which tries again; the row goes once
what it asked for is there. Starting Pane tries again by itself, so a
Pane stopped mid-setup recovers, and disabling a default extension (in
Manage extensions) is the opt-out: a disabled default extension is
installed, so it is never re-acquired or re-enabled.

## The artifact source

Release builds acquire from `https://downloads.pane.sh/` only. Tests and
development builds can name a source on this computer instead
(`PANE_ARTIFACTS`, read as [`Registry::local` in
npm](npm.md#the-registry) is: an `http://` or `https://` address on a
literal loopback address — `127.0.0.1`, any `127.x.y.z`, `[::1]` — with an
optional port and path; `localhost` and anything else is refused), so no
check ever reaches the network. **A release build has no way to replace
the published source.** The smokes serve `target/dist/artifacts` with
[scripts/artifact_server.py](../scripts/artifact_server.py) on 127.0.0.1;
the tests serve their own payloads in process
(`crates/pane-core/tests/support/artifacts.rs`).

A development build with no `PANE_ARTIFACTS` acquires nothing, so a
checkout runs nothing over the network by itself.

## Trying a first setup by hand

```sh
cargo xtask package-linux --dev
python3 scripts/artifact_server.py target/dist/artifacts /tmp/port &
PANE_ARTIFACTS=http://127.0.0.1:$(cat /tmp/port)/ cargo run -p pane
```

A fresh data folder (`PANE_DATA_DIR=/tmp/fresh`) shows the acquisition
and the calculator's answer to "6*7".

On Windows, the same with the Windows package (built where it can be):

```powershell
cargo xtask package-windows --dev
python scripts/artifact_server.py target/dist/artifacts $env:TEMP\port
$env:PANE_ARTIFACTS = "http://127.0.0.1:$((Get-Content $env:TEMP\port).Trim())/"
cargo run -p pane
```

On macOS, the same with the macOS package (built where it can be):

```sh
cargo xtask package-macos --dev
python3 scripts/artifact_server.py target/dist/artifacts /tmp/port &
PANE_ARTIFACTS=http://127.0.0.1:$(cat /tmp/port)/ cargo run -p pane
```

## Checks

- `crates/pane-core/tests/installer.rs`: the acquisition through the
  launcher's public interface — a first setup installing the calculator
  and the helper sample as managed copies with the default identity and
  the record in `installed.json`; progress on the status line while the
  core stays usable; an interrupted download recovered by retry; the
  cache reused, a damaged entry replaced; an unreachable source leaving
  the core usable with the row that tries again, and the row setting the
  extension up once the source works; the helper running from the managed
  copy with mode 0755; a payload that does not match its integrity, and
  one for another platform, explained and not installed; a restart
  fetching nothing; a disabled default extension not re-acquired. These
  run on every system, so the Windows and macOS acquisitions need no
  test of their own: it is the same code (the one Windows-only piece is
  the `.exe` helper-name rule, checked by the runner's unit tests).
- The [Linux smoke](platforms/linux.md#installing-pane-and-acquiring-its-calculator-53),
  the [Windows smoke](platforms/windows.md#installing-pane-and-acquiring-its-calculator-51)
  and the [macOS smoke](platforms/macos.md#installing-pane-and-acquiring-its-calculator-52):
  the package is built and installed on a clean machine — a fresh home
  folder on Linux and macOS, a fresh user profile on Windows — and the
  installed Pane, started with a PATH that holds nothing at all, acquires
  both payloads from the controlled source and answers "6*7" with 42,
  its helper echoing for this system. (The install script itself runs
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
  exist, so a Pane installed from today's package cannot complete its
  first setup on the real internet: it explains that Pane's downloads
  cannot be reached, keeps everything else working and offers the retry.
  Deploying the source (serving `artifacts/` above) is an execution
  prerequisite, like the signing credentials below.
- **Nothing is signed.** No signing credentials exist, so the package and
  its payloads are not signed: the package's `.sha256` says only what was
  packed, and a payload is checked only against the sha512 its index
  gives, over HTTPS. (The connection trusts the system's certificates.)
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
  The helper sample's payload declares the one helper target its file was
  built for (`linux-x86_64`, `windows-x86_64` and `macos-aarch64` on CI,
  `linux-aarch64` on an arm64 checkout; the sample's manifest names them
  all, and the packaging rewrites it to the target whose file the build
  assembled); a real deployment must build every supported target and
  serve one payload whose manifest names them all, or serve one payload
  per system at a per-system index.
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
- **No default-extension updates.** A default extension is installed once
  and left alone: a Pane whose default is installed acquires nothing, so
  a newer payload version is not fetched (uninstalling and restarting
  acquires the new one). Automatic extension updates are the
  [extension update](current-decisions.md) direction, tracked for npm and
  Git sources separately; how a default extension's updates arrive
  (through the same controls) is an open choice recorded for the user.
- **Concurrent Panes** on one data folder both acquire; each writes its
  own payload through a `.part` file, and a torn entry fails its
  integrity check and is downloaded again. One that installs the same
  default extension while the other is installing it is told the install's
  own wording, "Already installed …; use Update to replace the installed
  copy" — wording a default extension has no row for (a restarted Pane
  lists it): a small wart of the shared install path, left as it is.
