# Rust WASI 0.3 executable probe

This probe passed on Windows on 2026-09-27: Rust compiled a component whose seven imports are all `@0.3.0`, and Wasmtime 49.0.1 executed it. It uses a real third-party library, `serde_json`, after reading JSON through WASI 0.3 filesystem streams. See [result.json](result.json), the extracted [component.wit](component.wit), and actual guest output [view.json](view.json).

The compiler target is `wasm32-wasip2` because that is the installed target on Rust 1.97.0. **The emitted component is P3-only**, verified from its imports. This works by using `#![no_std]`, `alloc`, `wasip3` with its `std` feature disabled, and a Wasm allocator. This target name must not be mistaken for the component's actual ABI.

## What ran

1. `wasi:clocks/monotonic-clock@0.3.0.wait-for`, a native async import, awaited for 1 ms.
2. `wasi:filesystem/preopens@0.3.0` obtained the explicitly granted directory.
3. Async `descriptor.open-at` opened `fixture.json`; `read-via-stream` supplied its bytes through a Component Model stream and completion future.
4. `serde_json` parsed the bytes, checked the item ID, and serialized a launcher view.
5. `wasi:cli/stdout@0.3.0.write-via-stream` emitted the JSON through a stream and completion future.

The exported entrypoint is async `wasi:cli/run@0.3.0.run`. Execution with the directory grant exited 0. The same component without any directory grant exited 1 and emitted no JSON. This tests withholding the directory capability; it is not a comprehensive filesystem escape audit. `wasm-tools validate --features all` succeeded. Binary size was 115,377 bytes; its SHA-256 is recorded in `result.json`.

## Reproduce

Prerequisites: Rust 1.97.0 with `wasm32-wasip2` installed, Wasmtime 49.0.1 CLI, and wasm-tools 1.259.0. No custom toolchain, adapter, or global configuration is required. From PowerShell:

```powershell
./docs/research/wasi03-rust-spike/run.ps1 `
  -Wasmtime 'C:/path/to/wasmtime.exe' `
  -WasmTools 'C:/path/to/wasm-tools.exe'
```

The script builds with the checked-in Cargo lockfile into a temporary target directory, validates the component, rejects any import outside `@0.3.0`, executes both grant/no-grant cases, and updates the evidence files. Equivalent core commands:

```powershell
cargo build --release --locked --target wasm32-wasip2 --manifest-path docs/research/wasi03-rust-spike/Cargo.toml --target-dir "$env:TEMP/kyoko-wasi03-rust-target"
wasm-tools component wit "$env:TEMP/kyoko-wasi03-rust-target/wasm32-wasip2/release/wasi03_rust_spike.wasm"
wasmtime run -S p3 --dir docs/research/wasi03-rust-spike "$env:TEMP/kyoko-wasi03-rust-target/wasm32-wasip2/release/wasi03_rust_spike.wasm"
```

## Scope and remaining work

This proves a P3-only Rust guest with async clocks, filesystem streams, and a portable allocation-capable library. It does **not** establish support for arbitrary Rust libraries, `std::fs`, Tokio, native libraries, threading, or all of `std`. The wasip3 documentation explicitly says that using Rust's standard library on the `wasm32-wasip2` target uses P2; simply replacing a few calls with `wasip3` would produce mixed imports. P3-only guest SDK policy should reject such imports during packaging.

Without `std`, this installed compiler/bindings combination needed explicit `memcmp` and canonical-ABI `cabi_realloc` definitions, included in [src/lib.rs](src/lib.rs). The allocator traps on allocation failure and the panic handler traps; this is probe scaffolding, not a polished plugin SDK. Filesystem input is a bounded local fixture for this experiment; production guests need limits or incremental reading rather than unbounded stream collection.

Execution here uses the Wasmtime CLI. It does not itself prove a custom embedded Wasmtime host, guest cancellation, repeated in-process calls, or a shared application command protocol. The JSON output can feed the separate GPUI probe. The clock test proves successful suspension/resumption through an async import, not a timing benchmark.

## Primary references

- [wasip3 crate documentation](https://docs.rs/wasip3/0.9.0/wasip3/): generated P3 bindings and the P2-target/std distinction. The exact downloaded source is `wasip3 0.9.0+wasi-0.3.0`; its dependency is `wit-bindgen 0.62.0` and it supports disabling `std`.
- [Official wasi-rs CLI example](https://github.com/bytecodealliance/wasi-rs/blob/main/crates/wasip3/examples/cli-command.rs): async command export and stream-based stdout API.
- [WASI 0.3 filesystem interfaces in Wasmtime 49.0.1](https://github.com/bytecodealliance/wasmtime/blob/v49.0.1/crates/wasi/src/p3/wit/deps/filesystem.wit): async descriptor operations and stream/future transfers.
- [WASI 0.3 clock interfaces in Wasmtime 49.0.1](https://github.com/bytecodealliance/wasmtime/blob/v49.0.1/crates/wasi/src/p3/wit/deps/clocks.wit): async clock imports.
- [serde_json no_std documentation](https://docs.rs/serde_json/1.0.145/serde_json/#no-std-support): `default-features = false` plus `alloc`.

Toolchain documentation online reflects several different transition states; the proof here relies on the pinned crate source, actual local compiler, extracted component interface, and actual execution rather than assuming that a target name guarantees P3.
