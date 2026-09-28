# Pane

A general-purpose launcher for Windows, macOS and Linux whose users can install and create extensions around a small core. Pane is the user-selected product name; current prototype testing covers Windows only.

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
An extension's settings and content: the extension data a user chooses to keep or delete when uninstalling it. Its cache and local credentials are removed either way.
_Avoid_: All extension data, durable data (in UI text)

**Uninstall**:
Removing an installed package's managed copy, cache and local credentials, and its saved data if the user chooses, without running it; its source folder and files it saved elsewhere are kept.
_Avoid_: Disable, delete source

**Retained data**:
Extension data Pane keeps for a package identity that is not installed, recorded with the title it had; installing the same source again makes it that package's data again.
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

**Computed result**:
A root result an extension command computes from the query itself, such as the calculator's answer to "6*7", rather than one found by matching titles; it is listed above those, and invoking it performs its action, such as copying the answer.
_Avoid_: Suggestion, answer card, inline result

**Indexed result**:
A root result an extension command supplies ahead of the query, such as an installed application; Pane asks for them once root search is used, keeps them, and matches and ranks them by title like commands, for a query that is not blank.
_Avoid_: Index entry, cached result

**Installed application**:
A program the operating system lists as installed where Pane looks for it (Start menu shortcuts, application bundles, desktop entries); Pane's host finds and opens it for an extension, which a WASI guest cannot do itself.
_Avoid_: App (ambiguous with Pane itself), program

**Quicklink**:
A named web address the user saves through the Quicklinks default extension's form and finds in root search, where invoking it opens the address with the system's handler for web links; it is kept in that extension's content.
_Avoid_: Bookmark, shortcut, alias

**Global hotkey**:
A key combination the user assigns to an installed command in Pane, which opens that command in Pane's window while any application has focus; Pane keeps it as its own record and registers it with the system only while the command's extension is enabled.
_Avoid_: Shortcut (any key combination, including Pane's own keys), keybinding, alias

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
The `pane.json` file that declares a package's title, version, required extension API and commands, versioned by its manifest version.
_Avoid_: package.json (npm's file)

**Source-only package**:
A package whose manifest names components that have not been built; Pane explains it rather than installing it.
_Avoid_: Broken install

**Managed copy**:
Pane's own copy of an installed package's manifest and components, kept in Pane's data folder, separate from the user-owned source.
_Avoid_: Cache (it is not disposable)

**Update**:
Replacing the managed copy of an installed package from its source while keeping its package identity. A second explicit install of the same identity is rejected instead.
_Avoid_: Reinstall

**Reload**:
Replacing an installed package's code from its source folder while Pane and other packages keep running: the replacement is checked as an install would check it, then replaces the managed copy, the old instances stop and the new code starts. Settings are kept; live state is not carried over.
_Avoid_: Restart, hot swap, update (an update does not start the new code)

**Startup failure**:
A reload whose checked replacement was installed but could not start: a command trapped, or its component could not load or be instantiated (an error the command returns for its view is not one); Pane stops its instances, reports it with Retry and diagnostics and does not restore the earlier code. Distinct from a replacement that fails its checks, which leaves the working code in place.
_Avoid_: Build failure, rollback

**Generation**:
One run of an installed package's code, from when it is installed, enabled or Pane starts until it is disabled or its code is replaced by a reload or an update. Every call into the package belongs to the generation current when it was asked for, and is stopped when that generation ends; its late result is discarded.
_Avoid_: Version (a package's version is its manifest's), session, instance (one generation can start several), screen or search epoch (the launcher's counters of screens and searches, which only decide whether an answer is shown)

**Supported platforms**:
The operating systems a package, a command or an action declares it works on: a plain list, not a rule language. A declaration is not evidence of native support.
_Avoid_: Compatibility rules, target matrix

**Unavailable action**:
An action whose supported platforms exclude the current system; Pane keeps it listed, explains why and never runs it, so the extension's other actions stay usable.
_Avoid_: Hidden action, disabled extension

**Operation**:
A named, versioned function an installed package publishes in its package manifest for other extensions to call through Pane, with JSON input and result; only published operations are callable, so a command is never one implicitly.
_Avoid_: API, command (a command is what the user opens), endpoint

**Call chain**:
The operation calls waiting on one another at one moment, from the command that made the first; each package in it is busy until its call returns, so a call back into one is refused rather than waited on.
_Avoid_: Call stack (of one guest), workflow

**Form**:
A set of fields an extension command asks the user to fill in and submit; the launcher renders its standard controls and the extension validates the submitted values.
_Avoid_: Dialog, custom view

**Custom view**:
An interactive view an extension draws itself from shapes the launcher paints, receiving the user's key and pointer input while it is open; the launcher keeps focus and its accessible representation.
_Avoid_: Canvas, webview, custom control
