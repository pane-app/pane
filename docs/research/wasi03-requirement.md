# WASI 0.3 requirement and toolchain checks

**Subsequent execution evidence:** [Validation checkpoint](wasi03-validation.md) now proves both ordinary Rust std and a patched QuickJS guest using only P3 imports in a P3-only embedded host. Stock QuickJS remains mixed-version. Earlier toolchain observations below are historical, including dated Rust target-book caveats.

2026-09-27. User requirement accepted in [ADR 0013](../adr/0013-require-wasi-03.md). This is a documentation/source audit, not a new execution benchmark.

## Standard and runtime

WASI's release page records 0.3.0 released June 11, 2026 and 0.3.1 August 11, 2026. The 0.3 family uses component-native async functions, streams and futures. It reports final 0.3.0 support in Wasmtime 46 onward and recommends aligning binding and host WIT versions. Do not assume a 0.3-capable runtime converts a 0.2 guest into a 0.3 guest. [Official release documentation](https://wasi.dev/releases/wasi-p3).

The exact previously tested runtime, Wasmtime 49.0.1, has a p3 host module and linker registration. Its source still labels that implementation experimental, incomplete and unsuitable for production, and explicitly warns its documentation can lag. The released standard and implementation/API maturity are separate facts; preserve this discrepancy and validate the pinned implementation rather than repeating old standard-status warnings as current release facts. [Versioned p3 source](https://github.com/bytecodealliance/wasmtime/blob/v49.0.1/crates/wasi/src/p3/mod.rs).

## Rust

The published `wasip3` bindings (observed 0.9.0+wasi-0.3.0) provide 0.3 imports and async support. Their docs describe building with `wasm32-wasip2` while std still uses p2; this is mixed-interface output, not proof of p3-only compatibility. The compiler's p3 target page lists Tier 3 and warns std may still import p2. Those pages contain older dated text, so inspect actual artifacts. Local `rustup target list` currently offers p1/p2, with p2 installed, and no downloadable p3 target. This does not prove p3 bindings are unusable; it does rule out claiming we already have a demonstrated p3-only authoring pipeline. [Bindings](https://docs.rs/wasip3/latest/wasip3/), [compiler target](https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip3.html).

## JS/TS

The installed componentize-qjs 0.4.5 documentation supports async WIT exports, futures and streams with its default async engine. However, examples also import WASI 0.2.12 interfaces. Component-model async support alone does not establish all required WASI 0.3 interfaces. Our prior build explicitly used `sync: true`; its timings cannot validate the required async configuration. [QuickJS documentation](https://github.com/andreiltd/componentize-qjs).

Installed ComponentizeJS 0.23.0 documents Promise exports resolved through synchronous component functions and synchronous imports; our test disabled stdio, clocks, random and HTTP. It did not exercise a WASI 0.3 host. Jco's ability to host/transpile p3 components is distinct from JS guest generation through this backend. [ComponentizeJS](https://github.com/bytecodealliance/ComponentizeJS), [prior test source](wasm-primary-spike/build.mjs).

## Validation required

Use a pinned 0.3 WIT dependency set, inspect actual component imports/exports (including engine/std dependencies), and exercise a real 0.3 async host operation from both Rust and JS/TS. Validate streaming/cancellation/reload and the GPUI bridge separately. Do not accept a version string change, a synchronous pure function, or a p3-capable host alone as proof. Any p2 compatibility adapter/dependency must be surfaced explicitly instead of silently relaxing the user's requirement. Keep old artifacts/results unchanged for reproducibility; repeat resource measurements for the configuration actually chosen.
