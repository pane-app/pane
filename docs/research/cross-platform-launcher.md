# Cross-platform launcher: research and candidate design

Researched 2026-09-27. Status: recommendations for discussion, not accepted architecture. Repository source was inspected by separate researchers; no launcher was built or benchmarked. See [Tinycast](tinycast.md) and [Pi](pi.md) for commit-pinned source reports.

Follow-up: the user confirmed Windows/macOS/Linux, small feature core plus low resource usage, and AI as extension functionality. See [ADR 0001](../adr/0001-small-core.md). The user subsequently chose Pi-style full trust and npm/Git/local distribution, prioritizing extension capability over hardening: [ADR 0002](../adr/0002-trusted-extensions-and-open-distribution.md). The restricted contract and permission recommendations below are historical candidates, superseded where they conflict with that decision. Disable/data behavior is accepted in [extension policies](../extension-policy-proposal.md).

Runtime update: WASI 0.3 is required by [ADR 0013](../adr/0013-require-wasi-03.md). The earlier native Rust and managed Node entry-point choices are superseded. WIT/Wasmtime remains under evaluation, with optional native helpers. The earlier [feasibility measurements](wasm-extension-feasibility.md) and [language review](wasm-language-support.md) are historical research; use [current decisions](../current-decisions.md#runtime-direction-and-evidence) for the runtime status.

Renderer update: the user selected GPUI CE and Pi-style standard controls plus custom interactive views, with JS/TS and Rust extension authoring at launch. See [ADR 0003](../adr/0003-gpui-ce-and-extensible-views.md) and [proposed extension UI boundary](gpui-extension-bridge.md). Alternative renderer recommendations below are historical research, not the current renderer decision.

## Recommendation

Build around a deliberately limited extension contract: commands, query results, a few host-rendered views, per-extension storage, and explicitly granted desktop capabilities. Ship useful first-party extensions with the installer while keeping them removable. Make extension creation and replacement unusually easy.

Cross-platform support, third-party extensions, and low resource usage are separate commitments. A small permanent feature set does not establish low memory consumption. Choose rendering and execution technologies only after deciding which commitment takes priority and measuring a representative prototype.

## Requested projects compared

| Project | What to borrow | What does not transfer automatically |
|---|---|---|
| Tinycast | Focused keyboard interaction; lazy command execution; declarative views; generation-based cleanup | macOS frameworks, Raycast compatibility, memory claims, and a large built-in feature set |
| Pi conventional extensions | Simple TypeScript authoring, contribution APIs, local development paths and package distribution | Desktop integration, safe third-party execution, automatic per-extension hot reload |
| Chord inside the Pi repository | Explicit service dependencies, staged replacement and resource retirement | Marketplace, permission sandbox, unconditional rollback, or proof that experimental Pi architecture is deployed |

These conclusions are supported in the pinned [Tinycast source report](tinycast.md) and [Pi source report](pi.md). Tinycast's license text is AGPL-3.0-or-later; Pi's is MIT. Keep source reuse separate from architectural inspiration. [Tinycast license](https://github.com/abue-ammar/tinycast/blob/6fc6aa1b909ca24e3cd25e35c078a7c808ca34a9/LICENSE), [Pi license](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/LICENSE)

The user selected **Pane** as the product name. For current decisions and unresolved work, see [the decision index](../current-decisions.md).

## Relevant precedent beyond the two requested repositories

Gauntlet is unusually close to this idea: TypeScript extensions, React descriptions rendered through Iced, a Rust server, and separate Deno processes for extensions. Its architecture documents permissions and Git URL distribution, with a best-effort security caveat. [Gauntlet architecture](https://gauntlet.sh/docs/information/architecture)

Its repository now says development has stopped. Official support is listed for Linux X11 and Apple silicon macOS; Windows, Wayland, and Intel macOS are best-effort. This is valuable design evidence, but not proof of uniform platform support or a maintained foundation. [Gauntlet repository](https://github.com/project-gauntlet/gauntlet)

Wox is another relevant competitor: its current README advertises Windows, macOS, Linux, native GPU rendering, and Node.js/Python/script extensions. Its approximately 150 MB memory figure is the project's claim, not a measurement made here. [Wox repository](https://github.com/Wox-launcher/Wox)

**Inference:** “Cross-platform launcher with plugins” already exists. The product needs a sharper reason to choose it: a constrained, understandable core; excellent extension authoring; predictable permissions; reliable replacement; and documented resource costs.

## Candidate core boundary

| Host responsibility | Extension responsibility |
|---|---|
| Open/close/focus palette; keyboard navigation | Applications, calculator, quicklinks |
| Command registry and search coordination | File search, notes, snippets, clipboard history |
| Ranking policy, cancellation, bounded results | AI features, service integrations |
| Extension installation, activation, disposal, recovery | Feature-specific settings and background work |
| Permission checks, desktop capability broker | Requests to use granted capabilities |
| Storage namespaces and secrets service | Feature data within its own namespace |
| Consistent list/detail/form rendering | View descriptions and action handlers |

This is a proposed boundary. The core should own the mechanisms needed to enforce contracts; putting a permission check into an optional extension would make the promise unenforceable. Extension removal should leave recovery/settings functionality available. First-party native adapters can exist behind the same public capability API without becoming third-party native-code plugins.

## Cross-platform support contract

“Cross-platform” should specify desktop OS versions, architectures, Linux display systems, and which capabilities are supported, unavailable, or need user setup. It should not imply mobile support without a separate product decision.

| Area | Design implication |
|---|---|
| Global shortcuts | Use platform-specific implementations. The `global-hotkey` crate currently lists Linux X11 only. Wayland requires another path. |
| Wayland shortcuts | XDG GlobalShortcuts provides sessions and user-mediated shortcut binding; verify the installed desktop backend and provide a documented fallback such as a desktop shortcut invoking the launcher. |
| Clipboard history | Reading/writing the clipboard is not the same as observing it continuously while hidden. Do not infer universal history support from a clipboard API. |
| App discovery | Adapt OS application metadata to one extension-facing model. Do not require portable extensions to parse Windows shortcuts or macOS bundles themselves. |
| Window focus/manipulation and typing | Specify capability support per OS/session and detect unavailable behavior. A successful cross-platform build is insufficient validation. |
| Distribution | Test installers, startup integration, signing/update paths, upgrades, and removal on the actual target systems. |

The first two rows follow the [global-hotkey platform list](https://github.com/tauri-apps/global-hotkey) and [GlobalShortcuts portal contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html). The clipboard caveat follows the [Clipboard portal contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Clipboard.html): the portal extends compatible Remote Desktop or Input Capture sessions rather than supplying an independent unrestricted clipboard-history session. The remaining rows are proposed engineering requirements.

## Renderer and runtime choices to evaluate

These are trade-off judgments, not benchmark results.

| Candidate | Why evaluate it | Main cost to validate |
|---|---|---|
| Rust host with Tauri UI | Existing desktop plumbing; familiar frontend tooling | Total webview/helper-process memory, hidden-window behavior, platform rendering differences |
| Rust host with Iced or another native renderer | Controlled UI surface; close precedent in Gauntlet | Accessibility, IME, text behavior, renderer tooling and extension view adaptation |
| Deno extension subprocesses | TypeScript and a documented permission model | Packaging, cold activation, total process memory, unsafe escape capabilities |
| QuickJS extension subprocesses | Compact engine and controllable host API | Implementing browser-like APIs, tooling compatibility, bindings, isolation and time limits |
| Node extension subprocesses | Familiar ecosystem and Pi-like authoring | Plain process separation is not a permission sandbox; resource costs and unrestricted dependencies |

Tauri uses platform webviews and a core/webview process arrangement. That helps explain its deployment model; it does not prove an idle memory target. [Tauri process model](https://v2.tauri.app/concept/process-model/), [webview versions](https://v2.tauri.app/reference/webview-versions/)

Tauri capabilities apply to its window/webview command-access boundary. A custom extension broker still needs to authenticate extension identity and enforce extension-specific permissions; do not treat frontend capability files as a complete sandbox for arbitrary extension code. [Tauri capabilities](https://v2.tauri.app/security/capabilities/)

Deno denies sensitive access unless granted, but child processes launched with subprocess permission are outside its JavaScript permission sandbox. Arbitrary process execution is therefore a major increase in trust, not an ordinary narrowly scoped permission. [Deno execution model](https://docs.deno.com/runtime/run/), [subprocess documentation](https://docs.deno.com/examples/subprocess_tutorial/)

QuickJS documents a small embeddable JavaScript engine. That engine is only one component of the total application, and does not supply a complete Node environment or desktop extension SDK. [QuickJS](https://bellard.org/quickjs/)

**Proposed evaluation order:** first compare a minimal Tauri shell and a native shell on the required platforms if memory is a hard constraint. Separately compare extension activation, API compatibility, isolation, and idle cost. Keep runtime and UI decisions separate. Avoid building a custom React reconciler before confirming that rich React extensions are required.

## Proposed extension contract

- Manifest: stable extension ID, version, supported host API range, entry points, requested permissions, optional OS requirements, settings schema.
- Authoring: TypeScript compiling to a documented JavaScript subset. State exactly which dependencies and APIs work; TypeScript support alone does not imply Node compatibility.
- UI: list, detail, form, actions. Host renders descriptions; extension code does not execute inside the trusted settings/palette UI.
- Queries: asynchronous, cancellable, generation-tagged responses with result and time limits. Slow remote providers must not block local commands.
- Activation: load on invocation/query trigger; avoid waking every installed extension on every keystroke. Background services explicitly declare their need to remain active.
- Storage: per-extension persistent namespace; separate cache; credentials through a host-managed secret API.
- Installation: local development folders and versioned release artifacts first. Validate manifests before running code; retain provenance, checksum and previous version. Avoid executing arbitrary install scripts. A marketplace is a later discovery feature.
- Compatibility: reject unsupported API ranges before activation; publish deprecation and migration rules. A Raycast compatibility layer is a separate commitment, not an automatic benefit of using React.

## Hot reload contract worth promising

Proposed v1: replace one development extension without restarting the host. Restart that extension's view and transient state; preserve explicitly stored data. Do not promise arbitrary state-preserving module replacement.

1. Watch and debounce edits, then compile and validate a candidate version without disrupting the running version.
2. Stage registrations without committing duplicate shortcuts or executing external side effects.
3. Pause new invocations, cancel in-flight queries, and invalidate the old generation.
4. Remove registrations and host-owned resources; request cleanup with a deadline. Terminate the old process if it hangs.
5. Activate the new generation and commit registrations. On failure, restart the previous known-good artifact; make migration rollback rules explicit.
6. Ignore replies tagged with retired generations. Show actionable compiler/runtime errors in an extension console.

File/database writes, sent network requests, and completed external actions cannot generally be undone by reloading. Persistent data migrations need their own compatibility policy. Untrusted candidate code must not gain live capabilities merely because it is being validated.

A useful acceptance test is 100 replacements of an extension that owns a timer, shortcut, subscription, and delayed query: one copy of each resource should remain, stale queries should never update the new view, and memory should stabilize after cleanup. Add compilation failure, hung cleanup, process crash, and permission expansion cases.

## Proposed measurement plan

Measure the full application process tree, including webviews and extension helpers. Record the per-OS accounting method rather than equating unlike working-set/RSS figures. Measure installed footprint separately from download size.

Compare hidden idle, visible idle, cold activation, warm palette opening, and extension-active states. Report extension count and workload. Candidate goals for discussion: under 100 MB hidden idle with the default bundle and no active background extensions; warm hotkey-to-interactive p95 under 100 ms; local-query p95 under 50 ms. These are hypotheses to test, not achieved figures. Installation count should not imply resident runtimes for all installed extensions.

## First implementation slice, after the design interview

Show a palette on all agreed platforms; install one local extension; render a query result; invoke a host capability; edit and reload the extension; prove cleanup and crash containment. Include keyboard layouts, IME, screen-reader navigation and multiple displays in UI evaluation. Only then expand to app search, calculator and quicklinks, followed by optional clipboard history where supported.

The extension host and cross-platform desktop behavior are the main uncertainties. A large library of built-in features would postpone testing them.
