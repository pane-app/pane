# WASI 0.3, libraries and GPUI: validation checkpoint

2026-09-27 to 2026-09-28 (local date rollover). Bounded prototype authorized after Q33/Q34 acceptance and Q35 catalog deferral. These are throwaway experiments, not launcher implementation or a completed extension SDK.

## Runtime findings

| Probe | Actual evidence | Implication |
| --- | --- | --- |
| Rust + `serde_json` | 115,377-byte component; all seven imports use WASI 0.3.0. Actual async clock, file open, streamed read/completion future and streamed stdout pass. | P3-only Rust is feasible with this tested `no_std + alloc` setup. |
| TypeScript + Fuse.js + Zod | Real p3 async clock call, fuzzy matching and validation pass. Final component also imports 18 p2 interfaces from stock QuickJS. | Real library and async functionality work, but this JS configuration fails the p3-only requirement. |
| Rust std, pinned nightly + SDK 34 | 197,696-byte component, 17 P3-only imports; real file read/write/delete, environment, clocks, sleep and diagnostics pass in a P3-only host. | Ordinary std is feasible with the tested source-built toolchain, beyond the earlier no_std proof. |
| Patched QuickJS + P3 libc | 20 P3-only imports; Fuse/Zod/native async pass. Expanded guest passes 20 sequential same-instance async file-stream calls plus missing-file/no-preopen cases in a P3-only host. | The P2 dependency blocker is overcome in a local prototype; runtime initialization and lifecycle work remain. |

[Rust source, interface and reproduction](wasi03-rust-spike/README.md), [JS source, interface and reproduction](wasi03-js-spike/README.md).

The Rust compiler target still says `wasm32-wasip2`; the actual component interface is p3-only because the probe avoids std and supplies allocation/ABI shims. Do not claim every crate can use this route unchanged. Conversely, an engine capable of component async is not automatically a p3-only engine: the JS output's explicit import graph is the evidence.

The JS component is 6.67 MiB; the compiled cache is 9.23 MiB. Three fresh cached CLI invocations had a median peak working set of 30.68 MiB, with a deliberate 10 ms guest clock wait. First compilation peaked at about 109.68 MiB. These are small-sample CLI checks, not production/persistent host measurements or a fair comparison to the older synchronous dependency-free query. The componentizer also trapped on a JS BigInt passed to its u64 lowering; this test uses an exactly representable Number. Full integer mapping needs validation.

## Patched runtime status

**Follow-up execution, 2026-09-28:** the [QuickJS port](qjs-p3-port-spike/README.md) and [Rust std build](p3-std-spike/README.md) now pass under an [embedded P3-only host](p3-only-host/README.md). The host also rejects the stock mixed JS guest. The port needed P3 compilation/linking, real libc inclusion, TLS metadata export and a fix for QuickJS's collision with libc's context-slot bookkeeping. A 92-line source diff plus build flags records the prototype; it is not a finalized production fork or SDK.

Stock QuickJS still uses a preview1-to-P2 adapter. Its universal WASI trap-stubbing option was not used to achieve the port. The accepted P3 requirement and JS/TS/Rust launch languages remain intact.

The port's current costs are measurable: the same search workload grows from 6.67 to 9.44 MiB, and median peak cached CLI working set rises from 30.71 to 36.81 MiB in a new three-run comparison. Median process timing remains roughly 47 ms including a 10 ms wait. Different runtime/libc optimization settings prevent interpreting that difference as inherent P3 overhead. [Measurements](qjs-p3-port-spike/measurements.json).

One initialization defect remains: fresh instances replay the same snapshotted Math.random state. Date and file input are evaluated at runtime. Reseeding/SDK random integration needs a fix and validation. General Node/native-addon, HTTP/client library, Python/C# and Tokio compatibility are still unproven. Source-only packages need author build tools; supported prebuilt runtime artifacts can still avoid normal-user toolchain installation.

## GPUI connection

The separate [GPUI probe](wasi03-gpui-spike/README.md) built and passed native Windows rendering/reload/click checks at the selected GPUI CE commit. It displayed the actual TS guest result, emitted `calculator` on selection, reloaded the actual Rust guest result, and emitted `rust-p3`. [Recorded results](wasi03-gpui-spike/evidence/result.json), [initial screenshot](wasi03-gpui-spike/evidence/initial.png), [reloaded screenshot](wasi03-gpui-spike/evidence/reloaded.png). Both screenshots were visually inspected, and the test window/process was closed. Automation initially failed to discover the HWND by title; enumeration by process ID resolved that harness issue. JSON file handoff is a deliberately small boundary experiment, not live embedded Wasmtime or the proposed production helper IPC; selection events were recorded, not dispatched back into guest code.

## Scope left open

Embedded Wasmtime now runs multiple sequential calls in one guest instance. Concurrent calls/cancellation, lifecycle reload/disable cleanup, production helper IPC, the async view/event protocol, text input/IME, custom drawing, sustained resource budgets and non-Windows execution remain unvalidated. Updating a JSON view is not proof of guest hot reload. Next: fix snapshot initialization, validate lifecycle/cancellation, then optimize size and measure persistent-host resources.
