# Pi research: extension architecture for a cross-platform launcher

Researched 2026-09-27 from official repository documentation and source, pinned to [`2b0a123de98318c2ff8069661721ce0c3794c34e`](https://github.com/earendil-works/pi/tree/2b0a123de98318c2ff8069661721ce0c3794c34e). No downloaded code was executed. This is a source review, not a runtime benchmark or compatibility certification.

## Findings

**Pi is useful for extension authoring and lifecycle design; it is not itself a desktop launcher foundation.** The current repository contains both the established coding-agent extension system and a newer, separate application-composition runtime called Chord. These must not be conflated.

### Product and core boundaries

Pi describes its CLI as a minimal extensible terminal AI agent. It supports interactive, print, JSON, RPC, and TypeScript SDK use. Current package identity is `@earendil-works/pi-coding-agent`, version `0.87.1`, requiring Node `>=22.19.0`; older examples using the Mario Zechner namespace are not the current package identity. [CLI README](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/README.md), [package metadata](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/package.json)

The repository separates model-provider access (`pi-ai`), agent execution/state (`pi-agent-core`), terminal rendering (`pi-tui`), and the coding-agent application. It also now contains Chord, durable state, and telemetry packages. “Minimal” is therefore a product/interface philosophy, not evidence of a tiny executable, dependency count, startup time, or memory footprint. No size or performance measurements were made. [Repository package map](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/README.md)

### Established extension API

Extensions are TS/JS modules with a default factory receiving `ExtensionAPI`. Supported contributions include commands, model tools, shortcuts, flags, providers, event handlers, renderers, and UI widgets. A single file can be loaded from a user/project extensions directory or an explicit CLI path; directories can expose an entry point. `jiti` removes the requirement for a separate TypeScript build. Factories may be asynchronous. UI capabilities vary by operating mode, so behavior should remain independent of presentation. Crucially, extensions execute **inside the Pi process with its OS permissions**, including access to credentials and files. [Extension documentation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/extensions.md)

The loader keeps contribution maps per extension, validates factories, captures load errors, and continues loading other entries. It stages selected runtime changes until factory completion, discarding staged changes and loading-time event subscriptions on failure. Runtime invalidation makes stale API calls fail and removes tracked event-bus subscriptions. This is meaningful lifecycle protection, but cannot reverse arbitrary filesystem/process side effects from extension code. Host package aliases include compatibility mappings for the old `@mariozechner/*` names. [Loader implementation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/extensions/loader.ts)

### Reload is an explicit lifecycle operation

Established Pi reload sends `session_shutdown` with reason `reload`, invalidates the old runner, reloads settings/resources, resets providers, reconstructs the runtime, and emits `session_start` when bindings exist. Resource reload clears the extension-factory cache. This replaces the extension runtime; it is not proof of automatic file-watching, per-extension atomic replacement, or automatic migration of arbitrary JavaScript state. [Session reload implementation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/agent-session.ts#L3291), [resource reload](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/resource-loader.ts#L444)

Authors are instructed to start long-lived resources during session start or actual use, then clean them up idempotently during shutdown. Commands receive reload/session-switch capabilities that ordinary lifecycle handlers do not, partly to avoid deadlocks. Captured contexts become invalid after replacement. [Extension lifecycle contract](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/extensions.md)

### Installation, dependency identity, and compatibility

Packages distribute extensions, skills, prompts, and themes together. Sources include npm, git, and local paths; local packages are loaded without copying. An optional `pi` manifest declares resources, with directory conventions as fallback. Explicit npm versions and git tags/commits remain pinned. Settings support personal/project scopes and resource filtering; project loading is subject to project trust. Host-provided packages belong in peer dependencies with `*` ranges, not bundled copies; duplicate host copies can split class/registry identity. These rules are useful distribution conventions, not a guarantee of compatibility between arbitrary host and plugin versions. [Package documentation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/packages.md)

The conventional `PiManifest` parser exposes only resource arrays. It does not itself define an extension API version, supported OS list, or permission schema. A new launcher needs to make those decisions explicitly. [Manifest implementation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/pi-manifest.ts)

### Chord: the most directly relevant new material

Chord is independent of other Pi packages. It supplies typed services, facets, dependency-aware activation/disposal, replicated state, and an application-supplied remote-service boundary. One extension can have separately built facets for different environments. Its bundler creates content-addressed CommonJS entries; the Node loader verifies integrity, bypasses Node module caches, and releases retired-generation references on disposal. The host owns transport framing/routing. [Chord overview](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/chord/README.md)

Source inspection confirms substantial reload mechanics, with limits:

- Replacement facets must preserve their required/provided service shape.
- Candidates initialize while previous providers remain available; singleton routing changes before old resources retire.
- Ordinary candidate setup/activation failure can leave the old generation active if cleanup succeeds.
- Cleanup failure or failure after cutover can terminate the host. Rollback is not unconditional.

[Facet host implementation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/chord/src/facets/host.ts#L423)

The bundle loader restricts `require` to declared externals, but uses `node:vm.compileFunction` in the host context. This is a loading/dependency mechanism, **not evidence of a security sandbox**. Its integrity check also does not establish publisher trust. [Bundle loader implementation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/chord/src/node/bundle-loader.ts#L171)

Chord's planning document leaves discovery, installation, version resolution, signatures/trust, sandboxing, authentication, and durable application state outside its scope. Pi's application-host/facet document is explicitly labeled an experimental design specification. Chord is a concrete implementation worth a spike, but that does not make every proposed Pi multi-process feature established product behavior. [Chord scope](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/chord/PLANNING.md), [experimental Pi host specification](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/agent/docs/plugins.md)

### Platform and licensing facts

The release build script targets macOS, Linux, and Windows on both x64 and ARM64, using Bun compilation and bundled assets/native helpers. This is CLI portability; it does not supply global launcher hotkeys, desktop application indexing, windows, or Linux compositor integration. [Binary build script](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/scripts/build-binaries.sh)

Native Windows is documented separately from WSL. Git Bash is the default command shell; an optional native PowerShell tool exists, while editor `!` commands still use Bash. Portable JavaScript does not make shell commands or every extension portable. [Windows guide](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/windows.md)

The repository license is MIT, with copyright and permission notice retention required for copies/substantial portions. This is a factual reading of the repository license; individual dependencies and separately distributed extensions still need their own license review when selected. [LICENSE](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/LICENSE)

## Recommendations for the launcher (design judgment)

1. Borrow the tiny authoring surface: one command extension should fit in one file, with a typed host API and a local development path. Ship useful first-party extensions by default so a small core does not mean an empty product.
2. Keep the core responsible for lifecycle, command/query dispatch, rendering contracts, configuration/storage, and platform adapters. Make app search, calculators, and integrations first-party extensions wherever feasible.
3. Define `activate`, cancellation, resource ownership, `dispose`, and reload-state semantics before promising hot reload. Begin with explicit reload; add a development watcher after replacement is reliable.
4. Choose the trust model before the runtime. A separate plugin process helps crash recovery, but unrestricted subprocesses still have user permissions. Enforced capability security requires a restricted runtime/OS boundary and mediated host APIs.
5. Give manifests host API compatibility ranges, OS support, required capabilities, entry points, and namespaced identity. Validate them before activation and retain a last-known-good version.
6. Evaluate Chord with a bounded prototype: replace a query provider while its UI retains a stable handle; fail candidate activation; leak a timer; cancel an in-flight query; restart the plugin host. Do not adopt replicated state and remote topology before an actual launcher requirement needs them.

## Questions the research makes unavoidable

- Does “small” mean download size, idle RAM, code size, or a small conceptual/API surface? These lead to different runtimes.
- Are installable extensions trusted local scripts or third-party store packages requiring enforceable restrictions?
- Must reload preserve an active command and its UI, or can development reload cancel and reopen it?
- Does cross-platform mean macOS/Windows/Linux at first release, and must every extension work everywhere or declare support honestly?
- Is the first complete user experience app launch/search plus a calculator, or is extensibility itself the product? The latter has no immediate value without creators and useful packages.
