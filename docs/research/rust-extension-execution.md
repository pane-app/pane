# Rust extension execution: options and recommendation

Researched 2026-09-27. The user subsequently accepted the recommended native executable route with "ok then"; see [ADR 0007](../adr/0007-native-rust-extension-processes.md). This follows Q18's JS/TS runtime comparison and the user's question about Rust. No new application or native-plugin benchmark was run.

## Options

| Route | Useful property | Main cost |
|---|---|---|
| Native executable communicating with the launcher | Normal native Rust libraries and OS access; restart the executable on reload | Per-active-process overhead, IPC, and platform-specific distribution |
| Rust compiled to a WIT/Wasm component | Typed component boundary and portable guest artifact for a compatible host contract | Host runtime/import implementation; native-only crates and OS APIs may need adaptation |
| Native dynamic library loaded into the host | Direct in-process calls with a deliberately designed ABI | ABI/ownership compatibility and unloading callbacks/resources during reload |
| Rust Node add-on via Node-API | Rust implementation callable from JS; useful mixed-language package | Coupling to the JS extension host and native-addon packaging/lifecycle |

These are alternatives, not an obligation to ship four execution modes. [Nushell and Zed precedents](rust-plugin-precedents.md) illustrate executable and Wasm approaches. [NAPI-RS](https://napi.rs/) is a framework for Rust Node add-ons, not an independent launcher extension protocol.

Rust's ordinary ABI provides no stability guarantee. Loading a dynamic library with a C entry point can define a controlled boundary, but does not make arbitrary Rust structs, GPUI entities or closures stable across independently built packages. Unloading also requires ensuring no callbacks, threads or resources retain code pointers into the old library. [Rust ABI reference](https://doc.rust-lang.org/reference/items/external-blocks.html#abi)

The `wasm32-wasip2` Rust target produces components for WASI 0.2 hosts; this is a distinct target from a native desktop process. Our earlier Rust component smoke test establishes basic invocation, not unrestricted native-crate compatibility or GPUI UI support. [Rust target documentation](https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip2.html), [prior measurements](wasm-extension-feasibility.md)

## Accepted initial path

Provide a first-class Rust SDK and execute a Rust extension as a native helper process on demand, with an explicit background lifetime where needed. The compiled binary uses the same versioned launcher concepts as the JS/TS SDK: commands, query results, views, events, storage and lifecycle. It communicates directly with the launcher and does not need Node merely because JS extensions use Node.

Developers compile the package for supported OS/architecture targets. Normal users receive a matching prebuilt executable and packaged/managed runtime dependencies; they do not install rustc or Cargo. This satisfies the accepted no-manual-toolchain requirement only when the complete target package is available. A native Rust program can still depend on OS or third-party shared libraries, so packaging must verify those dependencies rather than assuming every Rust binary is fully static.

During development, build a new artifact while the old instance continues running. If the build fails, show diagnostics and retain the old instance. If it succeeds, clean up/stop the old instance and launch the replacement, restoring saved or explicitly exported state. Use separate versioned artifact paths where required to avoid overwriting an executing file, especially on Windows. Startup failure and storage migration recovery remain open; this is not universal rollback.

GPUI CE renders the UI in the main application. Rust authors use Rust helpers to describe controls, custom layout/drawing and respond to events. Arbitrary native GPUI objects cannot simply be sent between processes; the UI protocol must expose enough functionality, including custom views, to satisfy the accepted capability requirement. Verify the same representative custom interaction from Rust and JS/TS before committing to the protocol. See [UI boundary proposal](gpui-extension-bridge.md).

## Trade-offs to measure

The process path preserves native library access and gives an explicit restart boundary. Its costs are startup, resident memory for active helpers and data/event transfer. Batch view updates and keep host rendering responsive; do not start a process per keystroke or per command call when an extension is already active. Installed-but-unused extensions need not run. Idle retention policy and process pooling remain open.

WIT/Wasm remains a valid candidate for suitable portable extensions, particularly if measurement demonstrates a worthwhile benefit. The accepted capability-first priority argues against making every Rust extension fit a Wasm-only host API without evidence. No native-versus-Wasm production performance winner is established by this research.
