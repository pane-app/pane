# Extension packages from npm

Added for [#45](https://github.com/pane-app/pane/issues/45) (US16–US18,
US20–US22, T11, T12, G4, G6; contributions), following
[ADR 0012](adr/0012-pi-style-source-identity.md)'s npm identity and the
proposed [ADR 0019](adr/0019-pane-downloads-npm-packages-itself.md). A Pane
extension can be published to npm as an ordinary npm package holding its
`pane.json` and built components. Pane downloads and installs it itself:
the user needs no Node.js, npm or compiler, and nothing in the package runs
to install it.

## Installing one

- **Install extension from npm…**, root search's row before **Install
  extension from Git…** ([Git](git.md)),
  opens a form of Pane's own with one field, the npm package: its name
  (`@pane-samples/greeter`, `greeter`) and optionally an exact version
  (`greeter@1.2.3`). **Show package** downloads it and shows the preview;
  Back returns to root search, discarding a download in progress.
- `pane --install npm:<name>[@<version>]` opens the same preview, as
  `pane --install <folder>` does for a folder.

The preview is the one a folder has ([packaging](../guests/README.md#packaging-and-installing-a-local-extension)),
with lines of its own:

- "Source: npm package @pane-samples/greeter": the identity.
- "npm version: 0.1.0, the latest", "npm version: 1.2.3, the version you
  named: installing pins it to that version", or, for a package installed
  pinned, "npm version: 1.2.3, the version it is pinned to: name another
  version to change it".
- "Downloaded: <tarball address>, matching its sha512 integrity from the
  registry".
- "Runs only the WebAssembly components its pane.json names, in Pane: no
  Node.js, npm install scripts or npm dependencies", and, when its
  `package.json` declares them, "Not used: its `postinstall` script and its
  npm dependencies, which its package.json declares; Pane never runs or
  installs them".

Install unpacks and installs the package as a local package is installed
(only `pane.json`, the components it names and this system's helper files
are copied into Pane), while it keeps its npm identity, and records it in
`installed.json` as `"npm": "<name>", "npmVersion": "<version>"`, with
`"pinned": true` when a version was named. Its commands then run like any
other.

Each download is unpacked into a folder of its own under the data folder's
`extensions/downloads/` (which Git packages share since #46,
`crates/pane-core/src/downloads.rs`), named `<seconds>-<process>-<count>` by when it was
begun, and removed as soon as nothing reads it: once the preview is shown
(the downloads of the npm dependencies it planned too; installing downloads
again), once an install ends, and on every failure, a component failing its
check included. A starting Pane removes only downloads begun more than a day
ago, left by a Pane that stopped, so another Pane's install in progress on
the same data folder keeps its files.

## Identity, versions and updates

The identity is the npm name without version (`npm:@pane-samples/greeter`
as a key): an npm copy and a local folder with the same code are two
packages, and every version of one name is one package.

- **A second install is refused**: `Launcher::install_npm` of an installed
  name, whatever the version named, answers "Already installed from npm
  package @pane-samples/greeter; use Update to replace the installed copy".
- **Choosing it again offers Update**: the preview says "Installed: npm
  version 0.1.0 of this package" (", pinned" when it is) and its row is
  **Update**, "Replace the installed copy with npm version 0.2.0, pinned" or
  "…, the latest". An update keeps the identity, its data, whether it is
  disabled, its hotkeys and aliases, as a folder's update does.
- **Pins**: naming a version installs exactly it and records it as pinned.
  Updating a pinned package without naming a version keeps its pin (the
  preview offers the pinned version again); naming another version changes
  the pin. There is no way to unpin other than uninstalling and installing
  without a version. An automatic update never touches a pinned copy
  ([below](#updating-by-itself)); the pin is recorded for them.
- A version is exact (`1.2.3`, `1.2.3-beta.1`, `1.2.3+build`); ranges
  (`^1.0.0`) and other tags (`next`) are refused before anything is asked of
  the registry. There is no picker of earlier versions (Q32).
- An npm package has no **Reload** or **Develop** row in Settings › Extensions:
  it has no source folder. Its **Retry** row (after a failure to start),
  disable, clear cache, uninstall and retained data work as for a folder;
  kept data stays with the npm name and installing it again, at any
  version, finds it.

## Updating by itself

An eligible npm package updates by itself (what makes one eligible is
[below](#which-packages-update)): Pane's updater, a background thread of the
launcher's, checks the registry and replaces the managed copy with a
compatible newer version, at a safe moment, without the user asking
(US72–US75; #49). The same updater, cadence, controls and safe boundary
update a tracked Git package from the newer commit of its branch
([git](git.md#updating-by-itself); #50).

**When it checks.** Once a minute after Pane starts (so that the check
never competes with Pane's own start) and then every 24
hours while Pane runs, by the launcher's clock
([`Launcher::with_clock`](../crates/pane-core/src/launcher.rs), the
system's in release builds; tests move the clock). A check reads only the
registry's metadata
for each eligible package's name: nothing is downloaded while the latest
version is the installed one. Both the cadence and the initial wait are
provisional, as the spec leaves the delivery timing open. The user can
also ask for a check at once — root search's **Check for Extension
Updates** command, or the **Check for updates** button on the Settings
Extensions page, which says when Pane last checked under it — whatever
the cadence (#267). A pass the user asked for looks wider than the
automatic one: every installed package from a source Pane updates,
whether or not its automatic updates are turned off, and disabled and
paused ones too (the user asked; an update keeps a disabled one disabled,
and unpauses a paused one, as the preview's Update row does). A toast
follows it — "Checking for extension updates…", "Updating 2 of 3…" —
ending as its summary ("Updated 3 extensions", "Extensions are up to
date", "Updated 2, 1 failed") with View Details, and its checks run a
few packages at a time, each bounded by Pane's HTTP limits as ever.

**What a newer version goes through.** Exactly what an install does:
downloading and checking the tarball (its integrity and everything the
refusals below cover), reading the `pane.json` (the manifest, the
extension API it needs, its platforms and its helpers for this system)
and working out its dependencies as a plan. A newer version that cannot
be taken is skipped in the pass's update results with that reason — "Incompatible
package: it needs Pane extension API 0.2, but this Pane
provides 0.1. It keeps running its installed code." — and the installed
copy is left exactly as it is; the next check tries it again. What was
downloaded stays staged, holding its download folder, until it is
applied.

**The safe boundary.** A staged update applies when the package is
quiet: no screen of one of its commands is on display (a command, its
search, a form or a custom view — a call the user is waiting on runs
with one open, and the user may be reading its answer), and no call of
it the user asked for is still running (opening a command, running an
item, a query sent from root, a command's search, a form submission).
The running command always finishes first: while one is running, or its
screen is open, the update waits and is tried again every second, listed
in the pass's update results as "Waiting until <title> is not in use"
until it applies.
Managed background work is not waited for: a replacement ends it with
the package's generation and the new code starts it again, exactly as a
reload does ([generations](generations.md)). In the moment between the
boundary check and the replacement itself, opening one of the package's
commands is refused with "Settings from npm is updating; open it again
once that is done" — only then, and only for the update Pane applies by
itself; an update the user chose replaces anyway, as a reload does. The
retry timing, and not waiting for the calls other packages make into
the updated one (they answer that it was updated and may be made again),
are provisional choices.

**What an update is.** The same update the preview's **Update** row
makes: the identity, the saved data (settings, content, credentials),
the disabled state, the hotkeys and the aliases are kept; the old code's
generation ends, stopping what is still pending of it; and the new code
runs from the next call, as an update starts no code itself. The outcome
is the pass's update results ("0.1.0 → 0.2.0" in the view): a
successful background update is quiet — the list shows the new version —
while a failure is announced once, the next time the launcher is shown,
with the toast that opens the results.

### Which packages update

Eligible is an installed npm package that is
not pinned (a pinned version stays whatever the latest is), enabled,
not paused after a failure, and not turned off; the latest version can
be older than the installed one (an author retagging `latest`), and Pane
follows it, as an unpinned copy tracks the tag. A local folder's or a
development copy's code is never replaced here, and neither is a
disabled or paused one — updating a paused package would unpause it,
which is the user's choice to make (Retry); a disabled one is the user
switched off, and the code it does not run does not change under it.
These last two are provisional choices. A default extension is eligible
on the same terms — enabled, not paused, not turned off — updating from
its repository's newer release tags
([installer](installer.md#updating-the-default-extensions)).

**The controls.** Settings › Extensions ends with **Update extensions
automatically** ("On · every eligible extension updates by itself, at
its source's newer version" / "Off · no extension updates by itself;
choose Update on a package's preview"), and each installed npm package
that is not pinned has **Update Settings from npm automatically** after
its reload rows would be ("On · a compatible newer npm version replaces
it once no command of it runs" / "Off · replace it yourself with Update
on its preview"). Choosing a row turns it on or off at once; turning
updates on checks at once. A per-package choice cannot turn updates back
on while the global one is off. The choices are recorded in
`extensions/updates.json` beside `installed.json`, as the hotkeys and
aliases are; a record Pane cannot read means the defaults (on), and the
next choice the user makes writes it anew.

## Dependencies from npm

A `pane.json` dependency's `source` may be `npm:<name>` or
`npm:<name>@<version>` ([dependencies](dependencies.md#declaring)), in a
local or an npm package. The plan, claims and rollback are #42's:

- A missing required npm dependency is downloaded while the preview is
  worked out, listed as "Requires: Greeter from npm, installed with it from
  npm:@pane-samples/greeter" and installed first; with a version in its
  source, that version, pinned.
- An installed one is used as it is, whatever the latest is: installing
  another package never updates it and never downloads it again. A disabled
  or paused one stays so. But a source naming a version must get that
  version: an installed copy of another version is a conflict ("Nothing was
  installed: Caller requires Greeter from npm at npm version 0.2.0, and
  version 0.1.0 is installed; Pane does not replace the installed copy while
  installing another extension: update it to 0.2.0 (npm package
  @pane-samples/greeter@0.2.0) if Caller needs that version").
- There is one copy of each package, so two dependents pinning different
  versions of it conflict ("… Middle requires Greeter from npm at npm version
  0.2.0, and Caller requires version 0.1.0; Pane installs one copy of each
  package, so they cannot both have theirs"), as does a pin of another
  version than the latest one a dependent without a pin found first ("…, and
  Caller takes its latest, version 0.1.0; …").
- One that cannot be downloaded or installed stops the install before
  anything changes: "Nothing was installed: Caller requires `greeter` from
  npm package nobody, which cannot be installed: npm package nobody was not
  found in the registry …".
- Its record is `{ "id": "greeter", "npm": "<name>" }`, and a call by the
  dependency id reaches the installed package with that name.
- A package from npm cannot name a `local:` folder, which is on its
  author's computer: "… comes from npm but names the local folder
  `local:../helper` as its dependency `helper`; a package published to npm
  or Git can depend only on packages from npm or Git". Since #46 it can
  name a `git:` source ([Git](git.md#dependencies-from-git)).
- An operation call by identity takes `npm:<name>` (without a version) as
  it takes `local:<folder>`.
- One whose component imports `wasi:http` is recorded, and listed, as using
  the network ([command search](command-search.md)), as the package itself
  would be.

## What is refused

Each is explained on the preview, which then offers nothing, and nothing is
installed or left unpacked:

| Case | Status |
| --- | --- |
| No such package or version | "npm package nobody was not found in the registry https://registry.npmjs.org/", "… has no version 9.9.9; its latest is 0.1.0" |
| The registry cannot be reached | "Could not reach the npm registry … for <name>: <reason>" |
| No sha512 integrity (only a `shasum`) | "… has no sha512 integrity in the registry, which Pane needs to check its download" |
| A redirect | Pane follows none, so an answer sending it elsewhere is "The npm registry … answered 302 for …" |
| The tarball is elsewhere | "Pane does not download <name>@<version>: its tarball address … is not on the registry …: Pane downloads a package only from the registry that describes it, over HTTPS" |
| The download does not match | "The download of npm package <name>@<version> does not match the sha512 integrity the registry gives (it is sha512-…); nothing was installed" |
| Too large | more than 16 MiB of metadata, a 64 MiB tarball, 256 MiB unpacked or 10,000 entries (extension headers count), or an extension header larger than 64 KiB |
| An unsafe entry | "… cannot be unpacked safely: its tarball contains a symbolic link, `package/x`; Pane unpacks only files and folders" (also hard links, long link names, devices and named pipes), or "… contains `package/../../x`, which climbs out with `..`; Pane unpacks only paths inside the package" (also absolute paths, empty or `.` parts, names longer than 255 bytes, names with `\ : < > " \| ? *` or a control character, names ending in `.` or a space, and Windows device names such as `con`, `nul`, `conin$`, `com1` or `lpt³`, refused on every system alike), or a file appearing twice |
| An ambiguous tarball | a PAX header giving an entry another size than its own header ("its tarball gives `package/x` two sizes …"), a global header that changes paths or sizes, or an extension header describing no entry |
| Another package's tarball | "The tarball of npm package <name>@<version> holds <other>@<version>, not the package asked for" |
| Not a Pane extension | "npm package <name>@<version> is not a Pane extension: it has no pane.json. Pane installs npm packages published as Pane extensions (a pane.json and the WebAssembly components it names); it does not run other npm packages, which need Node.js and npm" |
| Published without its build | "npm package <name>@<version> was published without the built component dist/x.wasm of "<command>": its author must build it and include it in the package before publishing. Pane does not build npm packages or run their install scripts (its package.json has `postinstall` and `prepare`, which Pane never runs)" |

Then every check a folder gets applies: its manifest, WASI 0.3 imports, the
extension API shape, its platforms and its helpers for this system
([compatibility](platform-availability.md), [helpers](helpers.md)).

## The registry

Pane uses `https://registry.npmjs.org/` over HTTPS, through the same
client as extensions' web requests ([ADR 0018](adr/0018-extensions-reach-the-network-through-wasi-http.md),
`crates/pane-core/src/http.rs`): hyper, rustls and the certificates the
system trusts (rustls-native-certs), a new connection for each of the two
requests, no proxy and no redirect. Release builds have no way to replace
the registry. Tests and development builds can, with a registry on this
computer written as a loopback address: `Launcher::with_npm_registry(Registry::local(url))`
and `PANE_NPM_REGISTRY=http://127.0.0.1:<port>/`, both compiled only into
tests and development builds. Any other address is refused, `localhost`
included (a name could resolve elsewhere). The tests and native smokes serve
their fixtures from such a registry
([`npm_registry.rs`](../crates/pane-core/tests/support/npm_registry.rs),
[`scripts/npm_registry.py`](../scripts/npm_registry.py)); none of them
reaches the network.

### Trying the real registry by hand

No check reaches registry.npmjs.org; a contributor can try it by hand, with
a package that is published there as a Pane extension (a `pane.json` and
its built components in the tarball; the sample, `@pane-samples/greeter`,
is private and never published):

1. `cargo build --release -p pane` (a release build always uses the real
   registry and ignores `PANE_NPM_REGISTRY`).
2. `PANE_DATA_DIR=<a new folder> target/release/pane --install npm:<name>`
   (or `npm:<name>@<version>`). The preview's "Downloaded:" line names the
   `https://registry.npmjs.org/…` tarball whose sha512 integrity matched.
3. Choose **Install**, run its command, and check that
   `<folder>/extensions/installed.json` records its `npm` name and
   `npmVersion`, and that `<folder>/extensions/downloads/` is empty.
4. To see a refusal, name an ordinary npm package, such as `npm:left-pad`:
   "npm package left-pad@… is not a Pane extension: it has no pane.json …".

**Not run**: no one has tried this against registry.npmjs.org yet.

## Publishing one

[Author instructions](../guests/README.md#publishing-a-package-to-npm) and
the controlled sample, [`guests/npm/greeter`](../guests/npm/greeter)
(`@pane-samples/greeter`, `"private": true` so that `npm publish` refuses
it), which `cargo xtask guests` assembles and packs on each contributor
system into `target/guests/npm/pane-samples-greeter-0.1.0.tgz`, the tarball
`npm pack` makes of it; it is never published.
[`sample-dependencies-npm`](../guests/packages/sample-dependencies-npm/pane.json)
is a local package requiring it.

## Checks

- Unit tests in [`npm.rs`](../crates/pane-core/src/npm.rs): names and exact
  versions, the loopback-only registry (a literal address, not
  `localhost`), the tarball's origin, sha512 integrity, unpacking (top
  folder, no execute bit, paths outside the package, names systems read
  differently or cannot write, links, devices, pipes, duplicates, sizes,
  entry counts, damaged tarballs, GNU long names and PAX headers: their
  limits, their paths checked, a size only a PAX header gives, long links,
  global headers, dangling headers), and downloads (a folder each, removed
  when dropped; only those begun long ago removed at a start).
- [`crates/pane-core/tests/npm.rs`](../crates/pane-core/tests/npm.rs), with
  a local registry: preview, install, running its command and after a
  restart without downloading again (a start's check for a newer version
  reads the metadata, and downloads nothing while the latest is the
  installed one); the form; the identity (a second install refused,
  Update, a pin kept by an update without a version and changed by naming
  another); a local package requiring an npm one, installed with it and
  called by id; an installed one used as it is (no newer tarball fetched)
  and a disabled one kept disabled; a pinned dependency source, one
  conflicting with the installed version and two dependents pinning
  different versions; an npm dependency using the network recorded as
  such; a dependency that cannot be downloaded; an npm package naming a
  local folder; every refusal in the table, an unreachable registry (its
  port kept bound) and one whose certificate the system does not trust;
  nothing left downloaded after a preview, an install or a component
  failing its check, and a start keeping a download in progress; install
  scripts never run; kept data reclaimed by the name; no Reload or
  Develop rows.
- [`crates/pane-core/tests/update.rs`](../crates/pane-core/tests/update.rs),
  with the settings sample packed as an npm package and served from a
  local registry, and the Git sample's repository from
  [git](git.md#checks)'s own server (the [Git half](git.md#checks) of the
  same suite): a newer version updating the package by itself, keeping
  its settings and ending the old code's generation; a command that is
  running finishing first, the update waiting until the screen the answer
  is on closes; a pinned, disabled and opted-out package not replaced, and
  the controls (both rows) checked through Settings › Extensions; an
  incompatible version, a dependency that cannot be installed and an
  unreachable registry refused with their explanations, the installed
  copy untouched; an installed local folder's copy never asked about or
  touched; an action or an opening asked in the moment the replacement is
  being applied refused with the update's explanation rather than stopped
  by it; a new version that fails to start not rolled back, its settings
  kept and its failure explained when its command is opened; the check
  repeating on its cadence (the clock moved a day on) and at Pane's start
  after a restart.
- [`crates/pane/tests/npm.rs`](../crates/pane/tests/npm.rs): the form,
  preview, Install and Update in the native window at Pane's size, the
  Update row in view below the longer details, and the command running.
- The native smokes' own phase (frames 260 to 268;
  [Linux](platforms/linux.md#npm-packages-45)).

## Limits

- Only the public registry, without credentials: no private or scoped
  registries, `.npmrc`, or enterprise mirrors; no registry setting for
  users.
- The automatic update's choices are provisional: the cadence (a second
  after Pane starts, then every 24 hours), the retry every second while a
  package is in use, and that a disabled or paused package is not updated
  (see [updating by itself](#updating-by-itself)). The spec leaves the
  delivery and activation timing open.
- An automatic update does not wait for calls *other* packages make into
  the one it replaces: they are stopped, answer that the package was
  updated and may be made again, as any replacement stops them. Long-lived
  views and services restart with the new generation.
- More than one Pane on the same data folder would each check and each
  update, as each would run scheduled work.
- The HTTPS path to the real registry is not exercised by the checks, which
  never reach the network; it was not run against registry.npmjs.org
  ([by hand](#trying-the-real-registry-by-hand)). An automatic update over
  it is likewise untried.
- Downloads are not resumed or kept across starts; an interrupted one
  starts again, and the install downloads again what its preview showed. A
  staged update keeps its download while it waits for the package to be
  quiet; a Pane that stops drops it (a start removes downloads begun more
  than a day ago), and the next check stages the version again.
- The integrity is the registry's own; npm signatures and provenance are
  not checked.
- Unpacking reads the whole tarball into memory (at most 64 MiB).
- No proxy is used, as for extensions' web requests: a network that allows
  the web only through a proxy cannot reach the registry.
