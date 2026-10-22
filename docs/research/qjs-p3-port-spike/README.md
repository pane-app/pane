# QuickJS P3-only port: executable checkpoint

2026-09-28. **The patched prototype runs JS/TS libraries and real async P3 I/O with no P2 imports.** The stock componentizer still fails that requirement. This is a temporary source patch, not a finalized SDK or an adopted long-term fork.

## What passed

Both the search component and the expanded capabilities component validate with wasm-tools 1.259.0. Their complete external WASI import lists contain 20 interfaces, all `@0.3.0`, and `query` is a native component async export. [Import/function checks](validation-results.json), [search interface](search-component.wit), [capability interface](capabilities-component.wit).

The search guest runs the same Fuse.js 7.5.0, Zod 4.6.5 and 10 ms native async clock workload as the original mixed-version probe. Typo search, empty search, no matches and invalid-data rejection pass.

The expanded guest runs under an [embedded P3-only host](../p3-only-host/README.md). Twenty sequential calls in one instance each perform the async clock wait, library operations, async filesystem open, streamed file read and completion-future read. They explicitly dispose file/preopen descriptors and stream/future handles. Invocation counts progress from 1 through 20; Date values advance at execution time. Separate fresh instances correctly report a missing file and absence of preopened directories. [Host results](host-results.json). These are functional cleanup checks, not proof of leak-free operation under stress or cancellation.

The host's control test accepts the [P3 Rust std guest](../p3-std-spike/README.md) and rejects stock JS at a P2 import. No fallback P2 linker or universal WASI trap stubs were used to obtain the JS pass.

## Changes needed

Based on componentize-qjs commit `e563c6d6ae50b087980414015663ca9c948c09bb`:

1. Rebuild its Rust/QuickJS runtime for `wasm32-wasip3` with pinned nightly 2026-09-27, source-built std and SDK 34.
2. Remove the obsolete preview1 adapter reset call/import; retain libc's preopen/descriptor cleanup.
3. Link the SDK's actual P3 `libc.so` alongside runtime and generated WIT bindings. Remove the preview1 adapter and P2 registration from the scratch componentizer.
4. Explicitly export `__wasm_library_tls_info`. Without it, the dynamic component linker treated the runtime as having no TLS and its synthesized setter trapped during memory initialization.
5. Move QuickJS's suspended-task pointer from canonical context slot 0 to a Rust thread-local Cell. The new libc owns context slots for stack/TLS bookkeeping. The old storage collided with that machinery and caused an out-of-bounds access during async callback cleanup. Libc task hooks preserve the new task-local slot across suspension; the repeated async and stream tests exercise that fix.

[Reviewable source patch](runtime-port.patch), [runtime preparation](prepare-runtime.py), [runtime build](build-runtime.py), [componentizer preparation](prepare-componentizer.py). Initial packaging, TLS and callback failures are retained in their named result/log files. The componentizer build uses prebuilt runtime copies only to satisfy its build script; execution explicitly selects `Runtime::Custom`. They are not claimed to be separate speed/size/synchronous variants.

The scratch native componentizer resolved Wasmtime/Wizer 47.0.4; guest execution used Wasmtime 49.0.1. These are distinct tools. The Rust compiler, SDK and native componentizer are build-time dependencies. End users can still receive built components plus the application's embedded runtime.

## Size and resource observations

Same search workload, release Wasmtime CLI 49.0.1, three fresh precompiled runs per build on Windows:

| Observation | Stock mixed P2/P3 | Patched P3-only |
| --- | ---: | ---: |
| Component | 6.67 MiB | 9.44 MiB |
| Compiled cache | 9.23 MiB | 12.06 MiB |
| Median peak process working set | 30.71 MiB | 36.81 MiB |
| Median process elapsed, including 10 ms wait | 47.37 ms | 46.56 ms |
| One cold compilation peak | 103.85 MiB | 139.80 MiB |
| One compilation elapsed | 199.20 ms | 262.46 ms |

[Raw measurements and scope](measurements.json), [measurement script](measure.py). This first port costs about 2.77 MiB more component storage and 6.10 MiB more peak working set in this check. Stock uses its size-optimized bundled runtime, while this port uses a different Rust build and shared libc without matching wasm-opt processing. Therefore the difference is not an isolated measure of WASI 3 overhead. It is also not launcher idle RAM, steady per-extension cost, a persistent-host benchmark or an installer-size prediction. The expanded I/O guest is about 9.48 MiB and was not the workload used in the comparison.

## Open defect: random state survives snapshotting

> **Update 2026-09-28:** Reproduced on Linux and fixed by a runtime patch with a before/after regression check. `performance.now()` had a related snapshot defect, also fixed. See [Linux validation](../js-backend-validation/README.md). The text below is the original Windows finding.

`Math.random()` produces valid-looking, changing values within an instance, but separate fresh instances of the same snapshotted artifact start with the same value/sequence. The first random value is identical across the three host cases in `host-results.json`. Runtime Date and file input do update normally.

QuickJS initializes `ctx->random_state` during context creation (`rquickjs-sys 0.13.0`, `quickjs.c`); that context is created before the Wizer snapshot. Runtime reseeding or an explicit SDK/runtime random integration must be designed and tested. Do not describe this prototype as having completed randomness initialization or a crypto API. This finding is not proof that P3's host random implementation is defective.

## Reproduction and remaining scope

The recorded tool locations are in [local metadata](../p3-native-toolchain-local.json). With those tools and the pinned audited checkout present, run from the workspace root:

```powershell
python docs/research/qjs-p3-port-spike/prepare-runtime.py
python docs/research/qjs-p3-port-spike/build-runtime.py
python docs/research/qjs-p3-port-spike/prepare-componentizer.py
python docs/research/qjs-p3-port-spike/build-hosts.py
python docs/research/qjs-p3-port-spike/run-componentizer.py
node docs/research/qjs-p3-port-spike/bundle-capabilities.mjs
python docs/research/qjs-p3-port-spike/run-componentizer.py --capabilities
```

The expanded guest uses `bundle-capabilities.mjs`, its local `wit` directory, and the same `p3_build` executable with `capabilities.mjs` as input and `qjs-p3-capabilities.wasm` as output. WIT dependencies were copied unchanged from wasi-libc commit `06513b9ae0c1b14ca3010924939c007ed27628a1`, `wasi/p3/wit/deps`. Run `validate.py` and `validate.py --host-only` after both guests are built; `measure.py` reproduces the small CLI comparison.

Still unvalidated: cancellation/interleaving, long-run memory/resource cleanup, full u64/BigInt mapping, general Node/npm compatibility, native helper lifecycle, GPUI IPC integration, actual guest hot reload, macOS/Linux execution, and production runtime packaging. The earlier GPUI JSON reload is still only a rendering boundary experiment. Next useful work is runtime initialization/reseeding plus lifecycle/cancellation checks, followed by size optimization and persistent-host measurements.
