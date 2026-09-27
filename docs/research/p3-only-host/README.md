# Embedded Wasmtime with only P3 registered

2026-09-28. A bounded executable host using Wasmtime/wasmtime-wasi 49.0.1. `wasmtime-wasi` is built with only its `p3` feature; the linker registers only P3 and does not stub unknown imports. This makes rejection of P2 dependencies an execution check in addition to WIT inspection.

Passed:

- The [ordinary Rust std probe](../p3-std-spike/README.md) ran its filesystem, clock, environment, allocation and diagnostic checks.
- Stock QuickJS's mixed component failed instantiation on `wasi:cli/environment@0.2.12`, as expected. [Positive/negative control evidence](initial-results.json).
- The [patched P3 QuickJS guest](../qjs-p3-port-spike/README.md) performed 20 sequential async/library/file-stream calls within one instance, plus separate missing-file and no-preopen cases. [Results](../qjs-p3-port-spike/host-results.json).

`src/main.rs` creates an Engine, Component, Store and Linker and invokes either a P3 command or an async string query. It is a debug test executable, not the launcher helper, IPC protocol, crash supervisor or production SDK. Its timings and executable size should not be used as release resource budgets. Sequential same-instance calls do not establish concurrent-call isolation, cancellation, guest replacement, or hot reload.

Build with Cargo using a target directory in scratch storage, as recorded by [the host build helper](../qjs-p3-port-spike/build-hosts.py). The executable is `<scratch>/host-target/debug/p3-only-host.exe`. Argument forms:

```text
p3-only-host.exe component.wasm command unused fixture-directory
p3-only-host.exe component.wasm query io fixture-directory
p3-only-host.exe component.wasm queries io fixture-directory 20
```

The argument layout is deliberately a probe convenience, not a proposed user-facing CLI.
