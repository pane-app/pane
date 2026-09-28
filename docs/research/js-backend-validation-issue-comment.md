Linux validation of the QuickJS P3-only candidate (write-up: `docs/research/js-backend-validation/README.md`)

**Result: we have a reproducible candidate with regression evidence.** It is upstream componentize-qjs v0.4.5 (`andreiltd/componentize-qjs@e563c6d`) plus two patches to the runtime/core crates. It has not been adopted as a fork; that is still your decision.

**Reproduced on Linux x86_64:** the saved `runtime-port.patch` applies unchanged. It builds with `nightly-2026-09-27` (rustc `75a75c3e0`, `-Zbuild-std`) and wasi-sdk 34, and the componentizer builds with stable 1.98.1 `--locked`. One script, `reproduce.py --clean`, downloads verified archives into a scratch directory, builds both variants, runs the checks and measures. The scripts are portable Python and contain no Windows paths, but they were executed only on Linux.

**Checks** (`validation-fixed.json`, `validation-unfixed.json`):
- Every P3 artifact validates. All 20 WASI imports are `@0.3.0`.
- Fuse.js 7.5.0, Zod 4.6.5, a 10 ms native async clock wait and an async filesystem stream read pass 20 sequential calls in one instance. The missing-file (`no-entry`) and no-preopen cases pass.
- Control: stock componentize-qjs output (18 `@0.2.12` imports) is rejected by the P3-only host. `pane-core` rejects it with `CallError::Incompatible`.
- **JS (`sample.js`) and TS (`sample.ts`) guests implementing `wit/extension.wit` run through `pane_core::Runtime`**: get-view, greet, a 50 ms WASI clock wait, and an unknown item returning a guest error. They use a new `crates/pane-core/examples/run_guest.rs` and the public API only. `cargo xtask ci` passes.

**Math.random defect:** reproduced. 5 fresh instances all returned `0.4491794857419842`. QuickJS seeds `ctx->random_state` when the context is created, which happens inside the Wizer snapshot at build time. Stock componentize-qjs has the same bug, so the P3 port did not cause it. The same snapshot also broke `performance.now()`, which returned about −115 ms because the time origin came from the build machine's clock. `random-reseed.patch` installs native `Math.random` and `performance` replacements before user code runs. It reseeds them from `getentropy` (backed by `wasi:random@0.3.0`) on the first entry after restore. The regression checks fail before this patch (3 checks) and pass after. Still captured at build time: module top-level values, and QuickJS's Map `hash_seed`, which cannot safely be reseeded.

**Costs (Linux, fixed build):**
- JS/TS sample components are 4.0 MB (1.4 MB gzipped); the Rust sample is 40 KB. The Fuse/Zod component is 9.9 MB; stock is 7.5 MB.
- Deserializing a precompiled component takes about 0.9 ms. Instantiation takes about 0.1–0.2 ms.
- The largest cost: `pane-core`'s first `get-view` takes **about 1.03 s**, because Wasmtime is built without `parallel-compilation` and there is no compiled-artifact cache. The Wasmtime CLI compiles the same component in about 150 ms. Peak process memory is 81 MiB vs 15.5 MiB for Rust.

**Tool availability:** wasi-sdk 34, Wasmtime 49.0.1, wasm-tools, esbuild and TypeScript all have prebuilt binaries for Linux, macOS and Windows, and the nightly comes through rustup. The patched componentizer has no prebuilt binary; it needs Rust and must be built per OS. macOS has not been run.

**Limitations:**
- Every JS component imports all libc-level P3 interfaces (filesystem, sockets and so on), so its import list does not show what capabilities it uses.
- The final component still carries the componentize-time `init` export.
- u64 maps to `Number`. WASI errors show as `[object Object]` with details in `.payload`. There are no generated TS types.
- Not tested: cancellation and concurrency.

**Recommendation (your decision):** use pinned upstream componentize-qjs plus the two-patch queue for #6. Offer upstream (1) the per-instance reseed fix, (2) not using context slot 0 for the task pointer, and (3) an opt-in `wasm32-wasip3` runtime build. Do not start a long-term fork. Separately, `pane-core` should cache compiled artifacts or enable parallel compilation before JS extensions ship.
