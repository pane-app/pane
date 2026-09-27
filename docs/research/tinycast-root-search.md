# Tinycast root search and extension search

Research date: 2026-09-27. Inspected upstream HEAD `6fc6aa1b909ca24e3cd25e35c078a7c808ca34a9`, obtained by a fresh shallow clone. This matches the earlier Tinycast audit. Source inspection only; no repository code was built or executed.

## Answer to Q17

The launcher search field, matching/ranking, result aggregation and activation routing are central host functionality in Tinycast. However, root search does **not** run every installed extension's service search on each keystroke. It primarily matches a host-maintained index, including extension command metadata. Opening a command then runs its own interface and search behavior.

This distinction supports a small-core launcher: the core can own the shared search experience while extensions supply commands, searchable entries and optional live providers. The latter provider API is a recommendation for our project, not an existing general Tinycast API established by this audit.

## What appears in root search

`AppIndex.publishEntries()` combines discovered applications and system settings with meeting entries, extension commands, quicklinks, Apple Shortcuts, snippets, system actions, window commands/layouts/rooms, custom commands, quick actions and built-in commands. Entries carry names, alternate titles, keywords, owner names and user aliases. Search profiles are built when the index changes; queries rank this index. Empty search offers favorites and suggestions. [AppIndex](https://github.com/abue-ammar/tinycast/blob/6fc6aa1b909ca24e3cd25e35c078a7c808ca34a9/Tinycast/Features/Launcher/Service/AppIndex.swift#L652), [aliases](https://github.com/abue-ammar/tinycast/blob/6fc6aa1b909ca24e3cd25e35c078a7c808ca34a9/Tinycast/Features/Launcher/Service/AliasStore.swift).

`LauncherScreen` also directly evaluates the typed text as a calculator expression, parses colors, creates an Open in Browser row for a detected web address, and adds configured fallback actions. These built-ins are explicitly wired into the root screen; Tinycast's small resource aspirations do not imply all feature logic lives in external plugins. [LauncherScreen](https://github.com/abue-ammar/tinycast/blob/6fc6aa1b909ca24e3cd25e35c078a7c808ca34a9/Tinycast/Features/Launcher/UI/LauncherScreen.swift#L63).

## Extension commands versus extension content

`ExtensionManager.publishLauncherEntries()` maps the commands in each installed manifest to host `AppEntry` rows. The rows include command keywords and the extension title. It publishes metadata rather than asking every command to execute a query. [ExtensionManager](https://github.com/abue-ammar/tinycast/blob/6fc6aa1b909ca24e3cd25e35c078a7c808ca34a9/Tinycast/Features/Extensions/Service/ExtensionManager.swift#L155).

Selecting an extension row invokes `LauncherCoordinator.launch`, which dispatches to `ExtensionCoordinator.runExtensionCommand`. A view command navigates into the extension command screen; a no-view command runs headlessly. Only then does this user-launch path call the extension runtime. Once inside a view, the extension screen obtains the extension's `onSearchTextChange` handler. Background execution exists independently, but that is not broadcast root-search execution. [LauncherCoordinator](https://github.com/abue-ammar/tinycast/blob/6fc6aa1b909ca24e3cd25e35c078a7c808ca34a9/Tinycast/Features/Launcher/UI/LauncherCoordinator.swift#L117), [ExtensionCoordinator](https://github.com/abue-ammar/tinycast/blob/6fc6aa1b909ca24e3cd25e35c078a7c808ca34a9/Tinycast/Features/Extensions/UI/ExtensionCoordinator.swift#L165), [ExtensionScreen](https://github.com/abue-ammar/tinycast/blob/6fc6aa1b909ca24e3cd25e35c078a7c808ca34a9/Tinycast/Features/Extensions/UI/ExtensionScreen.swift#L95).

Consequently, typing a service's name can find its installed commands. This is different from typing a project name and automatically searching all content inside all connected services. I found no general fan-out to all installed extension search handlers in the inspected root-search path.

## Files and passing the query onward

File search is a dedicated mode. The root index includes the Search Files command, not the entire file result index. `FileSearchCoordinator.show(query:)` opens that mode and optionally seeds the query. Configured fallback rows let users type text first and then choose Search Files, Quick AI, Define, a shell command, or a parameterized quicklink. Merely rendering the fallback rows does not execute their destinations; activation calls `FallbackCoordinator.run`. [FileSearchCoordinator](https://github.com/abue-ammar/tinycast/blob/6fc6aa1b909ca24e3cd25e35c078a7c808ca34a9/Tinycast/Features/FileSearch/UI/FileSearchCoordinator.swift#L45), [FallbackCoordinator](https://github.com/abue-ammar/tinycast/blob/6fc6aa1b909ca24e3cd25e35c078a7c808ca34a9/Tinycast/Features/Launcher/UI/FallbackCoordinator.swift#L27).

Aliases are searchable names for individual entries. This inspection establishes aliases, inline command arguments, and explicit query handoff through fallbacks; it does not establish that arbitrary `service query` text is a universal prefix protocol.

## Design implication and subsequent decision

Keep the main search shell, command registry, ranking, navigation and result presentation in the core. Let installed/default extensions contribute their app entries, quicklinks, calculator answers and service commands. A command can appear immediately without starting its runtime on every keystroke. A scoped command view can then perform live service search. The user subsequently accepted this boundary with Raycast-style initial search behavior in [ADR 0006](../adr/0006-raycast-style-search-with-extension-providers.md). Optional global online search is future design work, rather than something required to copy Tinycast's usual behavior.
