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

**Root search**:
The launcher's main search and result view before a specific command is opened.
_Avoid_: Every integration's internal search

**Search provider**:
A source of matching results for a query, such as applications, files or an online service.
_Avoid_: The entire search interface

**Package identity**:
The identity that distinguishes an installed source package from other packages, independently of its display title or selected release.
_Avoid_: Display name, command name
