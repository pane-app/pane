# Pane

A general-purpose launcher for Windows, macOS and Linux whose users can install and create extensions around a small core. Pane is the user-selected product name; native evidence so far covers Linux (X11) and Windows, with macOS still to run.

## Language

**Launcher**:
The desktop application through which a user finds and invokes actions.
_Avoid_: Agent, operating system

**Core**:
The essential part of the launcher that remains available independently of installed extensions.
_Avoid_: All bundled features

**Extension**:
An installable addition that contributes functionality to the launcher.
_Avoid_: Plugin, add-on

**Default extension**:
An extension provided by default to supply an everyday feature; the user can disable it individually. It may be acquired automatically during initial setup rather than shipped inside the installer.
_Avoid_: Mandatory feature, core feature

**Artifact source**:
Where Pane reads the index of its default extensions and downloads their payloads from: Pane's own downloads, distinct from npm and the Git hosts. A development build can name one on this computer instead, for tests and smokes; a release build cannot.
_Avoid_: Registry (npm's), repository (Git's), update server (Pane's application updates)

**Acquired artifact**:
A default extension's payload Pane downloads itself at first setup: a tarball its index names by version, file, size and sha512 integrity, unpacked and checked as an npm package's tarball is and installed through the same path into a managed copy, whose identity is the default extension's own. Pane keeps what it downloaded in its payload cache only while it still matches that integrity.
_Avoid_: Installer payload (the installer carries none), bundled feature, runtime download (the extension runtime is part of Pane)

**Disabled extension**:
An installed extension whose execution and contributed functionality are switched off, while its settings and unexpired saved data are retained.
_Avoid_: Uninstalled extension

**Extension settings**:
Values an installed package's commands save through Pane, owned by its package identity and kept while it is disabled, updated or Pane is stopped.
_Avoid_: Preferences, cache

**Extension data**:
Values an installed package's commands keep through Pane, of four kinds (settings, content, cache and local credentials), owned by its package identity; the kind decides what a management action such as clearing its cache removes. Pane removes them itself, never by running the extension.
_Avoid_: Storage, state

**Extension content**:
An extension's own durable records, such as notes or history: extension data kept when its cache is cleared.
_Avoid_: Documents (the user's external files), cache

**Extension cache**:
Extension data the extension can compute or download again, which the user can clear at any time without affecting its settings, content or credentials. Distinct from Pane's own compile cache of components.
_Avoid_: Temporary files, managed copy

**Saved data**:
An extension's settings and content, and the clipboard history Pane keeps for it: the extension data a user chooses to keep or delete when uninstalling it. Its cache and local credentials are removed either way.
_Avoid_: All extension data, durable data (in UI text)

**Uninstall**:
Removing an installed package's managed copy, cache and local credentials, and its saved data if the user chooses, without running it; its source folder and files it saved elsewhere are kept.
_Avoid_: Disable, delete source

**Retained data**:
Extension data Pane keeps for a package identity that is not installed, recorded with the title it had; installing the same source again makes it that package's data again. Manage extensions lists it per identity, and the user can delete it there without the extension, which Pane does itself.
_Avoid_: Orphaned data, leftovers (a leftover is a managed folder awaiting removal)

**Local credential**:
A secret an extension keeps on this computer through Pane, such as a sign-in token. Deleting it does not revoke a remote session.
_Avoid_: Account, session

**Root search**:
The launcher's main search and result view before a specific command is opened.
_Avoid_: Every integration's internal search

**Root result**:
One entry root search lists for a query and can invoke, such as an extension command; it is matched by its title, subtitle and, for an installed command, its package's title, and ranked by the core.
_Avoid_: Item (an item belongs to a command's own list), search hit

**Result kind**:
What invoking a root result reaches, shown on its row: Command, Application, File, Link or Fallback. The core derives it from the result's action, never from its title.
_Avoid_: Type, category

**Result section**:
A labelled run of root results: "Commands" over a blank query's results, "Results" with their count over a query's, then "Fallbacks"; a run of computed answers sits under the title of the command that computed them ("Calculator"). Sections only label the list; they never reorder or filter it, and none claims recent use.
_Avoid_: Group (a shortcut group is a Settings term), suggestions

**Quick slot**:
One pin in the ordered list of root results the user pins to root search's pinned home, to invoke with one click or, for the first five, its Ctrl+digit chord (see Number hints). The list has no gaps and no limit: pinning adds at the end, unpinning closes the gap, moving swaps a pin with its neighbour. It holds the result's identity — a registered command by its id, or an indexed result under the command that supplies it — never its row, title or a computed answer, and Pane keeps the list as its own record (`quick-slots.json`), not extension data. A target that is disabled, paused, missing or not listed yet keeps its slot and says why it cannot run.
_Avoid_: Favorite, bookmark, shortcut, dock

**Pinned home**:
What root search shows above its results while the query is blank: the "Pinned" label and the quick slots, as a five-column grid of tiles that wraps, with a "+ Pin" hint in the last row's next free cell (horizontal), or as result rows of the pinned results only, nothing without pins (vertical), as the Launcher page chooses. A query hides it; clearing the query brings it back.
_Avoid_: Start page, dashboard, recents (Pane shows no recent use)

**Number hints**:
The numbers the launcher's items show while Ctrl is held alone for a moment, naming the Ctrl+digit chord that picks each: under the pinned home, the first five quick slots in order from 1 (later ones have none) and then the first rows, up to 9; or 1 to 9 the first rows of a query's results or a command's list. They show only while Ctrl is held; at rest nothing names the chords.
_Avoid_: Shortcut labels, badges

**Window mode**:
How much of the launcher shows while root search's query is blank, as the Launcher page chooses: expanded (the whole launcher) or compact (only the search field, the results and footer appearing with a query; with "Show pinned in compact window mode" on, the quick slots show as a row of small icons under the field).
_Avoid_: Size, density

**Actions panel**:
The panel, opened from the launcher's footer or its Open actions binding, that lists what can be done with root search's selected result: its primary action, then pinning it to the quick slots or unpinning it, then the alias and hotkey configuration of an installed command. A quick slot has one of its own, opening, unpinning and moving it. It lists only operations Pane can perform, and holds the result it opened for.
_Avoid_: Context menu, app menu (the Pane menu is separate)

**Appearance**:
How Pane's windows look, as the user chooses it on Settings' Appearance page: a theme (System, which follows the system's light or dark appearance as it changes, Light or Dark) and a material (Glass, a translucent tint over the system's blur where the platform provides it, or Solid, an opaque window), and, for the launcher alone, a background image with its background effect. Both windows render a choice at once, and Pane keeps it in its own settings record; a development override (`PANE_THEME`, `PANE_MATERIAL`) wins for its process, disables the choices and is never saved. The page's preview is a picture of the launcher in the appearance in effect, not the user's results. Accent colors, blur and tint strength, density, tip visibility and pinned visibility outside compact mode are not Pane settings.
_Avoid_: Theme (one half of it), skin, style

**Background image**:
A picture the user chooses on the Appearance page, drawn behind the launcher's content (ADR 0028). Pane keeps its own copy in its data folder, so the user's file can move or go. The picture fills the panel's top and fades into the panel's canvas, its own color moved toward the picture's; it scrolls away faster than the results and dissolves as they scroll. The Settings window never draws it. On the Solid material the panel under it is the opaque canvas; on Glass it is the canvas at the glass tint's alpha and the picture is drawn at 84%, so the window's frost shows through both. No copy of the desktop's wallpaper is ever drawn.
_Avoid_: Wallpaper (the desktop's), backdrop (the baked frame, an implementation word)

**Background effect**:
The texture drawn into the background image: None, Dither, ASCII, Halftone or Scanlines (the default), Roboco's new-thread background treatments.
_Avoid_: Filter, style

**Frost**:
The surfaces drawn over the background image — the search field's pill, the pins, the selected row and the footer — which blur what is behind them inside the window under a thin tint of the canvas. Without a background image there is no frost.
_Avoid_: Glass (the window's material), acrylic

**Pane menu**:
Pane's own menu, opened from the Pane mark at the left of the launcher's footer, holding Settings. It is not about any result.
_Avoid_: App menu, footer menu, more actions

**Computed result**:
A root result an extension command computes from the query itself, such as the calculator's answer to "6*7", rather than one found by matching titles; it is listed above those (a file result below them), and invoking it performs its action, such as copying the answer. The search it answers owns the call asking for it: a newer query, or leaving root search, cancels a call still pending.
_Avoid_: Suggestion, answer card, inline result

**Computed answer**:
A computed result whose action copies text, such as the calculator's answer: root search draws it as a card showing the query it answers and the text invoking it copies, and nothing else — Pane has no unit conversion and keeps no calculation history. It stays a root result: selectable, with its own id and its copy action.
_Avoid_: Calculation, conversion, answer card (the card is how it is drawn)

**No-results notice**:
What root search shows above its fallbacks when nothing else is listed for a query that is not blank: the query, and what the user can do — pick a fallback, install an extension, or offer a command as one in Manage extensions. It selects nothing: a fallback is chosen only by the user.
_Avoid_: Empty state (a screen's own line when it has no rows), zero state

**Granted folder**:
The one folder the user grants a package through Pane's own "Choose folder…" row, which Pane records itself (not as extension data) and lists for that package's commands under the scan policy; they name its files only by the ids Pane gave them, and Pane opens one after checking it again. The Files default extension's root results come from it.
_Avoid_: Search scope, library, preopen, the extension's folder setting

**Scan policy**:
Pane's fixed bounds on listing a granted folder, the same on every system: regular files only, breadth first in name order, at most 8 folders deep, 5,000 files and 20,000 entries, skipping hidden entries, links and unreadable subfolders; a listing that reaches a bound or skips a subfolder says it is partial. A listing is kept for one visit of root search.
_Avoid_: Indexer, crawl, whole-disk search

**Indexed result**:
A root result an extension command supplies ahead of the query, such as an installed application; Pane asks for them once root search is used, keeps them, and matches and ranks them by title like commands, for a query that is not blank.
_Avoid_: Index entry, cached result

**Installed application**:
A program the operating system lists as installed where Pane looks for it (Start menu shortcuts, application bundles, desktop entries); Pane's host finds and opens it for an extension, which a WASI guest cannot do itself.
_Avoid_: App (ambiguous with Pane itself), program

**Quicklink**:
A named web address the user saves through the Quicklinks default extension's form and finds in root search, where invoking it opens the address with the system's handler for web links; it is kept in that extension's content.
_Avoid_: Bookmark, shortcut, alias

**Clipboard history**:
The text a user copies, which Pane keeps on this computer for an installed package once the user turned it on in the package's command, watching the clipboard only while the history is on and the package runs; a copy its application marks as not to be kept (as password managers do), or from a program the user excluded, is not kept. Each item is kept for the package's retention after it was copied, then Pane deletes it, whether the package runs or not. It is that package's extension data of a kind of its own, written by Pane, never sent anywhere; the Clipboard History default extension shows it.
_Avoid_: Clipboard (the system's current contents, which deleting history never changes), clipboard log, paste history

**Retention**:
How long Pane keeps each clipboard history item after it was copied (7 days unless the user chose otherwise), counted from the copy, so disabling or re-enabling the package or stopping Pane never extends it. Clearing history deletes the items and keeps history on; turning it off and deleting it (the specification's Disable and delete history) also stops keeping what is copied.
_Avoid_: Expiry date (an item's deadline follows from its copy and the retention), TTL

**Clipboard history view**:
How Pane's launcher shows the Clipboard History default extension's command: its kept items, newest first under Today, Yesterday and Older in local time, beside a plain-text preview of the selected one. Pane reads the items itself from the package's clipboard history, only for its own registered default extension (never by a command's title), and copies, deletes, pauses and resumes through the history's existing operations after checking the item, the screen and the package's code are still the ones read. The extension's own list stays reachable for the rest of its controls (retention, exclusions, clearing, turning off and deleting); every other command keeps its list. The view's projection calls each item it reads a record, as the approved contract does; the reference's word is clip, which only its fixture shows to users.
_Avoid_: Clipboard manager, paste history

**Global hotkey**:
A key combination the user assigns to an installed command in Pane, which opens that command in Pane's window while any application has focus; Pane keeps it as its own record and registers it with the system only while the command's extension is enabled.
_Avoid_: Shortcut (any key combination, including Pane's own keys), keybinding, alias

**Alias**:
A word the user gives an installed command in Pane; typing it in root search lists that command first, and, for a query-taking command, typing it before some text lists a row that sends the text to the command when invoked. Pane keeps it as its own record by command id; a disabled package's commands offer none.
_Avoid_: Keyword (an author's search term), shortcut, nickname

**Fallback**:
A query-taking command the user chose to have offered below root search's results for any text typed; it is never chosen by itself, so the text reaches it only when the user invokes it.
_Avoid_: Default action, catch-all

**Query-taking command**:
An extension command that declares it takes a query: text typed in root search, which Pane sends it only when the user invokes it through its alias or as a fallback, and whose answer Pane shows.
_Avoid_: Argument (Raycast's per-field input), search provider (a provider is asked while the user types)

**Command search**:
The search field an opened command has when it searches as the user types, such as one searching an online service; Pane sends the text typed there only to that command, stops a search the text has replaced, and never asks the command from root search.
_Avoid_: Search provider (root search asks those), query-taking command (sent text from root search once, when invoked)

**Network use**:
What Pane shows of an installed package's web requests, which it does not gate: whether its component imports `wasi:http`, and the addresses it tried to reach since Pane started.
_Avoid_: Network permission (nothing is granted or refused)

**Search provider**:
A source of matching results for a query, such as applications, files or an online service.
_Avoid_: The entire search interface

**Package identity**:
The identity that distinguishes an installed source package from other packages, independently of its display title or selected release.
_Avoid_: Display name, command name

**Extension package**:
A unit of installation: a package manifest plus the built components of the commands it lists. A local package is a folder.
_Avoid_: Plugin bundle

**Package manifest**:
The `pane.json` file that declares a package's title, version, required extension API, commands, operations and dependencies, versioned by its manifest version.
_Avoid_: package.json (npm's file)

**Source-only package**:
A package whose manifest names components that have not been built, in a folder, published to npm without them, or a Git revision holding only the source; Pane explains it rather than installing it, and never builds it.
_Avoid_: Broken install

**npm-distributed package**:
An extension package published to the npm registry: a tarball holding its package manifest and built components, identified by its npm name without version. Pane downloads it itself, checks its integrity and unpacks only its files and folders; the unpacked package is then installed like a local package, into a managed copy, while it keeps its npm source identity. Pane runs none of its npm install scripts and installs none of its npm dependencies.
_Avoid_: Node package, npm module (Pane runs no Node code), plugin from npm

**Git-distributed package**:
An extension package distributed as a Git repository whose root holds its package manifest, identified by its repository (host and path, without `.git` or a reference; the path in lowercase on github.com, gitlab.com, bitbucket.org and codeberg.org, which ignore its case, and as written elsewhere): the same repository written as an HTTPS, SSH or scheme-less address is one package. Pane fetches the one revision asked for itself, over HTTPS, checks every object against its id and writes out only its files and folders, then installs it like a local package, into a managed copy, while it keeps its Git source identity. Pane runs nothing from the repository: no build, hook, filter or submodule.
_Avoid_: Cloned extension, repository checkout (Pane keeps no repository)

**Release revision**:
A Git revision of a Git-distributed package whose commit holds the built components its manifest names, such as a release tag or a release branch its author commits the built files to; only a release revision can be installed. A revision holding only the source is a source-only package.
_Avoid_: Release (a Pane release), build

**Tracked reference**:
The branch a Git-distributed package was installed from (the repository's default branch when none was named): an update fetches that branch again, whatever commit it has moved to. A tag or a commit named by its id is a pinned revision instead, which an update keeps; naming another reference changes either.
_Avoid_: Channel, floating version

**Pinned version**:
The exact npm version the user, or a dependency's source, named when a package from npm was installed or updated, recorded so that it is not taken for the latest; updating without a version keeps it, and naming another version changes it. A dependency's source that names a version must get that version: an installed copy of another version is a conflict, since installing another package never replaces an installed required dependency.
_Avoid_: Locked version, version range

**Managed copy**:
Pane's own copy of an installed package's manifest and components, kept in Pane's data folder, separate from the user-owned source.
_Avoid_: Cache (it is not disposable)

**Application update**:
A newer version of Pane itself, which Pane finds out about from its artifact source's index when it starts and tells the user of, as a row in root search; Pane downloads and installs it only when the user chooses, replacing the program in the install folder (the running one renamed aside) so the new version is used on the next start — which the user does, since Pane never restarts itself. Pane's data, and the extensions it installed, are untouched by one.
_Avoid_: Self-update, auto-update (nothing is automatic), app update

**Update**:
Replacing the managed copy of an installed package from its source while keeping its package identity. A second explicit install of the same identity is rejected instead.
_Avoid_: Reinstall

**Reload**:
Replacing an installed package's code from its source folder while Pane and other packages keep running: the replacement is checked as an install would check it, then replaces the managed copy, the old instances stop and the new code starts. Settings are kept; live state is not carried over.
_Avoid_: Restart, hot swap, update (an update does not start the new code)

**Automatic updates**:
Pane replacing the managed copy of an eligible installed package with a newer compatible version from its source, without the user asking: an npm package takes the registry's latest version, a Git one the newer commit of the tracked branch it was installed from; the package must be unpinned (a tag or commit named for a Git package counts as pinned), enabled, not paused and not turned off by the user's controls, and the newer revision is downloaded and checked as an install checks a package before anything is replaced. The replacement waits for a safe activation boundary — never during a command the user asked for that has not answered, nor while one of the package's screens is on display — and it ends the old generation as a reload does, keeping the identity, the saved data, the disabled state, the hotkeys and the aliases. The controls are a global choice and a per-package opt-out; a pinned version or revision, a local folder's copy and a development copy are never updated by themselves.
_Avoid_: App update (the application's own, #54–56), forced update

**Development mode**:
An installed local package whose source folder Pane watches while its author works on it: each save runs the package's documented build command in that folder, staging the components under Pane's data folder, and a build that succeeds reloads the package from there, while one that fails keeps its working code and shows the build's diagnostics. It lasts until the author stops it, the package is disabled or uninstalled, or Pane quits, each of which kills a running build with the processes it started; another installed copy of the package is never affected.
_Avoid_: Watch mode, hot reload, dev copy (a copy is an installation)

**Build failure**:
A development build that did not succeed: nothing is replaced, and the package keeps running its installed code. Distinct from a startup failure, whose replacement was installed.
_Avoid_: Crash, startup failure

**Obsolete build**:
A development build during which the source was saved again: it is never reloaded, what it left in the source folder is replaced with the installed components, and the folder is built again, so an older build cannot replace a newer one; after three in a row, Pane waits for the next save.
_Avoid_: Cancelled build (it runs to its end)

**Startup failure**:
A reload whose checked replacement was installed but could not start: a command trapped, or its component could not load or be instantiated (an error the command returns for its view is not one); Pane pauses the package, reporting it with Retry and diagnostics, and does not restore the earlier code. Distinct from a replacement that fails its checks, or a build failure, which leave the working code in place.
_Avoid_: Build failure, rollback

**Paused extension**:
An enabled extension Pane stopped running after a failure attributable to it: it could not start, or it crashed or stopped responding three times within five minutes (an error it answers with is not a failure, nor a call stopped because a generation ended). Its commands stay listed, saying why they do not run; its saved data is kept, and the pause holds across restarts until the user retries, reloads, updates, disables or enables it. Distinct from a disabled extension, which is the user's choice.
_Avoid_: Crashed extension, quarantined, disabled (by Pane)

**Extension runtime**:
The part of Pane that runs every installed extension's code (the Wasmtime engine; the runtime is a thread in Pane's process today), shared by all extensions; the window, root search's own rows and Manage extensions do not depend on it.
_Avoid_: Engine (one part of it)

**Runtime crash**:
A failure of the extension runtime itself, not attributable to any one extension, such as a panic of its thread: every call it held is stopped and none is run again by itself, even if its effect was done and only its answer lost; Pane names and pauses no extension, keeps saved data, ends the native helpers it ran and starts the runtime again, unless it crashed within five minutes before, when it stays stopped until the user restarts it in Manage extensions. Distinct from an extension's crash (a guest trap), which counts towards pausing that extension.
_Avoid_: Extension crash, paused runtime

**Unresponsive call**:
A guest call whose extension computed for five seconds in all without finishing, holding every other extension's calls behind it; Pane stops it where the guest yields (every guest yields to the runtime at each epoch tick), drops its instance and says so, and, since the extension's own code was running, counts it towards pausing that extension as a crash. Only the guest's own computing counts: time it spends waiting (on a clock, a save, a helper, another extension), time inside Pane's host calls, time the system gave other threads and starting its instance are not, so a healthy extension is never stopped for them.
_Avoid_: Timeout (waiting is not limited), hung extension, frozen

**Runtime hang**:
The extension runtime's shared thread making no progress: inside one piece of its work, outside every guest (which yields each tick) and every host call Pane marks, with its heartbeat still. Pane says it is not responding yet after ten seconds and gives up on it after thirty; which code held it is not known, so no extension is named. Pane gives up on the thread as on a runtime crash (every call it held is answered and none run again, its native helpers end, it is started again unless it failed within five minutes before, nothing is named or paused); the stuck thread cannot be ended, so it is abandoned, fenced so that nothing it still runs changes anything, and runs nothing more if it ever returns. Only the shared thread hangs; one package's slow call is an unresponsive call.
_Avoid_: Unresponsive call (an extension's own), freeze of Pane (the window keeps working)

**Generation**:
One run of an installed package's code, from when it is installed, enabled or Pane starts until it is disabled, paused or its code is replaced by a reload or an update. Every call into the package belongs to the generation current when it was asked for, and is stopped when that generation ends; its late result is discarded.
_Avoid_: Version (a package's version is its manifest's), session, instance (one generation can start several), screen or search epoch (the launcher's counters of screens and searches, which decide whether an answer is shown; a search also cancels its own pending calls for computed results)

**Scheduled work**:
Work Pane runs for an installed package without the user asking: a command's `pane.json` entry declares a schedule, an interval and the item whose action runs, and Pane runs that action each interval while the package's code may run, taking the generation current when the run is due. A disable, an uninstall, a pause or a code replacement ends it; enabling the package, replacing its code or restarting Pane starts it again, from a full interval, never replaying work that fell due meanwhile.
_Avoid_: Timer, cron job, trigger, background service (an explicit continuing service is another activation model), watcher

**Continuing service**:
Work Pane runs for an installed package without the user asking or without an interval: a command's `pane.json` entry declares a service, and Pane calls its component's `run-cycle` in a cycle while the package's code may run, each cycle answering the status to show and how long to wait before the next, so the service paces itself. It begins at once when the code may run (installed or enabled, Pane started, code replaced) and ends when it may not, its pending cycle stopped with the generation and its task's state — the guest instance's — dropped with it; a pause, a disable, an uninstall or a code replacement ends it, and enabling, replacing or restarting Pane starts it again with a fresh task.
_Avoid_: Background task, daemon, worker, watcher, scheduled work (an interval the manifest declares is another activation model), long-lived call

**Supported platforms**:
The operating systems a package, a command or an action declares it works on: a plain list, not a rule language. A declaration is not evidence of native support.
_Avoid_: Compatibility rules, target matrix

**Unavailable action**:
An action whose supported platforms exclude the current system; Pane keeps it listed, explains why and never runs it, so the extension's other actions stay usable.
_Avoid_: Hidden action, disabled extension

**Operation**:
A named, versioned function an installed package publishes in its package manifest for other extensions to call through Pane, with JSON input and result; only published operations are callable, so a command is never one implicitly.
_Avoid_: API, command (a command is what the user opens), endpoint

**Dependency**:
Another package whose operations a package calls, declared in its package manifest with the source it comes from and the operations and versions it calls; the package's code calls it by the declaration's id.
_Avoid_: Library dependency (an npm or Cargo library bundled into a component), extension pack

**Required dependency**:
A dependency a package needs: installing the package shows it and installs it first if it is missing, but never replaces an installed copy (which counts as pinned) or enables a disabled one; a required dependency that cannot be installed or does not publish what is called stops the install before anything changes.
_Avoid_: Hard dependency, prerequisite

**Required dependent**:
An installed package that requires another on this system, directly or through other installed packages that do (its required dependent closure; optional dependencies never count). Disabling the package it requires first shows the enabled ones, which are disabled together or not at all (Disable all or Cancel); enabling that package again does not enable them. Uninstalling it first shows all of them, disabled ones too, with their saved data; they are uninstalled together, keeping or deleting their saved data, or not at all (Uninstall all or Cancel), and installing that package again does not install them.
_Avoid_: Reverse dependency, child extension

**Optional dependency**:
A dependency a package uses only when the user installed it; installing the package lists it but never installs it.
_Avoid_: Soft dependency, suggestion, recommended extension

**Call chain**:
The operation calls waiting on one another at one moment, from the command that made the first; each package in it is busy until its call returns, so a call back into one is refused rather than waited on.
_Avoid_: Call stack (of one guest), workflow

**Native helper**:
A prebuilt program an installed package ships for each target (operating system and processor) it supports, which its commands run through Pane for what a WASI guest cannot do; Pane runs this system's file, never compiles one, and ends its process when the command cancels the run (its own timeout), the call that started it returns, the package's generation ends or Pane quits; otherwise it runs for as long as its work takes, since Pane serves other extensions' calls while one waits on it. Processes the helper starts itself are its own.
_Avoid_: Plugin binary, native extension (the extension's entry point stays a WASI component), sidecar

**Helper target**:
The operating system and processor a native helper's file is built for, written `<os>-<arch>` in the package manifest, such as `linux-x86_64` or `macos-aarch64`; Pane runs only the file for its own target.
_Avoid_: Platform (a supported platform is an operating system alone), triple

**Form**:
A set of fields an extension command asks the user to fill in and submit; the launcher renders its standard controls and the extension validates the submitted values.
_Avoid_: Dialog, custom view

**Custom view**:
An interactive view an extension draws itself from shapes the launcher paints, receiving the user's key and pointer input while it is open; the launcher keeps focus and its accessible representation.
_Avoid_: Canvas, webview, custom control
