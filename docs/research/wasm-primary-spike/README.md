# JS/TS and Rust components in Wasmtime: bounded comparison

**Historical experiment; not WASI 0.3 validation.** The user subsequently required p3. This run used a Rust p2 target, synchronous QuickJS configuration and a pure synchronous WIT query. Preserve its results, but validate actual p3 imports/async behavior and remeasure the selected configuration. See [requirement audit](../wasi03-requirement.md).

Runtime decision reopened 2026-09-27. This experiment adds actual TypeScript-to-Wasm execution to the earlier Rust-only smoke test. It does not implement the launcher, a GPUI bridge, custom host APIs or a production plugin SDK.

## What ran

The same `query(string) -> list<search-result>` WIT world is used by the existing Rust guest and a new TypeScript guest. esbuild strips TypeScript syntax and bundles to ESM (it does not perform TypeScript type checking). componentize-qjs 0.4.5 uses its sync, size-optimized QuickJS engine. ComponentizeJS 0.23.0 uses SpiderMonkey with stdio/random/clocks/http/fetch-event features disabled because the pure query uses none. No Node adapters, external npm business libraries, networking or native dependencies were exercised.

Both generated components ran successfully in Wasmtime 49.0.1 on Windows x64, returning the expected calculator record. The Rust component was reused from the earlier wit-bindgen 0.62.0 / Rust 1.97.0 wasm32-wasip2 build. Rust guest size excludes an interpreter; JS components embed their engines. The same installed Wasmtime CLI executes all three.

## Results

These are sequential, randomized/interleaved **fresh-process CLI** measurements. Each precompiled mode has 15 samples; each compile-each-process mode has 5. Memory is process peak working set, not per-plugin incremental memory in a shared persistent host. Every sampled result was checked for the expected typed response. Filesystem/OS caches may be warm.

| Guest | Component bytes | Native cache bytes | Precompiled median elapsed | Precompiled peak working set median | Compile-each median elapsed | Compile-each peak working set median |
|---|---:|---:|---:|---:|---:|---:|
| Rust + wit-bindgen | 63,347 | 192,360 | 26.33 ms | 14.21 MiB | 45.82 ms | 28.81 MiB |
| TypeScript + QuickJS | 1,044,344 | 3,490,400 | 29.93 ms | 18.82 MiB | 272.36 ms | 88.09 MiB |
| TypeScript + SpiderMonkey | 11,963,710 | 35,515,000 | 57.77 ms | 80.13 MiB | 2250.09 ms | 439.30 MiB |

The equivalent separate Node toy process measured 59.02 ms and 56.85 MiB across 15 samples. This is **not** the already-proposed persistent shared Node worker architecture, so it cannot establish a production winner. No native Rust executable baseline was measured.

Single observed componentization calls (excluding the preceding esbuild step) took 0.36 seconds for QuickJS and 23.30 seconds for SpiderMonkey. These are developer build observations, not statistical hot-reload latency. First-time Wasmtime compilation also has significant peak memory (about 86.6 MiB QuickJS, 445.8 MiB SpiderMonkey in one compile-only sample). Precompilation changes when that cost occurs, not whether it exists.

The full downloaded Wasmtime CLI is 41,009,152 bytes, shared by these invocations. It is neither the component size nor a measurement of a minimal embedded Wasmtime build. Gzip sizes, all samples, exact tool versions and timing are in [results.json](results.json).

## Reproduction

Developer tooling was installed in an isolated temporary directory recorded in [wasm-primary-local.json](../wasm-primary-local.json), not globally. That local metadata also identifies the originally resolved package versions. `package.json` and `package-lock.json` here preserve the tool dependency graph; npm caches may also contain downloads. The previous [Rust smoke test](../wasm-spike/README.md) supplies the Wasmtime executable and Rust artifact paths in its own local metadata file.

To recreate tool dependencies in another temporary directory, copy this directory's package.json and package-lock.json there and run `npm ci` in that directory. Then, from the workspace root:

```powershell
node docs/research/wasm-primary-spike/build.mjs <tool-directory> qjs
node docs/research/wasm-primary-spike/build.mjs <tool-directory> spidermonkey
```

Update `wasm-primary-local.json` and the older `wasm-spike-local.json` to the correct artifact/tool directories on that machine, then run:

```powershell
python docs/research/wasm-primary-spike/measure.py
```

The measurement script is Windows-specific. It compiles trusted locally generated components to native cache artifacts, invokes them using subprocess argument arrays, validates outputs and records results. On this Windows PowerShell configuration, a direct CLI attempt stripped quotes from `query("calc")`; the Python harness preserves the intended argument. That initial argument-parsing error was not a component failure and is not in the successful sample set.

## Limits and implication

This demonstrates JS/TS and Rust sharing a component interface and execution runtime. QuickJS merits the next prototype on size/build-cost grounds; it is not selected as a final backend by this measurement. Async APIs, long-lived handlers, host storage/network/subprocess calls, actual npm/native library compatibility, GPUI lists/forms/custom drawing, cancellation, reload cleanup, many simultaneously active guests, macOS/Linux execution and persistent resource budgets remain untested. No assertion of full Node compatibility or universal Rust-crate portability follows.
