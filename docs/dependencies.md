# Dependencies on other extensions

Added for [#42](https://github.com/pane-app/pane/issues/42) (US25–US29, T12,
T14, G4), following the accepted Q26 direction in the
[extension policies](extension-policy-proposal.md#dependencies-on-other-extensions).
A package that calls other packages' [operations](operations.md) declares
them in its `pane.json`, required or optional. Installing it from a local
folder shows them first and installs the missing required ones with it;
optional, disabled, paused and already installed dependencies are left as
they are. Disabling a required dependency first shows the packages that
require it, with Disable all and Cancel
([#43](https://github.com/pane-app/pane/issues/43),
[below](#disabling-a-required-dependency)), and uninstalling one shows them
with their saved data, with Uninstall all and Cancel
([#44](https://github.com/pane-app/pane/issues/44),
[below](#uninstalling-a-required-dependency)). Since
[#45](https://github.com/pane-app/pane/issues/45) a dependency can also
come from npm ([npm](npm.md#dependencies-from-npm)), and since
[#46](https://github.com/pane-app/pane/issues/46) from a Git repository
([Git](git.md#dependencies-from-git)), with the same plan.

A package that requires another can also **wait** for it: while a
required dependency is missing, disabled, [paused](pausing.md) or waiting
itself, the dependent's commands stay listed, saying what they need, and
none of their work runs ([below](#waiting-for-a-required-dependency)).

## Declaring

```json
"dependencies": [
  {
    "id": "greeter",
    "source": "local:../sample-operations-js",
    "operations": [{ "id": "greet", "version": 1 }]
  },
  {
    "id": "rust-greeter",
    "source": "local:../sample-operations",
    "optional": true,
    "operations": [{ "id": "greet", "version": 1 }],
    "platforms": ["windows", "linux"]
  }
]
```

| Field | Meaning |
| --- | --- |
| `id` | The name the package's code calls the dependency by, unique in the package: lowercase letters, digits and `-`. It has no `:`, so it is never mistaken for a package identity. |
| `source` | `local:` and a folder path, relative to the declaring package's folder or absolute; `npm:` and an npm package name, optionally with an exact version (`npm:@pane-samples/greeter@0.1.0`, #45); or `git:` and a Git repository, optionally with `@` and a branch, tag or commit (`git:https://github.com/owner/repo@v1.0.0`, #46). A package from npm or Git can name only `npm:` and `git:` sources. Anything else is an invalid manifest ("… must be `local:` followed by a folder path, `npm:` followed by a package name or `git:` followed by a repository; other sources are not supported yet"). |
| `optional` | `false` (the default) for a required dependency, `true` for an optional one. |
| `operations` | Every operation the package calls there, each with the version it calls; at least one. This is the compatibility requirement (an operation's version is the version of its input and result, [operations](operations.md#publishing)), and the only operations a call through the id reaches. |
| `platforms` | Optional: the systems on which the package needs it. Elsewhere it is neither installed nor checked, and a call to it that finds it missing is `unavailable`. |

There is no version range on the package itself: a package's `version` is
free text, and what a dependent relies on is the operations it calls at
their versions.

### Writing a local source

A `local:` path is written the same way on every system: folders separated
by `/`, as `local:../greeter` or `local:shared/greeter`, or absolute from
`/` (meaningful on macOS and Linux). A path with `\`, a drive letter
(`local:C:/…`) or a `//server` share is refused on every system, with "…
must separate folders with `/`, without a drive letter, `\` or a
`//server` share, so that every system reads it alike (such as
`local:../greeter`)", so that a package written on Windows resolves alike on
macOS and Linux.

## Addressing

Pane resolves a `local:` source against the declaring package's source
folder **as Pane resolved it**: a package installed through a symbolic link
(or a Windows junction) is identified by the folder the link points to, and
its relative sources are relative to that folder, not to the link's. A
source folder that exists is resolved as an installed folder is (the
operating system's absolute path, following links; [identity](../guests/README.md#packaging-and-installing-a-local-extension)).
For one that does not exist yet, `.` and `..` are removed from its spelling
and its deepest existing parent folder is resolved by the operating system,
so it matches the identity the folder gets once it is created. What each
dependency resolved to is recorded with the package in `installed.json`
(`"dependencies": [{ "id": "greeter", "local": "/…" }]`), so a source folder
moved or deleted after installing changes nothing. If no installed package
has the recorded identity, a call resolves the recorded folder again, so a
folder that became a link to an installed package after the dependent was
installed still reaches it.

An `npm:` source is the npm package it names, whatever version it gives:
its identity is the name (`npm:<name>`), recorded as
`{ "id": "greeter", "npm": "<name>" }`. A version in the source is a pin:
Pane installs that version, pinned, and the one copy must have it, so an
installed copy of another version, or another dependent pinning another
version, is a conflict rather than a version the dependent did not ask for
([npm](npm.md#dependencies-from-npm)). Without a version, an installed copy
is used whatever its version.

A `git:` source is the repository it names, whatever reference it gives:
its identity is the repository (`git:<host>/<path>`, [Git](git.md#identity)),
recorded as `{ "id": "greeter", "git": "<host>/<path>" }`. A reference in
the source is the revision to install when the repository is missing (a
branch tracked, a tag or commit pinned), and the one copy must be at it: an
installed copy from another branch, tag or commit, or another dependent
naming another one, is a conflict. Without a reference, an installed copy is
used whatever its revision, and a missing one is installed from the default
branch.

A guest calls a dependency with its id where it would give an identity:

```rust
call("greeter".into(), "greet".into(), 1, input).await
```

Pane finds the caller's package from the calling component, looks the id up
in its manifest and uses the recorded identity; the call then behaves as a
call by identity ([operations](operations.md#calling)). Only the operations
and versions the declaration lists are reached through the id; calls by
identity are not limited. The errors for an id:

| Kind | When |
| --- | --- |
| `refused` | The declaration does not list that operation at that version: "Caller declares that it calls `greet` version 1 through its dependency `greeter`, not `wait` version 1; declare it in its pane.json to call it". |
| `not-found` | The caller declares no dependency with that id ("Caller declares no dependency `nobody` in its pane.json, …; it declares `greeter` and `helper`"), or it is not installed: for a required one "Caller requires `greeter` from local folder …, which is not installed; install Caller again to install it", for an optional one "Caller's optional dependency `helper` from local folder … is not installed; install it to use it". |
| `unavailable` | It is needed only on other systems and not installed. |
| `disabled` and the rest | As for any call once the target is found. |

## Installing

Previewing a package (**Install extension from folder…** or
`pane --install <folder>`) works out its dependencies before offering
Install, reading only manifests and checking components without running
them, and lists them under the package's details, each source as declared
(relative to the Source line's folder):

- "Requires: JavaScript operations sample, installed with it from
  local:../sample-operations-js": missing, installed with it. The Install
  row then says "…, and install JavaScript operations sample, which it
  requires" (or "the N extensions it requires").
- "Requires: …, already installed": used as it is.
- "Requires: …, which you disabled: it stays disabled, and <title> cannot
  use it until you enable it in Settings › Extensions": the package is
  installed, the dependency is not enabled, and the status after installing
  says so again. Calls to it answer `disabled`.
- "Requires: …, installed but it is paused after an error; retry it in
  Settings › Extensions": Pane [paused](pausing.md) it; it stays paused, and
  the status after installing says so again.
- "Requires (for B): C, …": a dependency of a dependency Pane installs.
- "Optional: `rust-greeter` from local:../sample-operations, not installed:
  Pane does not install it; install it yourself to use it", or "Optional:
  <title>, installed" (or "installed but disabled").
- "Not needed on this system: `x` from local:../x (only on Windows)".

### Installing what the preview showed

The Install row carries what its plan assumed: the requested manifest, and
for each package it relies on, either the manifest it would install from
that folder, or the installed copy (managed folder, enabled, paused) it
would use. Choosing Install checks that the installed packages are still as
assumed and **claims** the requested package and every one it relies on:
until the install ends, uninstalling, reloading, updating, enabling or
disabling one, or deleting retained data it would reclaim, is refused with
"<title> is part of an install in progress". The install then reads the
folders and works the plan out again. If anything differs (a folder's
`pane.json` changed, a package was installed, uninstalled or changed
meanwhile), nothing is installed and the preview shows the new plan with
"What installing <title> needs changed since it was shown; check it again
and choose Install once more". An install without a preview
(`Launcher::install_package`; `pane --install` opens the preview) works the
plan out, then claims the same way before installing.

Installing then adds the missing required packages, each after its own
required dependencies, then the requested one, and shows "Installed Caller
with JavaScript operations sample, which it requires". An Update does the
same for a new copy that adds a required dependency ("Updated Caller with
…").

### What stops an install

Every problem with a required dependency is found before anything changes;
the preview is then titled "Cannot install <title>" with no Install row, and
the status (also for an install without preview) starts "Nothing was
installed:", listing each:

- **Unavailable**: its folder is missing or not a package, its npm package
  cannot be downloaded or unpacked ([npm](npm.md#what-is-refused)), it is
  source-only, it is for other systems, or its components do not pass
  Pane's checks.
- **Local from npm or Git**: a package from npm or Git names a `local:`
  folder, which is on its author's computer.
- **Git revision**: a `git:` source names a branch, tag or commit the plan's
  one copy is not at: the installed one, or the one fetched for another
  dependent.
- **npm version**: an `npm:` source pins a version the plan's one copy does
  not have: the installed one, the one another dependent pins, or the
  latest one taken for a dependent that pins none.
- **Incompatible**: it does not publish an operation the dependent calls, or
  publishes it at another version, or not on this system.
- **Pinned**: an installed dependency that is incompatible is not replaced.
  When a newer copy in its folder might publish what is called, Pane says
  "…; Pane does not replace the installed copy of <title> while installing
  another extension: update it from its folder if a newer copy publishes
  it". Installing never updates another package.
- **Conflicting**: two packages in the install need different versions of
  one operation from the same source: "A and C need different versions of
  `echo` from B (1 and 2); Pane installs one copy of each source, so they
  conflict". There is one installed copy per source and no solver choosing
  among versions, so no update advice is given.
- **Itself**: a package naming its own folder as a dependency.
- **Too many**: more than 16 packages would be installed with it
  (`MAX_INSTALLED_WITH`).

An optional dependency never stops an install, whatever its state.

### Cycles

Each source is visited once, so packages that require each other (A needs
B, B needs A) are installed together, A last; what each needs of the other
is still checked. An installed dependency's own dependencies are not
visited again: it was installed with them, or they were removed since
(#43, #44).

### Partial installation

Installing writes one package at a time. If one fails (a storage error, or
another install of the same source finishing first), Pane removes again the
packages this install added, most recent first, with their managed copies,
putting back the record of [retained data](extension-data.md#retained-data)
one of them had at its place among the others, so nothing is left installed
without its required dependencies and nothing claims to be ready. The
status names what failed: "Could not install C, which it requires: …". Only
if removing one again fails does it stay installed, and the status says so
("Pane could not remove again what it had installed, which stays installed:
B (…)"). None of the packages ran meanwhile, so none has data to lose.

## Disabling a required dependency

Added for [#43](https://github.com/pane-app/pane/issues/43) (US30, US31,
T13). Pressing the row of an enabled package in **Settings › Extensions** that
enabled packages require does not disable it yet. Pane shows "Disable
<title> and the extensions that require it?" with:

- its source, then every enabled package in its **required dependent
  closure**, each as "<title>, which requires <what brought it in> ·
  <identity>", nearest first, and those in it the user disabled already as
  "Already disabled: …" (they are not changed);
- "Each keeps its settings and saved data. Enabling <title> again does not
  enable them: enable each in Settings › Extensions.";
- the rows **Disable all N** (N counts the package itself; first and
  selected) and **Cancel** ("Keep them all enabled").

Cancel, or Back, returns to the extension's page in Settings (the launcher
to root search; to the extension list with the package's row selected
where the tests show it as a screen); nothing was changed, recorded or stopped. Disable all disables
the package and exactly the dependents shown, recorded in one write of
`installed.json` (all of them or, if it cannot be written, none: each is
enabled again and the error is shown), and says "Disabled Greeter and
Caller, which requires it" or "Disabled Greeter and the 2 extensions that
require it: Caller and Other". Each is disabled as an ordinary
[disable](../guests/README.md#keeping-settings) is: its commands leave root
search, an open one closes, its instances stop and its generation ends
([generations](generations.md)), its hotkeys are released, and its
settings, content, cache and credentials are kept. A package with no enabled
required dependents is disabled at once, as before.

If, when Disable all is chosen, an enabled package requires it that was not
shown (one was installed or enabled meanwhile), nothing is disabled and the
question is shown again with "What disabling <title> affects changed since
it was shown; check it again and choose Disable all once more". A dependent
disabled or uninstalled meanwhile is simply not disabled again. If the
package itself was disabled meanwhile, nothing is disabled and nothing is
asked again, even if a new dependent appeared too.

**The closure.** A package is in it when it requires the package asked
about, or another package in it, on this system, as its dependencies were
recorded when it was installed (`InstalledPackage::dependency_identity`,
reaching an installed package as a call by id does, including a folder
that became a link). Optional dependencies, and required ones declared only
for other systems, never bring a package in. Cycles are allowed: each
package is listed once and the package asked about never is. A disabled
package is followed (what requires it requires the package asked about
too), though not disabled again.

**Enabling again** enables only the package pressed: its dependents stay
disabled, across restarts too, until the user enables each. Enabling a
dependent whose required dependency is still disabled is allowed, and the
dependent then [waits](#waiting-for-a-required-dependency) for it, coming
back by itself once it is enabled.

**Pausing is not disabling.** When Pane [pauses](pausing.md) a required
dependency after it failed, nothing else changes and nothing is asked:
its dependents stay enabled and [wait](#waiting-for-a-required-dependency)
for it, coming back on Retry. The user disabling a paused package still
asks about its dependents (and ends the pause, as disabling does).

`Launcher::set_enabled` remains the single-package switch it was: it
disables only the package given, without asking.

## Uninstalling a required dependency

Added for [#44](https://github.com/pane-app/pane/issues/44) (US30, US31,
US61, US63, T13, T20, G4, G5; contributions). Choosing "Uninstall <title>"
in **Settings › Extensions** for a package that installed packages require
does not show the [single uninstall](extension-data.md#uninstalling-an-extension)
question. Pane shows "Uninstall <title> and the extensions that require
it?" with:

- its source; "These extensions require <title>, directly or through each
  other, so they are uninstalled with it; installing <title> again does not
  install them again:"; then every package of its required dependent closure (the
  same closure as [disabling](#disabling-a-required-dependency)), each as
  "<title>, which requires <what brought it in> · <identity>", nearest
  first. Unlike disabling, disabled dependents are uninstalled too, shown
  as "<title> (disabled), which requires …": without the package they
  require they could never work again;
- "Saved data: Greeter none · Caller 1 setting and 1 content record", each
  package's settings and content (the one asked about first), the data the
  choice is about;
- "Pane removes the installed copy, cache and credentials of each, without
  running it; their source folders and files they saved elsewhere are not
  touched. Deleting a credential does not sign you out of an online
  service.";
- the rows **Uninstall all N and keep saved data** (first and selected, as
  in the single uninstall), **Uninstall all N and delete saved data** and
  **Cancel** ("Keep them all installed"; Esc too).

Cancel, or Back, returns to the extension's page in Settings (the launcher
to root search; to the extension list with the Uninstall row selected
where the tests show it as a screen); nothing was removed, stopped or recorded. Uninstall all applies
the chosen row to every package shown, exactly as uninstalling each alone
would (their commands leave root search, their instances stop, their
managed copies, caches and credentials go, and their settings and content
are kept as [retained data](extension-data.md#retained-data) or deleted),
except that their records leave `installed.json` in **one write**, read
again first so that only those records change and what another Pane on the
same data folder recorded meanwhile is kept: all of them or, if it cannot
be written, none ("Could not uninstall A, B and C:
<reason>. They are all still installed and nothing was deleted."). With
Keep, each package that has saved data gets its own retained record;
another does not. The outcome is "Uninstalled Greeter and Caller, which
requires it; their settings and content are kept", or "…, and deleted their
saved data".

**Partial removal.** Once recorded, every package shown is uninstalled.
Removing each managed copy and each kind of data can still fail
afterwards; then the outcome is an error that names each package with what
is left of it, and only those: "Uninstalled Greeter and Caller, which
requires it, but for Caller, its installed copy in <path> could not be
removed yet (<reason>); Pane removes it when it next starts." Data that
could not be deleted stays on record as retained data of its own identity,
as for a single uninstall.

If, when Uninstall all is chosen, a package that was not shown requires it
(one was installed, or reloaded to require it, meanwhile), nothing is
uninstalled and the question is shown again with "What uninstalling <title>
affects changed since it was shown; check it again and choose Uninstall all
once more". A dependent uninstalled meanwhile is simply skipped. If the
package itself was uninstalled meanwhile, the question closes.

**Installing again.** Installing the package that was asked about installs
it alone: nothing records that its dependents were uninstalled with it, so
they come back only when the user installs each (installing a dependent
installs its missing required dependencies, as always), finding its own
retained settings and content if they were kept. A package that only
optionally uses it is never in the set and stays installed; its calls to
it answer `not-found` meanwhile.

`Launcher::uninstall` remains the single-package uninstall it was: it
uninstalls only the package given, without asking.

## Waiting for a required dependency

Added for [#152](https://github.com/pane-app/pane/issues/152), the first
slice of [#151](https://github.com/pane-app/pane/issues/151) (ADR 0041).
A command of an enabled, unpaused package **waits** while one of its
package's required dependencies is missing, disabled, [paused](pausing.md)
or waiting itself. Optional dependencies never make a command wait, nor
does one the package needs only on other systems, and nothing changes in
`pane.json`: Pane computes who waits from the dependencies already
declared and the packages' states. Every enabled, unpaused package starts
as able to run, and any package with an unmet requirement is removed,
repeating until nothing changes: a cycle of healthy packages runs, and a
cycle with one member missing waits as a whole. It is recomputed whenever
a package is installed, uninstalled, enabled, disabled, paused, retried,
reloaded or updated.

While a command waits, nothing of it runs: not its view, run entry point,
actions, arguments or setup screen; its [schedule](schedules.md)'s ticks,
which are skipped and not replayed; its [service](services.md), which
does not cycle; and its root and indexed results, which root search does
not ask for. A package waiting as a whole answers its published
operations `unavailable` ("<title> is waiting for <what>"). Waiting ends
no [generation](generations.md) and stops no instance: a call or cycle
already running finishes, and an open screen stays, its calls answering
as calls do. Waiting never counts towards [pausing](pausing.md).

The command's row in root search stays listed, saying what it needs
("Needs <title>, which is <state>", the state being not installed,
disabled, paused, or waiting for something else), and a chain names what
is actually missing ("Needs Notes Sync, which waits for Auth: Auth is
disabled"). Pressing Enter shows the reason with a row that fixes it:
"Enable <title>", "Retry <title>", or "Open Manage extensions". Its
quick slots, aliases, global hotkeys and fallbacks say the same reason
and run nothing.

When the requirement is met again — the dependency is enabled, retried,
installed, or its package reloaded or updated — the command comes back by
itself, with nothing for the user to do: its row is ordinary again, its
schedule starts from a full interval, its service's first cycle runs at
once in the instance it still has, and root search asks for its results
on the next query.

## For later slices

A plan (`crates/pane-core/src/dependencies.rs`) is data: its required edges
name the dependent and the target by identity with the target's state, its
problems are a kind with identities, and its wording is only in `Display`
and `Plan::lines`, so the dependent traversal of #43 and #44 can reuse the
recorded graph (`InstalledPackage::dependency_identity`) without the
wording. `dependencies::required_dependents` is that traversal: every
installed package in the required dependent closure, disabled ones included
(each with `enabled` and the package that brought it in), which #43 filters
to the enabled ones and #44 uses whole for Uninstall all. Both confirmations
check the set again when chosen with one step (`still_shown` in
`launcher/dependents.rs`).

## Checks

- [`crates/pane-core/tests/dependencies.rs`](../crates/pane-core/tests/dependencies.rs)
  drives the preview, installing with a missing required dependency and the
  cross-language call by id (JavaScript calling Rust), calls limited to the
  declared operations, optional, installed, disabled, paused, pinned,
  conflicting and unavailable dependencies, cycles, dependencies of
  dependencies, the 16-package limit, `.` and `..`, symbolic links (a
  package installed through one, a dependency folder that became one),
  refused Windows-style and non-local sources, an update adding a
  dependency, a dependency uninstalled later, changes between the preview
  and Install, packages refused to change during an install, and the
  sample.
- Unit tests in [`dependencies.rs`](../crates/pane-core/src/dependencies.rs)
  inject a failing install to check the rollback, including where retained
  data is put back.
- The native smokes install the [dependencies sample](../guests/sample-dependencies/src/lib.rs)
  and show "Hello, Pane, from JavaScript" in the real window (frames 75 to
  77; [Linux](platforms/linux.md#dependencies-42)).
- [`crates/pane-core/tests/waiting.rs`](../crates/pane-core/tests/waiting.rs)
  drives the waiting commands themselves (#152): a dependent waiting while
  its required dependency is disabled, paused, uninstalled or waiting
  itself, and coming back on enable, Retry, install or a reload that starts;
  the reason on the row, Enter's reason and fix rows, the `unavailable`
  answer to a waiting package's operations, waits three deep and the two
  cycles; schedules and services waiting and coming back; root results not
  asked for; the open screen and its calls left alone; and the quick slot,
  alias and fallback saying the reason and running nothing.
- [`crates/pane-core/tests/disable_dependents.rs`](../crates/pane-core/tests/disable_dependents.rs)
  drives disabling a required dependency through the extension list: the
  question listing the closure (through a dependent of a dependent) before
  anything changes, Cancel and Back changing nothing, Disable all disabling
  the shown set, stopping a dependent's running instance and keeping its
  settings, enabling the dependency alone (also after a restart), an
  optional user disabled at once, cycles, a dependent enabled or disabled
  while the question is shown, a record that cannot be written, and Pane
  pausing a dependency disabling nothing else while its dependent waits.
  Unit tests in
  [`dependencies.rs`](../crates/pane-core/src/dependencies.rs) cover the
  closure itself.
- The native smokes' own phase (frames 140 to 143;
  [Linux](platforms/linux.md#disabling-required-dependents-43)) asks,
  cancels, disables both with Disable all and enables the dependency alone.
- [`crates/pane-core/tests/uninstall_dependents.rs`](../crates/pane-core/tests/uninstall_dependents.rs)
  drives uninstalling a required dependency through the extension list:
  the question listing the closure (through a dependent of a dependent)
  with each one's saved data before anything changes, an optional user not
  in it, Cancel and Back changing nothing (also after a restart), Uninstall
  all keeping saved data (only the package with some retained, managed
  copies gone, instances stopped, source folders kept) and deleting it,
  installing the dependency again alone (after a restart) and then a
  dependent finding its settings, a disabled dependent uninstalled with it,
  cycles, a dependent appearing (by a reload) or uninstalled while the
  question is shown, the dependency uninstalled alone meanwhile, a record
  that cannot be written (none uninstalled), a second launcher on the same
  data folder installing a package meanwhile (its record and copy kept), and,
  on Unix, one dependent's
  managed copy that cannot be removed, reported against it alone and
  removed at the next start.
- [`crates/pane-core/tests/npm.rs`](../crates/pane-core/tests/npm.rs)
  drives npm dependencies from a local registry: a local package requiring
  an npm one, installed with it and called by id, an installed one used as
  it is and a disabled one kept, a pinned source, a pin conflicting with the
  installed version or with another dependent's, one using the network,
  one that cannot be downloaded, and an npm package naming a local folder
  ([npm](npm.md#checks)).
- [`crates/pane-core/tests/repositories.rs`](../crates/pane-core/tests/repositories.rs)
  drives Git dependencies from a repository served on 127.0.0.1: a local
  package requiring a Git one at a tag, installed with it and called by id,
  a source naming another revision than the installed one, and a Git
  package naming a local folder ([Git](git.md#checks)).
- [`crates/pane/tests/install.rs`](../crates/pane/tests/install.rs): the
  question in the native window at Pane's size with long source paths,
  whose first choice stays visible (a confirmation's details scroll within
  40% of the window), Escape keeping both and Enter uninstalling both; and
  a waiting command's row and Enter, with real key events: the reason
  under the row, the reason and the "Enable <title>" fix row on the screen
  Enter opens, and the command back once the fix row is chosen (#152).
- The native smokes' own phase (frames 180 to 183;
  [Linux](platforms/linux.md#uninstalling-required-dependents-44)) asks,
  cancels, uninstalls both with Uninstall all and installs the dependency
  again alone.

## Limits

- Local folders, npm (#45) and Git (#46), with these semantics.
- One copy per source, no version ranges and no multi-version solving.
- Waiting covers only what the packages' `pane.json` files already declare:
  the requirement is a whole package, not one operation of it, and the
  Manage extensions rows that would list and fix each one are #157's.
- Only the extension list asks about dependents; `Launcher::set_enabled`
  and `Launcher::uninstall` (used by tests and internal callers) change one
  package. The extension list offers no way to disable or uninstall a
  required dependency while keeping its dependents.
- Uninstall all applies one saved-data choice to the whole set; keeping one
  package's data while deleting another's means uninstalling them one at a
  time, dependents first.
- A dependency's record and its dependents' leave `installed.json` in one
  write, but their managed copies and data are removed one package after
  another afterwards; a Pane stopped in between leaves them as a failed
  removal would (listed copies removed at the next start, data on record as
  retained).
- A second install relying on a package an install in progress has claimed
  is refused rather than waiting.
- The symbolic-link tests are skipped where the system does not allow
  links (Windows without the privilege); Windows junctions are untested.
