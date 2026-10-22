# Node compatibility for launcher extensions

Researched 2026-09-27 for Q18. The user already chose trusted, capability-first extensions; this question concerns runtime/API compatibility, not reopening permissions. Source/documentation review only; no runtime compatibility or performance benchmark was performed.

## Raycast

Raycast runs extensions in a managed Node child process, with extension workers and communication to the native host. Its documented model does not further restrict filesystem, network or other Node facilities. This is a full runtime approach rather than implementing selected Node APIs on another JavaScript engine. Its security page remains macOS-focused and includes older future-tense background-execution wording; do not treat it as a complete current platform audit. [Runtime model](https://developers.raycast.com/information/security)

Raycast uses npm in its publishing build, accepts package dependencies, and documents ESM authoring. Its manifest's `external` option allows packages/files to be excluded from bundling for runtime import. These mechanisms support broad library use but do not prove arbitrary dependencies work with every bundle format or platform. [Publishing setup](https://developers.raycast.com/basics/prepare-an-extension-for-store), [FAQ](https://developers.raycast.com/misc/faq), [manifest](https://developers.raycast.com/information/manifest)

Store guidance permits system executables and some downloaded/packaged binaries, with publication conditions. A native executable invoked as a subprocess is different from a `.node` add-on loaded into the JavaScript process. The reviewed Raycast docs do not establish universal `.node` compatibility. [Binary dependencies](https://developers.raycast.com/basics/prepare-an-extension-for-store#binary-dependencies-and-additional-configuration)

## Pi and Tinycast

See [the pinned source comparison](pi-tinycast-node-compatibility.md). Pi's npm distribution uses Node; the inspected standalone build script embeds Bun, and its loader also handles bundled Node/Node SEA cases. Runtime details therefore depend on distribution. Tinycast uses JavaScriptCore and handwritten compatibility layers. Neither the existence of a JavaScript engine nor successful bundling proves Node API compatibility.

## Native dependency distinction

JavaScript-only packages, Node built-in APIs, compiled Node add-ons and external native executables are different compatibility cases. Node-API supplies ABI stability within its supported API/version conditions, while direct Node/V8 C++ APIs and external libraries can have other ABI constraints. Native packaging must still match OS, architecture and relevant system dependencies. [Node-API](https://nodejs.org/api/n-api.html#implications-of-abi-stability)

Worker execution is also material: Node documents differences from its main thread and conditions on loading native add-ons from multiple threads. Therefore selecting workers for reload/low overhead needs native dependency tests rather than assuming full main-thread equivalence. [Worker limitations](https://nodejs.org/api/worker_threads.html#class-worker)

## Accepted runtime direction

The user accepted managed real Node in Q18; see [ADR 0008](../adr/0008-managed-node-for-javascript-extensions.md). Use it as the JS/TS extension runtime. That aligns with the user's emphasis on ordinary libraries, filesystem/network/process access and low authoring friction. Keep GPUI CE responsible for rendering. This does not imply using Electron or a browser renderer.

Support native dependencies with explicit target packaging and tests, without promising every npm package works everywhere. Native Rust executable extensions are separately accepted in [ADR 0007](../adr/0007-native-rust-extension-processes.md); supporting a Rust-authored Node add-on is not by itself a first-class Rust extension API.

Retain accepted lazy activation and runtime management so users do not manually install Node. The runtime adds download/disk size and active memory costs; do not infer launcher resource usage from framework choice or the earlier fresh-process Wasm smoke test. Measure total host/extension memory, idle CPU, startup, reload and custom UI latency before deciding shared workers versus separate processes.

WIT/Wasm remains optional research for interoperability. Compiling ordinary JS extensions into Wasm would not automatically retain Node library/native-add-on compatibility. A smaller embedded engine with shims remains a possible trade-off, but it must justify compatibility and maintenance costs against the capability-first requirement.
