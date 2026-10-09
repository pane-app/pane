# Extensions compose through capabilities that Pane brokers, taking Cordis's ideas but not its code

Accepted 2026-10-06 by the user's decision, after research into Cordis and the model behind it. Cordis is the plugin framework that Koishi and its 4,000 community plugins are built on ([cordiverse/cordis](https://github.com/cordiverse/cordis)). Its model is set out in *A Programming Paradigm for Spatiotemporal Composability* (Shi, Zhang and Cui, [arXiv 2608.25512](https://arxiv.org/abs/2608.25512)). The research also looked at Mimir, a Rust coding agent with WASI 0.3 plugins whose source is private, so what is known of it is inferred from its installed artifacts.

This ADR amends:

- [ADR 0011](0011-extension-call-and-result-api.md): a call can now be addressed to a capability, not only to a package.
- [ADR 0012](0012-pi-style-source-identity.md): a capability name is a way of addressing that is independent of package identity.
- [ADR 0005](0005-lazy-activation-and-managed-dependencies.md): what a package needs gates whether its commands may run, and a package may opt in to being activated.
- [ADR 0004](0004-reload-extensions-without-restarting-launcher.md): the "explicit extension support" for restoring temporary state is now defined.
- [ADR 0024](0024-the-host-runs-scheduled-extension-work-by-its-own-clock.md) and [ADR 0025](0025-a-continuing-service-cycles-at-its-own-cadence.md): scheduled work and a continuing service now also wait for what their package needs. ADR 0024 had also rejected timers on the guest's side; this ADR adds timers a guest registers and owns.

It leaves [ADR 0002](0002-trusted-extensions-and-open-distribution.md), [ADR 0010](0010-best-effort-extension-api-compatibility.md), [ADR 0013](0013-require-wasi-03.md), [ADR 0036](0036-extension-ui-is-a-tree-pane-renders-written-with-a-gpui-like-api.md) and [ADR 0037](0037-a-command-declares-its-mode-and-host-functions-decide-what-happens-after-it-runs.md) as they are.

Pane takes Cordis's ideas and not its code. It runs no Node host, and no copy of Cordis runs inside a guest. It builds the model natively in its Rust host for its WASI 0.3 components, as Mimir does.

**Capabilities, with the host as broker.** A capability is a named, versioned set of operations, written `<namespace>:<name>@<major>`, such as `acme:translate@1`. A package declares in `pane.json` that it provides a capability, and it serves the capability's operations through the same entry point that serves its published operations. A consumer declares the capabilities it uses, each required or optional, and calls one by its name, not by a package. Pane routes every such call through the host. When several installed packages provide a capability, the user picks one of them in Settings. Until the user picks, Pane uses the first one installed. A consumer may name a source to install as the default provider when none is installed. Where a consumer declares that it uses every provider, Pane fans the call out to all of them and answers with each one's result, as root search asks every source of root results. The shape:

```json
"provides": [{ "capability": "acme:translate@1", "component": "translate.wasm",
               "operations": ["translate", "languages"] }],
"uses":     [{ "capability": "acme:translate@1", "operations": ["translate"],
               "default": "npm:@acme/deepl-translate" },
             { "capability": "acme:spellcheck@2", "operations": ["check"], "optional": true }]
```

A capability generalises how operations are addressed. It does not replace operations. The call, its JSON input and result, its error kinds, the bounded call chain and the generation that owns each call are those of [operations](../operations.md). A [dependency](../dependencies.md) on one particular package stays, for a consumer that wants that package and no other. Calling a package by its identity stays too. A capability is the second way to address operations, which ADR 0012 left open. Pane keeps no registry of namespaces. Namespacing keeps capabilities from colliding, as the paper's §6.6 advises, and the major version changes when a change breaks consumers, on ADR 0010's terms. A capability is not a permission: ADR 0002's rejected capability grants are another sense of the word.

**Waiting commands.** A package's command waits when a capability it requires, or a dependency it requires, is missing, disabled, paused or waiting itself. Its schedule, its continuing service and its root or indexed results wait with it. A waiting command stays listed and says why, for example "Needs DeepL Translate, which is disabled". Pane runs none of its work. When what it needs returns, the command comes back by itself, with nothing for the user to do. A capability the package provides waits with it, so the package's own consumers wait in turn. This is Cordis's pending state. An optional capability or dependency never makes a command wait: a call to it simply answers why it cannot be served. Waiting ends no generation and stops no instance. Pane binds every call to its target late, so it never has to re-apply a dependent the way Cordis re-runs a plugin.

**Owned registrations.** Anything an extension registers imperatively at run time is an owned WIT resource. That covers dynamic root items and commands, timers, subscriptions, watchers, and capability providers registered at run time. Dropping the resource undoes the registration. So does the instance that holds it going away, and so does the generation ending. Pane tags each registration with its owner and its generation, and refuses a handle whose generation has ended. This is Mimir's owned-resource contract, which is Cordis's "an effect returns its inverse" expressed as resource ownership in the component model. Declarative contributions in `pane.json` stay exactly as they are. A package that wants its registrations made without waiting for the user may declare an activation entry point. Pane calls it when the generation starts, as it starts a continuing service. This is an exception to lazy activation that the author opts into.

**One undo list per generation, inside the host.** Every host subsystem that does something for a generation records how to undo it in one list for that generation. That includes hotkeys, helpers, scheduled and service runs, index entries, watchers and owned registrations. When the generation ends, Pane runs the list newest first, and tests check that the list is empty afterwards. So a new subsystem cannot forget its cleanup. The list is built with the runtime work of [#136](https://github.com/hoangvu12/pane/issues/136). The guest never sees it.

**State handoff across a replacement.** When a package's code is replaced by Reload, by an Update (made by the user or automatic) or by a development-mode reload, Pane offers the old instance an opt-in export to take a snapshot of its state. The snapshot is size-limited and kept only in memory. Pane gives it to the new instance before anything else is asked of it. The command screen that was open is opened again with its launch record. None of this happens after a crash, a pause, a failure to start, or a disable followed by an enable. Every other part of ADR 0004 still holds: saved data survives, live objects and running tasks do not, and data migrations and external effects are never undone.

**What Manage extensions shows.** Manage extensions shows each package's required capabilities and dependencies that are missing or broken, the reason for each and how to fix it. It also shows the cycles among them.

**Not adopted:**

- Automatically re-applying dependents when a provider is replaced. Pane resolves every call through the host when the call is made, so nothing holds a provider to re-apply.
- Hot reload by evicting JavaScript's module cache. Pane's unit of reload is the component.
- Injecting services through Proxy objects.

This decision does not cover Cordis's per-consumer interception or its realms.

Cordis's code is not used, for three reasons:

- **It needs a Node host.** Pane runs no Node and no npm ([ADR 0019](0019-pane-downloads-npm-packages-itself.md)), and dropped its managed Node host ([ADR 0008](0008-managed-node-for-javascript-extensions.md), [ADR 0009](0009-shared-node-helper-with-extension-workers.md)) to keep its core small and light ([ADR 0001](0001-small-core.md)).
- **Pane's guests cannot share one JavaScript context.** Rust and JavaScript guests run in separate Wasm sandboxes, and Cordis's context is a JavaScript object that plugins share. A copy inside each guest would only compose that guest with itself.
- **Pane's teardown is already stronger than Cordis's.** Dropping a Wasmtime store releases everything the store holds, without trusting inverses the author wrote, which the paper's §5.1.1 says Cordis cannot check. Pane's guests yield at each epoch tick, so a loop that never yields is stopped, which Cordis cannot do for JavaScript. And replaced code is truly unloaded, while ES modules cannot be freed.

What Pane lacked were the ideas:

- **Effects owned by one scope and undone with it.** These become the owned registrations and the generation's undo list.
- **Dependents that wait instead of failing.** These become waiting commands.
- **The service broker of the paper's §6.2.** This becomes the capability broker.
- **Cycles detected from the declarations (§6.5).** Manage extensions shows them.
- **Namespaced keys (§6.6).** These name the capabilities.

The paper itself points to WebAssembly imports and dropping a Wasmtime instance as the language-independent form of its model (§6.4, §6.7), and Mimir shows that form working in a Rust host. One difference from Cordis is deliberate: a cycle among healthy packages does not leave them inactive. Pane starts nothing in dependency order, so a cycle only waits when one of its members cannot run. Calls that would go round a cycle are still refused when they are made, as before.
