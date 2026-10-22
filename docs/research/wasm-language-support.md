# WebAssembly extension languages

**Newer direction:** after the Q32 recap, the user reopened runtime selection to consider wit-bindgen + Wasmtime as the primary architecture. Earlier status below is historical. See [the completed reevaluation](wasm-primary-evaluation.md) and [new TS component experiment](wasm-primary-spike/README.md). Both JS backends were now executed on Windows; a production SDK and cross-platform host remain unimplemented.

Checked 2026-09-27. Documentation/source review only: no language toolchains installed or benchmarked for this report. This examines guest authoring; the launcher still needs a component runtime and implementations of its host APIs.

Product update: the user has accepted JavaScript/TypeScript and Rust authoring at launch; Python and C# may follow. Q18/Q23 subsequently selected managed Node workers for JS/TS and Q19 native Rust helpers. WIT/Wasm remains an evaluated optional future route, not the selected launch runtime or a required security boundary. Prototype priorities and language positions below are historical Wasm research recommendations, not adopted launch implementation choices. See [the interview](../launcher-design-interview.md).

## Assessment

WIT is a plausible shared extension contract for this launcher. It lets us describe typed imports and exports once and generate language bindings. `wit-bindgen` is not a complete SDK, runtime, UI toolkit, package manager, or hot-reload system. Its own repository implements several guest generators and points JavaScript and Python authors to separate projects. Its CLI and 0.x releases explicitly permit breaking changes. Pin the toolchain behind our own SDK commands. [wit-bindgen README](https://github.com/bytecodealliance/wit-bindgen/blob/32bcc3c00bfe987364d1eccd8c89f7d86ef53389/README.md)

**Recommendation:** evaluate one common component format, start with Rust plus a JavaScript path, and label Python/C# experimental until the same conformance and resource tests pass. Keep UI rendering and OS-specific operations in the host. This is an engineering recommendation, not an upstream guarantee.

## Practical support matrix

| Language | Guest build path | Runtime/package implications | Initial product position |
| --- | --- | --- | --- |
| Rust | `wit-bindgen` with `wasm32-wasip2` | Compiles directly; avoids embedding a JS/Python interpreter. Native-only crates still need porting. | First reference implementation. |
| JS/TS | Jco + ComponentizeJS, or the independently maintained componentize-qjs | Embeds a JS engine into the component; not equivalent to running Node. Tool choice affects APIs and async behavior. | First author-facing prototype; measure both paths before selecting. |
| Python | `componentize-py` | Bundles CPython, application and dependencies; native wheels need WASI builds. | Experimental, loaded on demand. |
| C# | `componentize-dotnet`, wrapping `wit-bindgen` and NativeAOT-LLVM | AOT output with required runtime support; not a desktop .NET process with arbitrary assemblies. | Experimental until build/API coverage is verified. |

The build paths above are documented in [wit-bindgen](https://github.com/bytecodealliance/wit-bindgen/blob/32bcc3c00bfe987364d1eccd8c89f7d86ef53389/README.md), [Jco](https://github.com/bytecodealliance/jco/blob/e2f16593fbe99fb05b52e386425b7120ae8d1808/README.md), [componentize-py](https://github.com/bytecodealliance/componentize-py/blob/9ff99dabc05a918a9875bad58b0ecb39cb90bb9a/README.md), and [componentize-dotnet](https://github.com/bytecodealliance/componentize-dotnet/blob/f6bfc5a9a5a25728aeec959be60c8f737278a183/README.md). Runtime qualifications are detailed below; support priorities are our recommendation.

## JavaScript and TypeScript

ComponentizeJS embeds SpiderMonkey through StarlingMonkey. Its README reports an approximately **8 MB embedding** and discusses engine sharing as future work. This is an upstream approximate engine-size statement, **not plugin RSS, download size, minimum total component size, or our measurement**. Snapshotting performs initialization during the build, and optional Weval AOT improves execution. The project calls itself experimental. [ComponentizeJS](https://github.com/bytecodealliance/ComponentizeJS/blob/4d812f5a7b524cea5bcfd565cac55f3fab876a57/README.md)

Its documented APIs are selected web-style APIs, including fetch and timers. The documented async path resolves async exports to synchronous component returns; imports are described as synchronous. Therefore Node filesystem/process/native-addon compatibility must not be inferred from using JavaScript. [ComponentizeJS APIs and async](https://github.com/bytecodealliance/ComponentizeJS/blob/4d812f5a7b524cea5bcfd565cac55f3fab876a57/README.md)

Jco now scaffolds JavaScript/TypeScript projects and defaults its bundled starter worlds to WASI 0.3, with explicit 0.2 selection available. That does not prove every guest backend supports identical async semantics. Jco also points to **componentize-qjs**, which embeds QuickJS, offers size-optimized/synchronous variants, and documents component async functions, streams, futures and cancellation. Evaluate it as another candidate; no comparative size or memory measurement was established here. SpiderMonkey's reported size is not a universal JavaScript floor. [Jco](https://github.com/bytecodealliance/jco/blob/e2f16593fbe99fb05b52e386425b7120ae8d1808/README.md), [componentize-qjs](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/README.md)

## Python

The maintainer explains that componentize-py bundles CPython, Python code and imported dependencies; it does not turn arbitrary Python into small native code. A proposal to import shared Python runtime libraries instead remains open. Historical sizes in that discussion are not treated as current measurements. [Size discussion](https://github.com/bytecodealliance/componentize-py/issues/98), [shared-library proposal](https://github.com/bytecodealliance/componentize-py/issues/28)

The README still requires dependencies needed at runtime to be resolved through top-level imports during building. This can require source changes in libraries that import lazily. [Known limitations](https://github.com/bytecodealliance/componentize-py/blob/9ff99dabc05a918a9875bad58b0ecb39cb90bb9a/README.md)

Native extensions are not categorically impossible: the NumPy example demonstrates one, but requires an **unofficial WASI NumPy build**. Ordinary Windows/macOS/Linux binary wheels are not the demonstrated input. [NumPy example](https://github.com/bytecodealliance/componentize-py/blob/9ff99dabc05a918a9875bad58b0ecb39cb90bb9a/examples/matrix-math/README.md)

Current examples include WASI 0.3 HTTP and TCP, using componentize-py 0.25.0 and Wasmtime 46. The HTTP example performs concurrent downloads and streams hashes. Thus a blanket claim that Python components cannot do async would be incorrect. This is example coverage, not full compatibility with every asyncio library. [HTTP example](https://github.com/bytecodealliance/componentize-py/blob/9ff99dabc05a918a9875bad58b0ecb39cb90bb9a/examples/http-p3/README.md), [TCP example](https://github.com/bytecodealliance/componentize-py/blob/9ff99dabc05a918a9875bad58b0ecb39cb90bb9a/examples/tcp-p3/README.md)

## C#

componentize-dotnet documents .NET 10+ SDK, CLI/library templates, WIT generation and fully AOT-compiled output. It explicitly warns that underlying technologies are developing and missing features. Some troubleshooting examples retain much older versions, so the prose alone is insufficient to pin a working stack. [SDK README](https://github.com/bytecodealliance/componentize-dotnet/blob/f6bfc5a9a5a25728aeec959be60c8f737278a183/README.md)

Its NativeAOT-LLVM dependency is an experimental .NET fork targeting WebAssembly. General Native AOT restrictions include no runtime code generation or dynamic assembly loading and require trimming-compatible dependencies. These are reasons to validate intended libraries; they do not establish exact feature parity for this experimental fork. [NativeAOT-LLVM](https://github.com/dotnet/runtimelab/tree/feature/NativeAOT-LLVM), [Microsoft Native AOT limitations](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/#limitations-of-native-aot-deployment)

## Windows authoring versus portable execution

| Tool | Evidence for author tooling on desktop systems |
| --- | --- |
| ComponentizeJS | CI tests Linux, Windows and macOS using prebuilt engine artifacts. Its workflow explicitly says rebuilding the underlying engine itself does not work on Windows and does that build on Linux. |
| componentize-qjs | Project documents prebuilt CLI archives for Linux, macOS and Windows. |
| componentize-py | Version 0.25.1 publishes Windows x86-64, Linux x86-64/ARM64 and macOS x86-64/ARM64 wheels. No Windows ARM64 wheel is listed. |
| componentize-dotnet | CI builds/tests templates on Windows and Ubuntu with .NET 10. macOS authoring is not established by that CI evidence. |

Sources: [ComponentizeJS CI](https://github.com/bytecodealliance/ComponentizeJS/blob/4d812f5a7b524cea5bcfd565cac55f3fab876a57/.github/workflows/main.yml), [QJS README](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/README.md), [Python release files](https://pypi.org/project/componentize-py/0.25.1/#files), [.NET CI](https://github.com/bytecodealliance/componentize-dotnet/blob/f6bfc5a9a5a25728aeec959be60c8f737278a183/.github/workflows/build.yml).

Authoring support and executing a finished component are separate questions. A portable artifact still needs the launcher's runtime to implement every imported interface. This review does not establish general guest threading support across these paths; async I/O examples do not prove parallel threads.

## Resource and developer-experience acceptance tests

No cross-language RAM, CPU, startup or hot-reload measurements were performed here. Benchmark identical search/result/host-call workloads for Rust, JS, Python and C# separately. Record compressed package bytes, installed bytes, compiler-cache bytes, cold activation, warm query latency, idle CPU, committed memory and resident memory as distinct metrics. Test one active plugin and many active plugins, then disable/reload them and verify reclamation.

Hot reload should initially mean rebuild, validate, replace the instance, reconnect host-owned resources and discard stale results. Persistent data can survive in host storage; arbitrary interpreter heaps cannot be promised to survive. Generated bindings remove repetitive marshaling work, but cancellation, versioning, friendly errors, package installation and developer commands remain our SDK responsibilities. These are proposed design requirements, not features supplied by wit-bindgen.
