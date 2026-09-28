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

**Root search**:
The launcher's main search and result view before a specific command is opened.
_Avoid_: Every integration's internal search

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

**Supported platforms**:
The operating systems a package, a command or an action declares it works on: a plain list, not a rule language. A declaration is not evidence of native support.
_Avoid_: Compatibility rules, target matrix

**Unavailable action**:
An action whose supported platforms exclude the current system; Pane keeps it listed, explains why and never runs it, so the extension's other actions stay usable.
_Avoid_: Hidden action, disabled extension

**Form**:
A set of fields an extension command asks the user to fill in and submit; the launcher renders its standard controls and the extension validates the submitted values.
_Avoid_: Dialog, custom view

**Custom view**:
An interactive view an extension draws itself from shapes the launcher paints, receiving the user's key and pointer input while it is open; the launcher keeps focus and its accessible representation.
_Avoid_: Canvas, webview, custom control
