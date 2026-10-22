# Extension activation, background work and end-user setup

Researched 2026-09-27 for Q15-Q16 using official documentation and a separate [Pi source audit](pi-activation-setup.md). The user subsequently accepted the recommendations and emphasized no additional manual installation for ordinary users; see [ADR 0005](../adr/0005-lazy-activation-and-managed-dependencies.md). Exact runtime and packaging mechanisms remain open. No extension runtime was installed or benchmarked for this comparison.

## Raycast

Commands are launched on demand and unloaded when finished or when a view command is popped back to root search. This describes command lifetime rather than a claim that each installed extension has its own permanently running process. [Lifecycle](https://developers.raycast.com/information/lifecycle)

Background refresh schedules `no-view` and `menu-bar` commands using an interval. Runs are bounded and can be terminated for exceeding their execution window; scheduling is not exact. Store-installed commands start with refresh disabled, and it activates on first opening the command or through preferences. Users can disable refresh. This is scheduled work, not a general permanent-service contract. The scheduling explanation is macOS-specific; it does not establish identical Windows behavior. [Background refresh](https://developers.raycast.com/information/lifecycle/background-refresh)

Raycast manages and automatically downloads its Node runtime; users do not need to install Node merely to execute ordinary Store extensions. Extension developers separately need Node/npm. Built Store artifacts are downloaded and unpacked; an extension's external tools can still impose extra requirements. [Managed runtime and Store installation](https://developers.raycast.com/information/security), [developer prerequisites](https://developers.raycast.com/basics/getting-started)

## VS Code

Extensions activate on events such as invoking a command, opening a matching language file or expanding a contributed view. Startup activation is also available. The guidance favors activation only when needed. Lazy activation does not mean automatic unloading after every command: authors register contributions and resources for their activated lifetime. [Activation events](https://code.visualstudio.com/api/references/activation-events), [extension anatomy](https://code.visualstudio.com/api/get-started/extension-anatomy)

Desktop VS Code provides a Node extension host, alongside browser and remote configurations. An ordinary desktop extension uses this host rather than asking the user to install Node for the extension host itself; a language server, compiler or external program used by an extension can have additional requirements. [Extension host](https://code.visualstudio.com/api/advanced-topics/extension-host)

The marketplace supports platform-specific VSIX packages and selects an appropriate target package. This is useful precedent for distributing prebuilt native code without requiring every end user to compile it. It is not native Rust extension API support: our Rust SDK and execution interface remain our responsibility. [Platform-specific packages](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#platform-specific-extensions)

## Pi

Pi imports and initializes enabled extension factories during startup/resource reload; individual commands/tools execute later on demand. Authors defer long-lived work to session start or actual use, and clean it up on shutdown. This is eager code loading in a shared process, not one process per installed extension. Its npm installation requires Node; standalone executables embed a runtime, but package installation can still invoke external npm and Git. See [the pinned source audit](pi-activation-setup.md) for implementation evidence and package-build qualifications.

## Launcher direction subsequently accepted

For Q15, expose three kinds of work: on-demand commands/query providers, scheduled tasks, and continuing background services. Read static contribution/activation metadata without importing every extension at launcher startup. Start executable code when its command/event is needed, when a scheduled task is due, or when an enabled background service needs to start. Do not silently classify all extensions as permanent background services.

Clipboard history needs continuous observation while enabled; periodic refresh alone can miss intervening clipboard changes. A file indexer may watch changes, while a weather extension may only need scheduled refresh. An extension can expose several modes, with controls that stop its managed work when disabled. This is lifecycle management under full trust, not a restriction on extension capability.

For Q16, ordinary supported extension packages should run without manually installing Node or a Rust toolchain. Ship/manage the selected JS runtime and publish prebuilt native Rust artifacts per supported OS/architecture if the native Rust path is selected. If Wasm is selected later, its runtime and supported host interfaces need equivalent packaging; that decision remains open.

Pi-style npm/Git/local sources remain accepted. Installation source and artifact format are different choices: external sources can provide ready-to-run releases, while source-only packages may need explicitly managed build tooling or a developer setup path. Do not promise arbitrary Git repositories or packages with native build hooks are toolchain-free.

Trade-offs: managed runtimes increase download/disk footprint and require updating; prebuilt Rust packages require a platform build matrix; background services have ongoing CPU/memory cost. Lazy activation can reduce unnecessary work but does not establish a numerical resource budget or mean each activation creates a separate process. None of those costs were measured here.
