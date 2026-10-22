# JS/TS WASI 0.3 backend: Linux validation (issue #2)

> The adopted toolchain now lives in [tools/componentize-js](../../../tools/componentize-js/README.md) (issue #6), with the same pins and patches. This directory is kept as the evidence behind it.

2026-09-28, Linux x86_64 (24 cores). **The patched QuickJS candidate reproduces on Linux. It passes the positive cases and the mixed P2/P3 control, and now runs JS and TS guests for Pane's real contract through `pane-core`.** The fresh-instance randomness defect from the [Windows checkpoint](../qjs-p3-port-spike/README.md) was reproduced. A second snapshot defect was found (`performance.now()` was negative). Both are fixed by a small runtime patch, and a regression check fails before the fix and passes after. This is still a patched prototype of an upstream tool, not a finished SDK or an adopted fork.

## Results

All checks are in [validation-fixed.json](validation-fixed.json). The same checks against the build without the fix are in [validation-unfixed.json](validation-unfixed.json).

| Check | Unfixed | Fixed |
| --- | --- | --- |
| P3 artifacts validate, and all 20 WASI imports are `@0.3.0` (capabilities, JS sample, TS sample) | pass | pass |
| Stock componentize-qjs 0.4.5 output imports WASI 0.2 (18 `@0.2.12` interfaces) | pass | pass |
| P3-only host: Fuse.js 7.5.0 + Zod 4.6.5 + 10 ms native async clock wait + async filesystem stream read, 20 sequential calls in one instance (invocations 1..20, 20 distinct `Math.random` values) | pass | pass |
| P3-only host: missing file gives a `no-entry` payload; no preopen gives an error | pass | pass |
| P3-only host rejects stock output (`wasi:cli/environment@0.2.12` has no matching implementation) | pass | pass |
| **Regression:** 5 fresh host instances produce distinct first `Math.random` values | **fail** (all `0.4491794857419842`) | pass |
| **Regression:** `performance.now()`/`timeOrigin` are relative to the fresh instance | **fail** (`perfNow` ≈ −115 ms, constant `timeOrigin` 131.119) | pass (≈ 12–13 ms after a 10 ms wait) |
| `pane-core` runs the JS sample and the TS sample: `get-view`, `greet`, 50 ms `wait`, `random`, and an unknown item returning `CallError::Guest` | pass | pass |
| **Regression:** 3 fresh `pane-core` runtimes give distinct `Math.random` (TS sample) | **fail** (all `0.2946778650034909`) | pass |
| `pane-core` rejects the stock TS sample with `CallError::Incompatible` (lists the P2 imports) | pass | pass |

`cargo xtask ci` passes with the added example.

## Root cause and fix

QuickJS (quickjs-ng, vendored in `rquickjs-sys 0.13.0`) sets `ctx->random_state = js__gettimeofday_us()` in `JS_NewContextRaw`, and `JS_AddPerformance` sets `ctx->time_origin` from the monotonic clock. componentize-qjs creates the context inside Wizer at build time, so both values are frozen into the snapshot. Every instance therefore resumes the same xorshift64* sequence. `performance.now()` also subtracts the build-time monotonic reading from the runtime host's clock, which starts near zero for each new Wasmtime context. The stock componentize-qjs 0.4.5 output has the same `Math.random` defect: the same `run-action("random")` returned `0.5602532543360419` in two fresh `wasmtime run` processes. So this is an upstream snapshot issue, not something the P3 port introduced.

The fix is [random-reseed.patch](random-reseed.patch), about 115 added lines in the componentize-qjs runtime crate. It does not change the QuickJS C source.

- Before the shim or user module is evaluated, the patch installs a native `Math.random` and a new `performance` object: `now()`, plus `timeOrigin` as a getter. The random generator is the same xorshift64* with the same mantissa construction QuickJS uses. It is seeded from `getentropy`, which wasi-libc backs with `wasi:random/random@0.3.0`. The `performance` object has to be replaced because QuickJS defines `timeOrigin` as non-configurable.
- The last step of Wizer initialization sets a "pending" flag. The first runtime entry of each instance (`with_ctx`) clears it and reseeds, before any JS runs in that instance.
- The state lives in plain `static`s, not `thread_local!`. A first attempt with `thread_local!` returned 0 from every call. The value written during initialization was never seen at runtime. This matches the existing patch's note that SDK 34's libc manages task-local context/TLS. I did not investigate the exact libc mechanism.

Other state the snapshot captures, left as is:

- Module top-level code runs at build time. `buildRandom` and `buildDateMs` in the capabilities guest are the same in every instance. This is how snapshotting works: authors must not create secrets, IDs or timestamps at module top level.
- QuickJS's `ctx->hash_seed`, used for Map/Set key hashing, is also derived at build time and is the same in every instance. It cannot be reseeded after the snapshot, because existing Map entries would stop hashing to the same buckets. The practical effect is weaker hash-flooding resistance, not wrong results.
- `Date.now()` reads the clock on every call and was already correct. No `crypto` API was added or checked.
- Components are not bit-for-bit reproducible, because the snapshot contains build-time clock and entropy-derived state. The runtime `.wasm` without the fix did reproduce bit-for-bit (`f53807b9…`) across two clean builds.

## Pane contract through the real runtime

[guests/sample.js](guests/sample.js) (plain JS, no bundler) and [guests/sample.ts](guests/sample.ts) implement `pane:extension/command`: `export const command = { getView, runAction }`. A top-level `result` maps to return or throw. [guests/pane-world.wit](guests/pane-world.wit) `include`s an unmodified copy of `wit/extension.wit` and adds `import wasi:clocks/monotonic-clock@0.3.0`. JS can only import what its world declares, whereas a Rust guest adds the imports it uses. The TS sample is type-checked with TypeScript 7.0.2 against a hand-written [wasi.d.ts](guests/wasi.d.ts), because componentize-qjs does not generate TS declarations.

These guests run through `pane_core::Runtime` by way of a new example, [crates/pane-core/examples/run_guest.rs](../../../crates/pane-core/examples/run_guest.rs). It uses only the public API: no pane-core source or API change, and no test depends on the scratch toolchain. The exported interface matches the host's `bindgen!` world unchanged.

## Sizes and costs (Linux, fixed build)

The raw data and scope are in [measurements-linux.json](measurements-linux.json). These are fresh release processes with 3–5 samples. Peak RSS covers the whole process, measured with GNU time.

| | Rust sample | JS sample | TS sample | P3 search (Fuse/Zod) | Stock search |
| --- | ---: | ---: | ---: | ---: | ---: |
| Component | 40 KB | 4.01 MB | 4.01 MB | 9.92 MB | 7.54 MB |
| gzip -9 | 15 KB | 1.38 MB | 1.38 MB | 2.92 MB | 2.11 MB |
| Cranelift compile, in process | 24 ms | 1.01 s | 1.01 s | 1.01 s | — |
| Compile peak RSS | 15 MiB | 81 MiB | 81 MiB | 99 MiB | — |
| Precompiled (`.cwasm`) | 0.12 MB | 5.70 MB | 5.70 MB | 11.86 MB | 10.59 MB (CLI) |
| Deserialize `.cwasm` | 0.13 ms | 0.86 ms | 0.86 ms | 0.83 ms | — |
| Instantiate (first / later median) | 0.05 / 0.01 ms | 0.18 / 0.10 ms | 0.18 / 0.09 ms | 0.18 / 0.09 ms | — |
| 20 instantiations, process peak RSS | 8.5 MiB | 11.0 MiB | 11.0 MiB | 11.0 MiB | — |
| `pane-core` first `get-view` (read and compile, no cache) | 24 ms | 1.03 s | 1.03 s | — | — |
| `pane-core` later action / process peak RSS | 0.02 ms / 15.5 MiB | 0.23 ms / 81 MiB | 0.33 ms / 81 MiB | — | — |

The Wasmtime CLI 49.0.1 ran the same search workload, precompiled, including the 10 ms wait. P3: 18.2 ms and 26.3 MiB per process. Stock: 17.7 ms and 26.2 MiB. The CLI compiled the components in 145–162 ms, because it uses parallel compilation. `pane-core` and the probe host build Wasmtime with `default-features = false`, which drops `parallel-compilation`, so every first open of a JS extension spends about 1 s in single-threaded Cranelift. **This is the largest cost found.** A compiled-artifact cache, or enabling that feature, is a Pane runtime decision and is not made here. The stock/P3 size gap in this run is 7.54 vs 9.92 MB. Stock uses upstream's bundled runtime; the P3 runtime links SDK 34's shared `libc.so` and has no wasm-opt pass. The comparison is not an isolated measure of what WASI 0.3 costs.

## Exact inputs

| Input | Pin |
| --- | --- |
| componentize-qjs | `andreiltd/componentize-qjs` @ `e563c6d6ae50b087980414015663ca9c948c09bb` (v0.4.5, still upstream HEAD on 2026-09-28); codeload tarball sha256 `80effd71…a1e233` |
| Patches | [runtime-port.patch](runtime-port.patch) (the saved Windows port, applies unchanged) + [random-reseed.patch](random-reseed.patch) |
| Runtime compiler | `nightly-2026-09-27` = rustc 1.101.0-nightly `75a75c3e0` (2026-09-26), `-Zbuild-std=std,panic_abort`, target `wasm32-wasip3` |
| wasi-sdk | 34.0 (`wasi-sdk-34.0-x86_64-linux.tar.gz` sha256 `b761e3a0…84b2c4`) |
| Runtime output | `componentize_qjs_runtime.wasm` 1,248,792 bytes, sha256 `a602636e…3fe5b6` (fixed) |
| Componentizer | stable 1.98.1 (repository toolchain), `Cargo.lock` from the pinned commit, `--locked`; embeds Wasmtime/Wizer 47 |
| Host runtime | Wasmtime/wasmtime-wasi 49.0.1 (`pane-core`, probe host); Wasmtime CLI 49.0.1 tarball sha256 `c71f7e0d…abd534` |
| npm | [package-lock.json](package-lock.json): esbuild 0.28.2, fuse.js 7.5.0, zod 4.6.5, typescript 7.0.2, componentize-qjs 0.4.5 (stock control only); Node 24.21.0 |
| Inspection | wasm-tools 1.259.0 |
| Rust guest | `guests/` at this commit: stable 1.98.1, `wasm32-wasip2` target, `wasip3` crate, P3-only imports |

Build times on this machine: runtime 28 s clean (3 s incremental), componentizer 189 s clean, componentize 0.16–0.25 s per guest.

## Reproduction

```sh
export PANE_SCRATCH=/path/outside/repo      # default: <repo>/../pane-scratch
python docs/research/js-backend-validation/reproduce.py --clean
```

`reproduce.py` runs [fetch.py](fetch.py): verified downloads, `npm ci`, and the pinned nightly through rustup. Nothing else is installed globally. It then runs [prepare.py](prepare.py), [build.py](build.py), [componentize.py](componentize.py) and [validate.py](validate.py) for `unfixed`, and requires that run to fail. It repeats them for `fixed`, and finishes with [measure.py](measure.py). The scripts use `pathlib`, `EXE`-suffixed binaries and host-triple asset names, and contain no Windows paths. They were executed only on Linux. On Windows they need `patch` (Git for Windows ships it), and `measure.py` is Linux-only because it uses GNU `time`. The probe host gained `precompile` and `instantiate` modes for the measurements.

## Host and toolchain availability

| Tool | Linux x64/arm64 | macOS x64/arm64 | Windows x64/arm64 |
| --- | --- | --- | --- |
| wasi-sdk 34 | prebuilt (also riscv64) | prebuilt | prebuilt |
| Rust nightly + `rust-src` (wasm32-wasip3 has no prebuilt std; `-Zbuild-std`) | rustup | rustup | rustup |
| Wasmtime 49.0.1 CLI / library | prebuilt / builds | prebuilt / builds | prebuilt / builds |
| wasm-tools 1.259.0 | prebuilt | prebuilt | prebuilt |
| componentize-qjs 0.4.5 npm native binding (stock only) | gnu + musl | yes | msvc |
| esbuild 0.28.2, TypeScript 7.0.2 native | yes | yes | yes |

Linux results are measured here and Windows results in the earlier checkpoint. macOS has not been run. The patched componentizer has no prebuilt binary: it must be compiled (stable Rust) with the P3 runtime (nightly plus wasi-sdk) embedded or passed in. Extension authors would only need a single prebuilt componentizer binary, or an npm package with native bindings, once one is published for each OS. End users need only the built component and Pane.

## Remaining limitations

- Every JS component imports all 20 libc-level P3 interfaces (cli, filesystem, sockets, random, clocks), whatever the source uses. The import list therefore does not show which capabilities an extension actually uses.
- The componentize-time `init` export remains in the final component, as it does upstream. Calling it after snapshotting returns an error, but it is an unneeded export surface.
- WIT u64 maps to `Number` (not BigInt). WASI error variants surface as `Error: [object Object] (see error.payload)`. There are no generated TS types.
- Not checked: cancellation, concurrent calls into one instance, long-run leaks, general npm/Node API compatibility, the u64 range, macOS, and the rest of `hash_seed` hardening.

## Maintenance and upstream strategy

The candidate is **upstream componentize-qjs v0.4.5 plus two patches**: runtime-port.patch (about 30 changed lines) and random-reseed.patch (about 115 added lines). Both are confined to the runtime/core crates, and neither modifies QuickJS itself. Upstream is active (84 PRs; releases about every two weeks; Wasmtime bumps tracked). It has no P3-only output mode, and no issue or PR covers this snapshot state.

Candidates to send upstream, in order of how likely they are to be accepted:

1. **Per-instance reseed of `Math.random`/`performance` after Wizer.** This is an upstream bug that also affects stock output. It is self-contained and has a regression test.
2. **Stop using canonical `context-get/set-0` for the task pointer**, or coordinate slot ownership with wasi-libc. This is required for any libc that uses context slots.
3. **A `wasm32-wasip3` runtime build option.** It would link the SDK's P3 `libc.so`, drop the preview1 adapter reset, skip `add_to_linker_async` for P2 in Wizer, and export `__wasm_library_tls_info`. Upstream builds with stable Rust and wasi-sdk 33 while the P3 target needs nightly `-Zbuild-std`, so this is best proposed as an opt-in feature once the target has a prebuilt std.
4. For quickjs-ng: seed `random_state` from `getentropy` (the source already has `TODO use getrandom()`), and add a public reseed hook. For wasi-sdk: nothing is required. Documenting libc's context-slot and TLS ownership would have saved the TLS debugging.

Without upstream acceptance, Pane would carry a patch queue. That means rebasing two patches on each componentize-qjs release, rebuilding the runtime with a pinned nightly and wasi-sdk, and publishing a per-OS componentizer (runtime embedded) through CI. That is a real ongoing cost, but much smaller than an engine fork. A hard fork would also have to follow Wasmtime/wit-dylib churn, which upstream currently does every release.

## Recommendation (the user decides)

Adopt **pinned upstream componentize-qjs plus a small patch queue** as the JS/TS backend for the native sample (#6), and propose items 1–3 upstream right away. Do not commit to a long-term fork. Re-evaluate if upstream declines P3 support, or if the patch queue grows beyond these integration changes. Alternatives from [js-p3-backend-options.md](../js-p3-backend-options.md) remain untested fallbacks: the `dicej/componentize-js` reboot, and StarlingMonkey plus async bindings. Nothing here required them. Before #6 ships, `pane-core` should handle the roughly 1 s single-threaded first compile, with a cache or parallel compilation.
