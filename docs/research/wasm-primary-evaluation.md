# Reopened runtime direction: WIT + Wasmtime

**Latest runtime tests:** [P3 validation](wasi03-validation.md) now establishes ordinary Rust std plus a locally patched QuickJS guest with P3-only imports and async/library/filesystem-stream execution in a P3-only host. Stock QuickJS still imports P2. The port has measured size/memory cost and unresolved snapshot random-state initialization; no production SDK/backend commitment is implied.

**Superseding requirement: WASI 0.3.** The user explicitly requires p3, not p2. This earlier synchronous comparison does not validate that requirement; QuickJS must be reevaluated with actual p3 interfaces and async configuration. See [current toolchain audit](wasi03-requirement.md) and [accepted decision](../adr/0013-require-wasi-03.md).

2026-09-27. The user clarified: "lets see if we can use those 2 instead, i thought we alr picking them". Treat WIT/wit-bindgen plus Wasmtime as the preferred architecture under evaluation. Earlier Node/native executable decisions are under review; they must not override this correction. This is a concrete feasibility result and recommended prototype design, not an implemented launcher or proof of production readiness.

## Result

**Yes, the architecture is feasible enough to pursue as the primary runtime.** A new Windows experiment compiled one TypeScript guest through both QuickJS and SpiderMonkey componentizers, and ran both in Wasmtime using the same WIT contract as the earlier Rust/wit-bindgen component. All sampled queries returned the expected results. [Reproducible experiment and measurements](wasm-primary-spike/README.md)

For JS/TS, wit-bindgen alone is insufficient: a JS engine/componentizer is also needed. Wasmtime executes the generated component. The engine choice changes package/build/resource costs substantially. Current Jco has experimental Node adapters, so it is inaccurate to say every Node API is inherently unavailable; it is equally inaccurate to promise full Node/npm/native-addon compatibility. Some adapters require our own host imports and shipped reference adapters target a real Node host. [JS/TS tooling audit](wasm-primary-js-review.md)

## Recommended prototype architecture

```text
Rust extension -> wit-bindgen + Rust wasm32-wasip2 compiler --+
                                                          |
JS/TS extension -> bundle + JS componentizer/engine --------+-> Wasm components
                                                               |
GPUI CE UI process <-> launcher messages <-> helper embedding Wasmtime
                                              shared Engine
                                              Store per active extension generation
                                              WASI + launcher host imports
```

Use QuickJS as the next JS backend to validate, based on the measured package/build costs; keep it provisional until representative dependencies, async and lifecycle behavior pass. Rust and JS/TS remain launch authoring targets; Python/C# remain later work, not automatically supported by the component format. Pin released toolchain versions behind our SDK rather than exposing upstream churn to every extension author.

The separate helper is a proposed adaptation of the earlier GUI/runtime process split. WIT defines the guest-to-runtime API; it does not automatically define the helper-to-GPUI IPC protocol. Keep GPUI objects in the UI process and expose typed view/event/resource operations. No GPUI bridge has yet been implemented. [Host design review](wasm-primary-host-review.md)

## Capability and compatibility

The full-trust priority stays. Custom host imports can expose broad filesystem, network, subprocess and desktop functionality; no mandatory per-extension permission prompts are proposed. Host-side implementations can use ordinary Rust/OS libraries. However, guest code cannot directly assume arbitrary Node native addons or native-only Rust crates work under WASI. Those integrations need compatible libraries or supported host/native helper bridges. API coverage is real development work, even with permissive trust.

A prebuilt component can serve multiple OS targets when each host provides its imports. Native sidecars and OS-specific integrations still need platform-specific implementation/artifacts. Ordinary users need not install Node, Cargo or componentizers to run prebuilt components. If this architecture is adopted, the previous user-runtime Node-download decision is no longer the default; Node may still be author build tooling or an explicitly required external integration. Do not quietly retain Node as a second primary runtime or claim arbitrary source-only packages need no build tools.

## Runtime installation and library dependencies

The proposed launcher ships its helper with Wasmtime embedded. Supported prebuilt components need no separately installed Wasmtime, Node, QuickJS or Rust toolchain. JS components contain their guest engine. This is the intended distribution design, not a tested installer; extensions that call external programs still need those programs shipped or otherwise available.

JS/TS dependencies can be bundled into the component at author build time. Pure computation/parsing libraries are good candidates, but each dependency graph must match available language features and APIs. QuickJS demonstrates ES module resolution from `node_modules` during initialization. Node builtins have experimental, API-specific Jco adapters; native addons and unsupported Node behavior do not become compatible merely by generating WIT bindings. [QuickJS module example](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/examples/module-resolution/README.md), [Jco compatibility boundaries](https://bytecodealliance.github.io/jco/interop/nodejs-builtins.html).

Rust dependencies compile into the guest when they support the chosen WASI target. Browser-oriented Wasm support alone does not establish WASI compatibility; native OS APIs, C dependencies and process/thread assumptions need separate checks or host/native bridges. [Rust WASI target](https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip2.html). The host may use native libraries behind the SDK. Actual third-party guest library compatibility remains untested by our toy search experiment. Bundling dependencies can increase package size and resource use; no blanket npm/crates.io compatibility or zero-overhead promise is made.

## Resource evidence and limits

The toy Rust component is 63,347 bytes; the same TypeScript query produced 1,044,344 bytes with QuickJS and 11,963,710 bytes with SpiderMonkey. Precompiled fresh CLI process median peak working sets were 14.22 MiB, 18.82 MiB and 80.13 MiB respectively. These include Wasmtime process overhead; they are not additive per-plugin memory figures or a persistent-host benchmark. First-time compilation peaks were materially higher, and native compiled caches add disk usage. The full Wasmtime CLI executable is about 41 MB, not a minimal embedding measurement.

This does not prove the primary architecture is smaller/faster than the previously proposed shared persistent Node worker design. The Node comparator is a fresh toy process, and no native Rust process baseline was added. Single observed componentization calls were about 0.36 seconds for QuickJS and 23.30 seconds for SpiderMonkey; that is not a real save-to-view hot-reload benchmark. [Raw measurements and exact scope](wasm-primary-spike/results.json)

## What must be resolved before final runtime adoption

1. Implement the same storage/network/subprocess host API for a Rust guest and a JS/TS guest; exercise actual library dependencies and capability needs.
2. Build the GPUI standard-view and custom-interaction bridge without blocking the UI thread.
3. Validate reload/disable, stale events, async cancellation, blocked native host calls and memory/handle reclamation in the persistent helper. Guest execution interruption alone cannot cancel arbitrary host calls.
4. Measure idle/active process-tree resources and latency at realistic extension counts, including first compilation and cache retention.
5. Validate execution/install behavior on Windows, macOS and supported Linux environments. Current execution evidence is Windows only.

The appropriate next step is a focused vertical prototype, not promising every language library already works. This reevaluation supersedes treating Wasm as merely an optional future idea; it does not silently adopt a reduced capability contract or claim that the earlier compatibility benefits of native Node/Rust transfer unchanged.
