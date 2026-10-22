# WIT + Wasmtime as the primary desktop extension host

Date: 2026-09-27. The user reopened the runtime decision and asked to evaluate wit-bindgen + Wasmtime instead of the previously selected Node/native helpers. This is a feasibility review and proposed prototype direction, not a claim that the replacement is implemented or validated. No installs or execution were performed for this note. The live Wasmtime API pages identify themselves as **50.0.0-dev**; pin and check the intended released version before implementation. The earlier local experiment used 49.0.1.

## Finding

**Feasible, including powerful trusted extensions, but we must build the desktop host API.** Wasmtime lets a Rust embedder link custom typed host functions into components. The host implementation can use ordinary native Rust libraries and OS services. Consequently, file access, subprocesses, clipboard and desktop automation can be exposed to trusted components without adopting a permission-prompt system. This is an architectural inference from the embedding API, not evidence that those launcher APIs already exist. [Host functions](https://docs.wasmtime.dev/api/wasmtime/component/struct.LinkerInstance.html#method.func_wrap)

WASI's inspected CLI world includes filesystem, sockets, clocks, randomness, I/O and CLI interfaces; it does not supply the launcher's desktop UI or subprocess-spawning contract. Wasmtime supports configuring inherited environment/network and preopened directories, but those settings still need host configuration. A trusted extension is not automatically a native process with every OS API available. [WASI imports](https://raw.githubusercontent.com/WebAssembly/wasi-cli/main/wit/imports.wit), [WASI context](https://docs.wasmtime.dev/api/wasmtime_wasi/struct.WasiCtxBuilder.html)

Proposed broad subprocess import: executable, argument list, environment, working directory, streamed output, wait/cancel. This lets authors ship a native helper for an integration missing from WASI. The helper still needs binaries for each OS/architecture; its runtime/resources count toward the extension. A WIT wrapper does not make native dependencies portable or eliminate their packaging work. No mandatory native-helper fallback has been selected.

## GPUI boundary

WIT transports typed data and resources represented by handles. GPUI CE's `Element` API uses Rust application/window context and host rendering hooks. Keep those objects in the GPUI process; expose view descriptions, events and resource handles through our API. The native Rust host interprets those requests and renders them. Generated bindings do not turn GPUI Rust traits into a desktop SDK. [WIT resources](https://component-model.bytecodealliance.org/design/wit.html#resources), [GPUI Element source](https://github.com/gpui-ce/gpui-ce/blob/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/crates/gpui/src/element.rs)

A standard list/form API is a plausible starting point; the accepted custom-view requirement also needs layout/drawing/input primitives and a working custom interactive example. Keep keyboard focus, text editing/IME and accessibility native. Never wait for guest execution on the render thread. These are proposed launcher mechanics, not upstream automatic features. See the existing [UI boundary note](gpui-extension-bridge.md).

## Placement, interruption and reload

Recommended prototype shape:

```text
GPUI CE UI process
  <-> launcher-owned messages/events
shared extension helper embedding Wasmtime
  -> one shared Engine
  -> one Store per active extension generation
  -> linked WASI services + our native host imports
```

Wasmtime's Engine is shareable and cheaply cloned. Stores own instances until the Store is dropped, so repeated reloads must replace the Store, not continually instantiate into one immortal Store. This supports lazy activation and per-extension reload. [Engine](https://docs.wasmtime.dev/api/wasmtime/struct.Engine.html), [Store](https://docs.wasmtime.dev/api/wasmtime/struct.Store.html)

Embedding directly in the UI process is possible and avoids IPC, but native host callbacks can abort that process; Wasmtime documents stack-exhaustion examples. A separate shared helper is therefore the preferred experiment for the already accepted goal that extension failures leave the launcher usable. Helper aborts can still disrupt every component in it. This process split is a reliability proposal, not a security sandbox or a measured resource improvement. Ordinary guest traps are reported as invocation errors. [Native stack caveat](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html#method.max_wasm_stack), [Component call errors](https://docs.wasmtime.dev/api/wasmtime/component/struct.TypedFunc.html#method.call)

Epoch interruption/fuel can stop running guest code; neither interrupts a blocked native host call. Host I/O must use cancellation-aware asynchronous implementations. Disable/reload should retire an extension generation, cancel owned tasks, remove views/subscriptions and drop its Store. Host-owned resources need their own cleanup: dropping a Rust `Child`, for example, does not terminate its process. Arbitrary detached side effects remain outside guaranteed cleanup, consistent with the trusted-extension policy. [Interruption limitations](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html#method.epoch_interruption), [Child lifetime](https://doc.rust-lang.org/std/process/struct.Child.html)

Dropping a Store releases its instance allocation, but it does not promise that process working set immediately returns to baseline. Shared compiled-code caches and allocator retention require measurement; Wasmtime's pooling allocator explicitly supports keeping freed slots resident for reuse. Bound cache lifetime and compare reload plateaus. [Pooling residency](https://docs.wasmtime.dev/api/wasmtime/struct.PoolingAllocationConfig.html#method.linear_memory_keep_resident)

## Prototype acceptance criteria

These are proposed checks, not completed results or new product promises:

1. Rust and JS/TS guests call the same host storage, network and subprocess APIs and return structured results. Verify a real dependency in each language; test convenience APIs rather than only arithmetic.
2. Render one list, one editable form and one custom interactive view through GPUI; keep typing/focus responsive during guest computation and pending I/O.
3. Preserve the working generation on build failure. Replace only the affected extension after successful build; startup failure shows logs/Retry and does not roll back automatically, as accepted in Q31.
4. Reload/disable during a request and child process; reject stale results and demonstrate cleanup of managed tasks/resources. Run 100 reloads and inspect total process-tree memory and handle counts for continuing growth.
5. Trap, infinite-loop and blocked-I/O cases leave UI responsive; deliberate helper failure leaves UI running and reports disrupted extensions. Prove deadline/cancellation behavior rather than inferring it from Wasm isolation.
6. Measure installed size, cold/warm activation, idle CPU, steady-state p50/p95 command latency and total memory with 1 and 10 active representative guests. Compare equivalent persistent Node/native execution. The earlier fresh-process CLI toy timings do not establish these results.
7. Repeat functionality on Windows, macOS and the selected Linux environments before claiming cross-platform parity. Test author compilation separately from end-user installation, which must require no manually installed toolchains.

Recommendation: continue the WIT/Wasmtime primary-runtime experiment. Its host architecture can satisfy the selected product model. The decision should turn on JS/TS library compatibility, authoring experience and measured resources, with native host API coverage made explicit; sandboxing is not a reason to reject or adopt it.
