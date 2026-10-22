# Throwaway JS/TS WASI 0.3 and library probe

2026-09-27, Windows x64. **Real 0.3 functionality works, but this stock QuickJS component does not meet the p3-only requirement.** It imports 18 WASI 0.2.12 interfaces for its engine/runtime in addition to WASI 0.3 clocks. This is a measured blocker, not an accepted compatibility exception.

## What ran

TypeScript bundled by esbuild 0.28.2, componentize-qjs 0.4.5 with `sync: false, optSize: true`, Wasmtime 49.0.1, inspected with wasm-tools 1.259.0. Fuse.js 7.5.0 performs fuzzy search and Zod 4.6.5 validates data. The guest imports `wasi:clocks/monotonic-clock@0.3.0`, awaits its actual `wait-for` for 10 ms, and measures the elapsed guest clock. This is component-native async, not only a JS Promise resolved synchronously.

Three functional cases passed: misspelled `calclator` selects Calculator, empty input returns all rows, and an unmatched query returns none. Zod rejects invalid input; successful rows are validated at component initialization. Three precompiled fresh-process invocations also passed. [Results and import list](results.json), [complete extracted interface](component-interface.wit), [actual view output](view.json).

The host is the Wasmtime CLI, not our proposed persistent embedded helper. JSON output is a throwaway bridge payload for the independent native GPUI probe, not a finalized SDK schema. The extra `init` export is upstream tooling output and has not been designed as a public extension API.

## Catches found

- The final component imports p2 stdio, clocks, filesystem, environment and related interfaces. Its custom clock call is p3, but the whole component remains mixed. Do not describe `satisfies_p3_only: false` as a passing runtime acceptance result.
- A WIT `u64` argument supplied as JS BigInt trapped in the current QuickJS lowering (`expected number: FromJs { from: "big_int", to: "f64" }`). Passing Number works for this exactly representable 10 ms value. Full 64-bit integer precision/SDK mapping needs separate validation; do not extrapolate this probe to arbitrary u64 values.
- The componentizer's `stubWasi` flag stubs **all** `wasi:` imports with traps, including actual p3 imports. It is not a migration fix or a way to preserve full capability while declaring p3 compliance. We did not enable it.
- TypeScript was transpiled, not typechecked. No Node APIs, HTTP client libraries, native addons, long-lived stream cancellation, live guest reload or persistent-host resource behavior was established.

Source evidence: [QuickJS component linker and preview1 adapter](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/crates/core/src/lib.rs), [WASI trap-stub implementation](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/crates/core/src/stubwasi.rs). The clocks WIT is copied unchanged from [Wasmtime 49.0.1](https://github.com/bytecodealliance/wasmtime/blob/v49.0.1/crates/wasi/src/p3/wit/deps/clocks.wit).

## Reproduce on this workspace

```powershell
node docs/research/wasi03-js-spike/build.mjs
python docs/research/wasi03-js-spike/run.py
```

The runner prints successful functional case counts **and** `satisfies_p3_only: false`. Author tools use [previous temporary tool paths](../wasm-primary-local.json); libraries/artifacts and wasm-tools use [new temporary metadata](../wasi03-js-local.json). For another machine, install the pinned componentizer/esbuild tools and the two library versions into local temporary prefixes, download matching Wasmtime/wasm-tools, and update these local path records. Library manifests/lockfiles are retained here; original author-tool lockfiles are in [the earlier spike](../wasm-primary-spike/). No global npm package or PATH change was made.

The wasm-tools zip was checked against the SHA-256 in its GitHub release metadata; it remains in temp. Component and native compiled cache are also temporary. The cached artifact is generated locally before running with `--allow-precompiled`.

## Resource scope

The async component with these libraries is 6,991,453 bytes; its native cache is 9,680,992 bytes. Exact per-process working sets and timings are in results. These include CLI/runtime overhead and an intentional clock wait; three cached samples are a smoke measurement, not a production performance benchmark. Differences from the earlier 1.04 MB synchronous dependency-free guest cannot be attributed solely to WASI version: engine configuration, async and libraries all changed.

The next JS task is a p3-capable engine/runtime integration that removes or explicitly implements its legacy requirements without silently relaxing the user's constraint. This probe does not establish an off-the-shelf solution.
