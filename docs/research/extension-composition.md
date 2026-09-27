# Extension-to-extension composition

Researched 2026-09-27 for Q25. The user asks whether other products support extensions using one another. Q25 was subsequently accepted; see [ADR 0011](../adr/0011-extension-call-and-result-api.md). Documentation/source review only; no interoperability prototype was run. Pi is investigated separately in [its source audit](pi-extension-composition.md).

## Raycast: launch another command

`launchCommand` can target a command in the same or another extension, supplying manifest arguments and serializable launch context. Cross-extension targets include owner/author, extension and command identifiers. Missing or disabled commands cause an error; the documented cross-extension path presents a permission alert. This is Raycast behavior, not a proposed permission requirement for our already trusted extension model. [Command API](https://developers.raycast.com/api-reference/command)

The return type is `Promise<void>`: resolution means the target has launched, not finished. It does not deliver a target command's computed result. Consequently, this API alone does not implement the earlier example of calling file search and awaiting its result array. This is a scoped observation about the documented API, not a claim that cooperative extensions cannot build callback schemes. [Command API return contract](https://developers.raycast.com/api-reference/command#launchcommand)

## VS Code: commands with return values

VS Code explicitly supports extensions invoking commands supplied by other extensions. Commands accept arguments and may return results; the guide demonstrates retrieving a list of definitions. This is the closest researched precedent for the proposed request/result interface. [Commands guide](https://code.visualstudio.com/api/extension-guides/command)

Its `executeCommand<T>` returns the handler's value, or undefined if no value is returned. Extensions can also export APIs from activation and consume another extension's exports. Such direct exported objects are distinct from command dispatch. [API reference](https://code.visualstudio.com/api/references/vscode-api#commands), [extension APIs](https://code.visualstudio.com/api/references/vscode-api#extensions)

For extensions running on different sides of VS Code's remote architecture, its docs recommend commands: direct exported API objects do not work across UI/workspace hosts, while commands are routed to the correct host and arguments cross a serialization boundary. This is relevant to our JS workers/Rust helpers; it does not mean VS Code already supplies our Rust SDK. [Cross-host communication](https://code.visualstudio.com/api/advanced-topics/remote-extensions#communicating-between-extensions-using-commands)

## Pi: shared events and command dispatch

Conventional Pi exposes a shared `pi.events` bus across loaded extensions. Authors subscribe with `on` and publish with `emit`; `on` returns an unsubscribe function. `emit` returns void rather than waiting for handlers or collecting results. This supports extension coordination, while request/reply conventions would be authored on top. Listeners are cleaned up when the extension runtime is invalidated. See [pinned source audit](pi-extension-composition.md).

Pi also allows an explicitly configured `sendUserMessage` call to pass through extension slash-command handling. That path uses prompt/command semantics rather than a structured result-returning operation API; a missing command can proceed as a normal prompt. The inspected conventional ExtensionAPI does not expose a generic `executeCommand`/`invokeTool` result primitive, and tool discovery metadata is not a callable implementation. These are scoped findings, not a claim that full-trust extensions cannot construct their own integration. See [API and dispatch evidence](pi-extension-composition.md).

## Implications for the launcher

The earlier recommendation combines two distinct capabilities: opening a command's UI and invoking a programmatic operation with a returned result. A searchable UI command need not return useful data or support headless invocation. Authors should explicitly publish the programmatic operations they support; no promise that every installed feature becomes an automation API automatically.

Accepted in Q25: include a small host-routed request/result primitive for explicitly exposed operations, alongside ordinary command launching. Use serializable values across JS/TS and Rust, and an asynchronous completion/error contract. Example: a workflow calls an exposed `files.search` operation, receives file records, then invokes another operation. Names and syntax are illustrative, not a settled SDK.

Keep initial scope to target resolution, lazy activation of enabled targets, dispatch, results/errors and cancellation. Report missing/disabled/incompatible targets without silently enabling them. Dependency installation, global pub/sub, workflow editors, distributed-service discovery and elaborate dependency solvers are separate scope; none is implied by accepting a command-call primitive. Timeouts, reload-generation handling, schema/version checks and recursive call behavior need concrete design before implementation. Full trust remains unchanged.
