# QuickJS WASI 0.3 migration: source-level scope

**Later executable result:** [The direct port now runs](qjs-p3-port-spike/README.md), including real libraries/async/I/O under a P3-only host. The source investigation below predates those tests; its "no port built" statements are historical. The port still needs initialization/lifecycle work and has measured size/memory cost.

2026-09-28. Bounded source investigation following the [executable validation](wasi03-validation.md). No runtime port was built during this investigation. The tested stock JS component still fails the requirement because it imports 18 WASI 0.2 interfaces. JS/TS and Rust, native component async, real plugin capabilities, and WASI 0.3 remain required.

## Finding and recommendation

A direct P3 runtime rebuild is the most promising next experiment. Current upstream libc and Rust source provide more of the necessary foundation than older target documentation suggests. This is an integration hypothesis, not a successful build or a commitment to maintain an engine fork. First prove the compiler/libc combination on a small ordinary-std guest, then try the QuickJS runtime. Do not start by writing a general P2-to-P3 compatibility layer.

The [alternative-backend audit](js-p3-backend-options.md) found no ready replacement among the inspected Jco, ComponentizeJS, StarlingMonkey, reboot and official preview1-adapter paths. This does not establish that no third-party solution exists.

## Why the current component carries P2

Inspected componentize-qjs commit: `e563c6d6ae50b087980414015663ca9c948c09bb`, version 0.4.5, cloned to `%TEMP%/kyoko-qjs-p3-audit`.

| Boundary | Actual source | Required investigation |
| --- | --- | --- |
| Runtime build | Hardcodes `wasm32-wasip2`, wasi-sdk 33, and shared/PIC runtime compilation. | Coordinate Rust std, C compilation, libc, linker and component metadata for P3. Replacing a target string alone is not evidence. |
| Rust embedding | Enables rquickjs `std`; uses allocation, collections and other std facilities. | Verify a rebuilt P3 std runtime, rather than assuming the existing no_std Rust probe covers this engine. |
| Component assembly | Links runtime plus generated `wit-dylib` and explicitly registers the preview1 reactor adapter. | Inspect final dependencies after rebuilding; eliminate actual legacy requirements before removing their adapter. |
| Snapshot startup | Calls `wasi_snapshot_preview1.reset_adapter_state` and `__wasilibc_reset_preopens` after evaluating JS. | Replace the legacy reset dependency and preserve correct state cleanup across snapshot and runtime. Simply deleting reset calls risks stale state. |
| Build-time host | Wizer installs both P2 and P3 linkers. | Test initialization and the final guest with P3-only facilities. A build host accepting both versions can hide remaining dependencies. |

Sources: [runtime build](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/crates/core/build.rs), [runtime manifest](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/crates/runtime/Cargo.toml), [assembly and Wizer](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/crates/core/src/lib.rs), [initialization](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/crates/runtime/src/lib.rs), [reset import](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/crates/runtime/src/abi.rs).

The componentizer accepts custom runtime bytes, providing a useful seam for a prototype. Its separate `stubWasi` option traps every `wasi:` import, including desired P3 imports; it is not a migration mechanism. [Options and runtime selection](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/crates/core/src/lib.rs), [stub implementation](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/crates/core/src/stubwasi.rs).

## New upstream evidence supporting a direct rebuild

**wasi-libc has concrete P3 implementations.** At audited commit `06513b9ae0c1b14ca3010924939c007ed27628a1`, CMake selects separate P3 sources and metadata; its P3 WIT includes 0.3.0 CLI, clocks, filesystem, HTTP, random and sockets. Its blocking support waits on P3 waitable sets and handles completion/cancellation. These are actual source implementations, not a README-only target claim. This does not prove their integration with QuickJS. [Build selection](https://github.com/WebAssembly/wasi-libc/blob/06513b9ae0c1b14ca3010924939c007ed27628a1/libc-bottom-half/CMakeLists.txt), [world](https://github.com/WebAssembly/wasi-libc/blob/06513b9ae0c1b14ca3010924939c007ed27628a1/wasi/p3/wit/wasi-libc.wit), [blocking implementation](https://github.com/WebAssembly/wasi-libc/blob/06513b9ae0c1b14ca3010924939c007ed27628a1/libc-bottom-half/sources/wasip3_block_on.c).

**wasi-sdk 34 is released.** The official release is dated 2026-08-25; current SDK documentation lists `wasm32-wasip3`. GitHub release metadata reports a 619,003,408-byte Windows x64 SDK archive. That is author/CI build tooling, not proposed launcher installer size. No SDK archive was downloaded for this review. [Release](https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-34), [SDK targets](https://github.com/WebAssembly/wasi-sdk#supported-targets), [release asset metadata](https://api.github.com/repos/WebAssembly/wasi-sdk/releases/tags/wasi-sdk-34).

**Rust 1.97 source has P3-specific std wiring.** Its std manifest selects `wasip3 0.6.0` for `target_env = "p3"`; random uses the corresponding P3 bindings. Its filesystem implementation routes WASI through the Unix/libc implementation. This is stronger current evidence than the target book's dated 2025 transition notes, but does not prove every std path or linked C library is P3-only. [Manifest](https://github.com/rust-lang/rust/blob/1.97.0/library/std/Cargo.toml), [random](https://github.com/rust-lang/rust/blob/1.97.0/library/std/src/sys/random/wasi.rs), [filesystem selection](https://github.com/rust-lang/rust/blob/1.97.0/library/std/src/sys/fs/mod.rs), [target documentation with dated caveats](https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip3.html).

Local `rustc --print target-list` includes `wasm32-wasip3`, but `rustup target list` offers no prebuilt P3 standard library for the active toolchain. A source-built std/toolchain setup is therefore part of the next experiment; `rustup target add` is not an established solution. The exact Rust/LLVM/libc/component-ABI versions need to be checked together.

rquickjs 0.13 exposes a no_std-capable feature layout and a Rust-allocation option, but its QuickJS C build still needs deliberate libc/toolchain handling. A no_std embedding port is a fallback investigation, not a free flag that removes all C runtime dependencies. [Core features](https://github.com/DelSkayn/rquickjs/blob/v0.13.0/core/Cargo.toml), [C build](https://github.com/DelSkayn/rquickjs/blob/v0.13.0/sys/build.rs).

## Bounded implementation sequence

1. Build a small Rust std + P3-libc guest with allocation, Date-equivalent wall time, randomness, diagnostics and file I/O; inspect the complete final import graph and execute it with only P3 linked. Stop here if toolchain/ABI compatibility is unresolved.
2. Rebuild QuickJS's existing runtime through the same toolchain; retain its working native-async bindings. Adjust component assembly and snapshot reset only as required by observed imports/state.
3. Re-run Fuse/Zod and real async-clock checks, then add runtime-sensitive Date/random/diagnostic/I/O checks and initialization-error cases. Inspect imports again after snapshotting. Validate resource release/cancellation before claiming lifecycle support.
4. Measure component/cache size, build time, cold compilation, and a persistent host's idle/active memory. Existing mixed-version CLI numbers cannot predict port overhead or launcher resource use.

This work could reduce to a manageable toolchain/linking patch, or expose a larger runtime/ABI port. Source review cannot honestly assign a reliable schedule yet. Prefer a small upstreamable patch if feasible. A substantial long-lived engine fork would be a separate architectural tradeoff to discuss with the user, not silently adopted here. End users would still receive built artifacts; extra author/CI tooling does not imply manual runtime installation.
