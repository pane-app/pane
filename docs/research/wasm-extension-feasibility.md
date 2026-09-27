# WIT and WebAssembly extensions: feasibility, costs and local smoke test

**Newer direction:** after the Q32 recap, the user reopened runtime selection to consider wit-bindgen + Wasmtime as the primary architecture. Earlier status below is historical. See [the completed reevaluation](wasm-primary-evaluation.md) and [new TS component experiment](wasm-primary-spike/README.md). Both JS backends were now executed on Windows; a production SDK and cross-platform host remain unimplemented.

Researched and smoke-tested 2026-09-27. Status: candidate architecture, not adopted. The user explicitly requested evaluation of `bytecodealliance/wit-bindgen` for JS, Python, Rust and C# extension authoring, including resource costs and catches.

Decision update: [ADR 0002](../adr/0002-trusted-extensions-and-open-distribution.md) chooses trusted extensions and prioritizes capability. Wasm remains under evaluation for interoperability and resource use; sandboxing and per-extension permission enforcement below are research possibilities, not product requirements. Subsequent Q18/Q23 selected managed Node with workers for JS/TS, and Q19 selected native Rust helpers. Wasm is optional future research, not the initial runtime; the recommendations below preserve the earlier evaluation history.

## Verdict

This is a credible way to define one language-independent extension contract. It deserves a bounded multi-language prototype. It does not supply a complete SDK, guarantee support for arbitrary libraries, or make all guest languages equally small. The best initial fit appears to be command and search-provider extensions returning host-rendered data.

The actual stack is:

```text
Versioned WIT API
  -> guest bindings + language-specific compiler/componentizer
  -> portable .wasm component
  -> Wasmtime embedded in the launcher/extension host
  -> explicitly provided launcher services and native OS adapters
```

`wit-bindgen` primarily generates guest bindings for languages compiled to Wasm; it does not execute the component. JavaScript and Python use related tooling. The Rust host can generate its side with Wasmtime's `component::bindgen!`. [wit-bindgen](https://github.com/bytecodealliance/wit-bindgen), [Wasmtime component embedding](https://docs.wasmtime.dev/api/wasmtime/component/index.html)

WIT describes callable contracts and typed data. It is not a UI framework, package manager, permission policy or background-task scheduler. We still own those product decisions. [WIT reference](https://component-model.bytecodealliance.org/design/wit.html)

## What was actually tried

Built a Rust component with wit-bindgen 0.62.0 and Rust's `wasm32-wasip2` target, then invoked it using Wasmtime 49.0.1 on Windows x64. The WIT export accepts a query string and returns records containing result IDs and titles. It returned the expected calculator result.

The component was 63,347 bytes. The downloaded full Wasmtime CLI executable was 41,009,152 bytes; this is a separate runtime cost and is not the minimum embeddable runtime size. Precompilation produced a 192,360-byte local artifact.

| Fresh-process mode, 15 samples each | Median wall time | Median peak working set |
|---|---:|---:|
| Rust component, compile each invocation | 33.73 ms | 28.91 MiB |
| Same component, precompiled locally | 19.50 ms | 14.29 MiB |
| Equivalent Node query, outside Wasm | 47.06 ms | 56.84 MiB |

This demonstrates viable execution and a small Rust guest. It does not prove the launcher will be faster or smaller than a persistent Node design. Process startup, output serialization, runtime setup and shutdown are included; filesystem caches may be warm. No native Rust baseline, embedded host, UI, cross-platform runtime test, host services, hot reload, or JS/Python/C# component execution was measured. [Reproducible source and method](wasm-spike/README.md), [raw results](wasm-spike/results.json)

## Resource costs to expect and measure

1. **Shared runtime:** Wasmtime and any enabled compiler/backend increase the host's code size and baseline memory. Guest `.wasm` size is not the whole application size.
2. **Per-extension runtime:** interpreted languages may package their interpreter or engine with the component. Multiple active components can therefore have separate heaps and language-runtime state. Do not assume automatic deduplication of interpreter state.
3. **Compilation and initialization:** parse/validate/compile/instantiate costs affect first activation. Precompile locally on installation/update and cache compatible artifacts where useful. Precompiled artifacts are configuration/target-sensitive and must come from a trusted compilation path; do not deserialize arbitrary downloaded native caches as though they were sandboxed Wasm. [Precompilation](https://docs.wasmtime.dev/examples-pre-compiling-wasm.html)
4. **Boundary conversion:** strings, lists and records require canonical-ABI representation changes and may involve allocations/copies. Batch search results; avoid thousands of tiny host calls or large image payloads across the boundary on every keystroke. [Canonical ABI](https://component-model.bytecodealliance.org/advanced/canonical-abi.html)
5. **Retention:** installed extensions should not imply active instances. Share the engine and reusable compiled code where supported, instantiate lazily, and drop stores/resources on disable or replacement. Long-lived background extensions need a separate budget. [Wasmtime Store lifetime](https://docs.wasmtime.dev/api/wasmtime/struct.Store.html)
6. **Limits:** configure guest-memory/table/instance limits and execution interruption. These do not cap all host allocations or automatically interrupt a blocking host API. Host network/process/file operations also need cancellation, deadlines and output bounds. [Store limits](https://docs.wasmtime.dev/api/wasmtime/struct.StoreLimitsBuilder.html), [execution interruption](https://docs.wasmtime.dev/examples-interrupting-wasm.html)

No language-independent percentage slowdown or memory figure is justified by the available evidence. Compare a defined set of real extensions under the same lifecycle and UI workload.

## Main functional catches

- **Language support is a matrix.** See [the language audit](wasm-language-support.md) for Rust, JS/TS, Python and C#. Compiler support, build-machine support, dependency availability, async support and execution support are separate questions.
- **Desktop APIs remain ours.** Clipboard, app discovery, global hotkeys and window actions need host imports and per-OS adapters. Portable components cannot overcome missing OS capabilities.
- **Existing packages may not work.** Node native addons, desktop Python wheels, arbitrary subprocess use, .NET reflection/PInvoke and OS-specific libraries require individual validation. Supporting a language does not mean supporting its entire native ecosystem.
- **UI remains a design problem.** Returning a list/detail/form description is straightforward conceptually. Arbitrary React DOM apps or unchanged Raycast views do not become portable by generating WIT bindings.
- **Reload needs an application lifecycle.** Wasm instances provide a clean unit to replace, but timers, host resources, pending requests, migrations and UI state still need cancellation/disposal/generation rules. Rebuilding Rust/Python components may feel different from JS reload-on-save.
- **Versioning still matters.** Generated bindings should declare an API version and imports. Reject incompatible contracts before activation. Introducing new mandatory imports can break older hosts.
- **Async varies by toolchain/version.** Current docs include both WASI 0.2 and 0.3 paths. Do not expose advanced streams/futures in the first SDK until the intended guest languages have passing conformance examples. Runtime docs on `docs.wasmtime.dev/api` currently identify themselves as development-version docs; our local executable was release 49.0.1.

## Historical trust discussion (superseded by Q9)

Unlike an unrestricted Node extension, a Wasm instance reaches external functions through explicitly linked imports; this provides a useful enforcement point. The host can give an extension storage or network access without providing arbitrary shell execution. Any broadly powerful host import must be assessed accordingly. The runtime sandbox does not make host API implementations infallible or eliminate resource exhaustion. [Wasmtime security model](https://docs.wasmtime.dev/security.html)

This makes the earlier trusted-versus-restricted discussion worth revisiting. Flexibility can come from a broad, explicit host API with per-extension grants. An unrestricted native-helper escape hatch, if eventually required, should be a separately identified trust level rather than silently weakening every extension's boundary. This earlier recommendation is superseded by Q9: trusted extensions and capability priority are accepted; no additional trust-level/permission system is selected here.

## Historical next-evaluation proposal (not launch scope)

Use the same WIT search/command contract for a Rust guest and one JS/TS guest first. Add one host storage call, one HTTP request, a structured view and a reload while a request is pending. Measure total process memory, compilation/initialization, steady-state query latency, idle cost and repeated reload behavior. Compare against an equivalent persistent Node extension host.

Then add Python and C# conformance examples with real dependencies. Only advertise a language as supported when its template builds, installs, invokes host APIs, reloads and respects lifecycle limits on the supported authoring/runtime platforms. Cross-platform promise is assessed separately on Windows, macOS and named Linux environments.

Recommendation: keep WIT/Wasmtime as a serious candidate, start small, and stage language support. Do not commit to shipping four equally polished SDKs based solely on binding generation.

## Clarification after Q32

WASI is a group of standardized interfaces for WebAssembly programs, not the name of the executable runtime. Wasmtime is the WASI/Component Model runtime used in our smoke test; wit-bindgen generates guest interface bindings and does not execute components. [WASI](https://wasi.dev/), [Wasmtime](https://docs.wasmtime.dev/introduction.html), [wit-bindgen](https://github.com/bytecodealliance/wit-bindgen)

The selected launcher architecture is [managed Node](../adr/0008-managed-node-for-javascript-extensions.md), [JS workers](../adr/0009-shared-node-helper-with-extension-workers.md), and [native Rust helper processes](../adr/0007-native-rust-extension-processes.md), with GPUI CE rendering. The Windows toy test establishes a working Rust component invocation, not four-language SDK readiness or a resource advantage over the selected persistent architecture. No full runtime comparison was performed or repeated for this clarification.
