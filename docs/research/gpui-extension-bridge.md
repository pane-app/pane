# Extension UI boundary for the selected GPUI CE renderer

**Current status correction:** older Node/native-first recommendations below are historical. WASI 0.3 is required and optional native helpers are accepted. The [new GPUI probe](wasi03-gpui-spike/README.md) builds a native Windows view consuming actual guest JSON and emitting action events; refer to its evidence for tested behavior. This does not yet implement persistent Wasmtime IPC, custom interactive-view parity or guest hot reload.

Design note, 2026-09-27. GPUI CE and Pi-style standard/custom UI are accepted in [ADR 0003](../adr/0003-gpui-ce-and-extensible-views.md). This note proposes the UI boundary; native Rust processes were subsequently accepted in [ADR 0007](../adr/0007-native-rust-extension-processes.md). Managed real Node is also accepted in [ADR 0008](../adr/0008-managed-node-for-javascript-extensions.md). Q23 also selects a shared Node helper with extension workers in [ADR 0009](../adr/0009-shared-node-helper-with-extension-workers.md). No bridge has been implemented.

## Proposed shape

```text
JS/TS extension ---- JS/TS SDK ----+
                                 +-- view updates / UI events -- GPUI CE host
Rust extension ---- Rust SDK -----+
```

Keep layout, text input, focus, rendering and event dispatch in the GPUI CE host. Extensions retain their application logic/state and use SDK helpers to publish views and respond to events. The interface should support familiar standard controls plus custom layout and drawing, rather than a fixed list/form-only vocabulary. Both authoring languages need a real custom-view example before the interface is considered sufficient.

This proposal keeps ordinary UI code separate from GPUI's internal Rust object types. It is intended to support runtime replacement, language interoperability and SDK evolution, not sandboxing. Trusted extensions retain access to native integrations under [ADR 0002](../adr/0002-trusted-extensions-and-open-distribution.md).

## What the selected framework supplies

GPUI CE provides Rust views, entities and custom elements. These are useful implementation tools for the host renderer. They are not by themselves a remotely callable interface that an independently installed JS or Rust extension can use. See the [source audit](gpui-ce.md) for platform and API evidence.

An extension-facing custom-view protocol must identify nodes/resources and events, define focus/input behavior, and manage asset ownership. Low-level drawing commands alone are insufficient for editable text, accessibility or ordinary controls. Prefer host text/input components for those jobs and retain them while updating extension-provided content.

Do not block the rendering thread waiting for an extension response. Apply updates with extension/generation identifiers, discard retired-generation messages, and retire owned resources on disable/reload. These are proposed mechanics to satisfy accepted lifecycle behavior, not a claim they already work.

## Rust direct GPUI code versus an SDK boundary

Direct Rust `Render`/`Element` implementations linked into the launcher offer full framework expressiveness. Turning these into independently distributed, hot-reloadable libraries is a separate problem: Rust's default ABI has no stability guarantees. A C ABI wrapper does not automatically make arbitrary GPUI entities, closures or trait objects stable. [Rust ABI reference](https://doc.rust-lang.org/reference/items/external-blocks.html#abi)

Recommendation: prototype a native Rust extension exchanging view updates and events with the host before committing to direct GPUI dynamic libraries. A native process can use ordinary Rust libraries and be restarted without unloading GPUI objects from a shared address space. This still needs measurements of process overhead and UI event latency. Native Rust processes are now the accepted execution route; the concrete communication protocol and UI interface remain proposed, and the process boundary is not a sandbox.

For JS/TS, managed real Node is accepted; its memory cost must be measured. Q23 selects a shared Node helper with a worker per active JS/TS extension, subject to prototype checks; separate-process exceptions remain only candidates.

WIT can remain under evaluation for typed interoperability, but Wasm cannot directly pass native GPUI objects across its boundary. Choosing GPUI CE does not require WIT or make existing WIT bindings a complete UI SDK.

## Bounded validation work to propose

Use the same three interfaces from both JS/TS and Rust: a searchable list, a form with IME text input, and a custom interactive graph or color picker. Replace an extension while a view is open; verify keyboard focus, cancellation, stale-event rejection and disable cleanup. Measure idle CPU, total process-tree memory, GPU memory where available, startup and update latency. Framework marketing and the earlier Wasm CLI smoke test do not establish these numbers.

Platform verification includes Windows/macOS/Linux rendering and launcher-specific focus/hotkey behavior. No GPUI application was built or benchmarked for this note.

For Q14's reload precedent, see [Raycast](raycast-reload.md) and [Pi](pi-reload.md). Their documented/source behavior does not establish transparent preservation of arbitrary live extension state. The user accepted affected-extension reload behavior in [ADR 0004](../adr/0004-reload-extensions-without-restarting-launcher.md); the concrete transport and UI boundary proposed here remain open. This is not a claim that conventional Pi already performs per-extension replacement.
