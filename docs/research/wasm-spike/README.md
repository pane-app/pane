# Throwaway WIT component smoke test

**Historical experiment; not WASI 0.3 validation.** The user subsequently required p3. This Rust build used the p2 target and a synchronous query. See [requirement audit](../wasi03-requirement.md).

Question: can a tiny Rust extension expose a typed search API as a WebAssembly component on this Windows machine, and what does a fresh CLI invocation cost?

Result: yes. `query("calc")` returns `[{id: "calculator", title: "Calculator"}]` through Wasmtime. This is an exported function with a string argument and a list-of-records return; no launcher host imports, UI, installation, lifecycle or hot reload were implemented.

## Reproduction

Tested: Rust 1.97.0, wit-bindgen 0.62.0, Wasmtime 49.0.1, Node 24.18.0, Python 3.12.10, Windows x64. Cargo.lock records dependencies. The Rust WASI target was added using `rustup target add wasm32-wasip2`.

The official Wasmtime Windows zip was downloaded from its [49.0.1 release](https://github.com/bytecodealliance/wasmtime/releases/tag/v49.0.1) and extracted into a unique temporary directory. No Wasmtime installer or global PATH change was used. Build outputs are also in that directory; `../wasm-spike-local.json` records its location and must be adjusted if reproducing on another machine.

From the workspace root in PowerShell:

```powershell
$spikeMetadata = Get-Content docs/research/wasm-spike-local.json -Raw | ConvertFrom-Json
$env:CARGO_TARGET_DIR = Join-Path $spikeMetadata.root 'target'
cargo build --locked --release --target wasm32-wasip2 --manifest-path docs/research/wasm-spike/Cargo.toml
python docs/research/wasm-spike/measure.py
```

`measure.py` creates a locally trusted precompiled artifact, performs one initial invocation per mode, then interleaves 15 fresh processes per mode using a fixed random order. It verifies the expected result of every sampled invocation. The process memory counter is Windows `PeakWorkingSetSize`, read through `GetProcessMemoryInfo`; elapsed time includes process startup, execution, output collection and shutdown. Wasm compilation caching is explicitly disabled in the compile-each-process mode.

## Observed results

| Mode | Median elapsed time | Median process peak working set |
|---|---:|---:|
| Wasmtime, compiling component each process | 33.73 ms | 28.91 MiB |
| Wasmtime, locally precompiled component | 19.50 ms | 14.29 MiB |
| Node, equivalent toy query | 47.06 ms | 56.84 MiB |

Sizes: Rust component 63,347 bytes; local precompiled component 192,360 bytes; full Wasmtime CLI executable 41,009,152 bytes; downloaded runtime zip 13,327,962 bytes. Initial Rust release build reported 31.58 seconds including dependency setup; one Wasmtime precompilation took about 39.77 ms including CLI startup.

Raw samples, first invocations and versions are in [results.json](results.json).

These are fresh-process measurements with potentially warm filesystem/OS caches, not cold-machine startup measurements or steady-state embedded runtime costs. The Node script does not run inside Wasm. No conclusion about JS/Python/C# component overhead, multi-extension memory, native Rust performance, UI memory, sustained CPU, or full launcher speed follows from this toy test. A persistent Node host would amortize startup differently. The full CLI executable size is not the minimum size of an embedded Wasmtime library.
