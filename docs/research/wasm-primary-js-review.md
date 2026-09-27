# JavaScript guests for a Wasmtime-first launcher

Date: 2026-09-27. Status: runtime decision reopened; research, not an accepted replacement architecture. This review did not install tools or execute guest components. The parent investigation owns the new local experiment.

## Finding

JavaScript/TypeScript can target the same WIT extension contract as Rust and execute inside Wasmtime. There are two practical guest engines: ComponentizeJS embeds StarlingMonkey/SpiderMonkey; componentize-qjs embeds QuickJS. Both put a JavaScript engine into the component. Wasmtime does not itself execute JS source, and wit-bindgen alone does not turn arbitrary npm applications into components. Jco supplies JS/TS build tooling and can select either engine. [ComponentizeJS source](https://github.com/bytecodealliance/ComponentizeJS/blob/4d812f5a7b524cea5bcfd565cac55f3fab876a57/README.md), [QuickJS componentizer](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/README.md), [Jco build guide](https://bytecodealliance.github.io/jco/creating-new-js-components.html).

The difficult tradeoff is preserving the capability-first extension promise. Portable npm logic can work, and Jco now has experimental Node API adapters. Full Node behavior, native addons, arbitrary desktop APIs and existing process integrations still require compatibility work or native host bridges. Choosing Wasmtime remains possible, but these are product/SDK responsibilities rather than benefits automatically supplied by WIT.

## Minimal Windows build to test

The existing [smoke world](wasm-spike/wit/world.wit) exports `query(string) -> list<search-result>`. A matching guest is:

```js
export function query(text) {
  return [
    { id: "apps", title: "Open Applications" },
    { id: "calculator", title: "Calculator" },
    { id: "links", title: "Quicklinks" },
  ].filter(row => row.title.toLowerCase().includes(text.toLowerCase()));
}
```

For an isolated experiment with `search.js` and a copied `wit/` directory:

```powershell
npm install --save-exact componentize-qjs@0.4.5
npx componentize-qjs --wit wit --js search.js --sync --opt-size -o search-js.wasm
wasmtime run --invoke 'query("calc")' search-js.wasm
```

`--sync` deliberately selects the non-async guest engine; the default QuickJS engine requires component-model async support. `--opt-size` chooses the size-oriented runtime. The npm wrapper requires Node >=18 at build time and has Windows x64/ARM64 native bindings, alongside Linux/macOS variants. Prebuilt CLI archives are another authoring route. These are developer tools: distributing their generated component does not imply shipping Node to every launcher user. [CLI flags](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/README.md), [published package metadata](https://registry.npmjs.org/componentize-qjs/0.4.5), [build matrix](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/.github/workflows/npm-publish.yml).

The alternative direct CLI is `componentize-js --wit wit -o search-js.wasm search.js`. Current Jco also exposes `jco componentize --backend qjs --wit wit -o search-js.wasm search.js`; TypeScript entrypoints are automatically bundled and stripped of type syntax. Run the TypeScript checker separately. For JavaScript dependencies, request `--bundle`. Our first pure-JS experiment should avoid conflating bundler/polyfill issues with WIT execution. [Jco guide](https://bytecodealliance.github.io/jco/creating-new-js-components.html), [current command implementation](https://github.com/bytecodealliance/jco/blob/e2f16593fbe99fb05b52e386425b7120ae8d1808/packages/jco/src/cmd/componentize.ts).

Do not repeat an old blanket claim that ComponentizeJS cannot author components on Windows. Current source normalizes Windows paths; its CI tests Windows/macOS/Linux using a prebuilt engine. Building the underlying engine itself remains Linux-only in that workflow. A test matrix is evidence of intended coverage, not proof every release/build succeeds. [CI](https://github.com/bytecodealliance/ComponentizeJS/blob/4d812f5a7b524cea5bcfd565cac55f3fab876a57/.github/workflows/main.yml), [componentization implementation](https://github.com/bytecodealliance/ComponentizeJS/blob/4d812f5a7b524cea5bcfd565cac55f3fab876a57/src/componentize.js).

## npm and native compatibility

| Area | Observed support and implication |
| --- | --- |
| Ordinary JS algorithms | Suitable when dependencies fit the guest language/builtin environment. Test actual packages; npm distribution does not imply Node requirements. |
| ES modules | QuickJS's loader resolves relative imports and `node_modules` during initialization. Its direct loader explicitly does not transform CommonJS; Jco bundling is a separate route. |
| Node builtins | Jco's compatibility layer handles reviewed APIs with portable code, WASI-backed behavior or custom host imports. This is experimental, with incompatible changes possible without a major version. |
| Files and subprocesses | Some Node-compatible wrappers exist, but depend on explicit host providers. Their provided real-Node adapters are not implementations for our Rust/Wasmtime host. We would implement the WIT imports or bridge to a helper. |
| `child_process` | Current documented adapter covers synchronous `spawnSync`, `execSync`, `execFileSync`. Async `spawn`, callback `exec`/`execFile`, `ChildProcess`, `fork` and IPC remain unsupported. |
| Dynamic module loading | Jco's `node:module` wrapper does not provide Node's runtime loader. Calls through its generated `require()` refuse; bundling ahead of time is materially different from dynamic extension loading inside a guest. |
| Native `.node` addons | The guest engine is not Node, and these tools do not establish general Node-API addon loading. A supported native operation needs a host/native bridge or another implementation. |

Sources: [QuickJS module-resolution example](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/examples/module-resolution/README.md), [Node compatibility boundaries](https://bytecodealliance.github.io/jco/interop/nodejs-builtins.html), [jco-std adapter catalog](https://github.com/bytecodealliance/jco/blob/e2f16593fbe99fb05b52e386425b7120ae8d1808/packages/jco-std/README.md), [child-process adapter](https://bytecodealliance.github.io/jco/interop/nodejs-builtins/supported-modules/child-process.html), [module loader boundaries](https://bytecodealliance.github.io/jco/interop/nodejs-builtins/supported-modules/module.html).

Jco can add required WIT imports/dependency files during Node API componentization. Run compatibility experiments against a copied WIT directory rather than letting experimental tooling alter the canonical launcher interface unexpectedly. [Child-process interface injection](https://bytecodealliance.github.io/jco/interop/nodejs-builtins/supported-modules/child-process.html).

For a capability-first design, host bridges can be permissive. There is no need to invent mandatory user permission prompts merely because a guest is Wasm. However, permissive configuration cannot make an unimplemented host function exist. Desktop integration, subprocess lifecycle and UI rendering still need host implementations.

## Async and UI consequences

ComponentizeJS documents async JavaScript exports whose Promises are driven to completion and returned through synchronous component functions; imported functions remain synchronous under that documented model. This does not automatically give the launcher a nonblocking UI: invocation must be scheduled away from the GPUI UI thread. [ComponentizeJS async section](https://github.com/bytecodealliance/ComponentizeJS/blob/4d812f5a7b524cea5bcfd565cac55f3fab876a57/README.md#async-support).

QuickJS's default runtime additionally implements component-model async, with WIT futures/streams and cancellation machinery. Its synchronous runtime is selectable separately. A pure synchronous search result smoke test does not validate long-lived subscriptions, cancellation, streaming UI, recursive cross-extension calls or reload cleanup. We must pick and test an explicit host/guest async contract instead of treating all uses of “async” as equivalent. [Runtime intrinsics](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/docs/runtime-intrinsics.md), [export invocation implementation](https://github.com/andreiltd/componentize-qjs/blob/e563c6d6ae50b087980414015663ca9c948c09bb/crates/runtime/src/interpreter.rs).

GPUI objects remain in the native host. The WIT contract must describe views/events/actions or drawing requests that the host executes. Neither JS engine replaces the outstanding GPUI bridge design.

## Size evidence and what it does not prove

Inspected published npm metadata/tar archives on this date:

| Item | Bytes | Meaning |
| --- | ---: | --- |
| `@bytecodealliance/componentize-js@0.23.0` package unpacked total | 42,798,700 | Build-tool package, excluding transitive dependencies. |
| Its `lib/starlingmonkey_embedding.wasm` archive member | 10,520,737 | Unspecialized release guest engine; not a finished extension or RAM measurement. |
| `componentize-qjs@0.4.5` wrapper unpacked total | 61,900 | Wrapper only; excludes platform binding. |
| Windows x64 QuickJS componentizer binding package | 35,446,456 | Native authoring-tool package; not the generated component size. |

Sources: [ComponentizeJS registry record and linked tarball](https://registry.npmjs.org/@bytecodealliance%2fcomponentize-js/0.23.0), [QuickJS wrapper](https://registry.npmjs.org/componentize-qjs/0.4.5), [Windows binding](https://registry.npmjs.org/@andreiltd%2fcomponentize-qjs-binding-win32-x64-msvc/0.4.5).

Measure actual output components, compilation/cache cost, active-instance memory and repeated calls. The earlier 63 KB Rust component contains no JavaScript engine and is not a JS extension size baseline. Multiple self-contained JS components include their engine payload; the extent of runtime/compiler/code-page sharing needs an explicit implementation and measurements. No RAM or latency superiority is concluded here.

## Recommended next evidence

1. Build the same search world as Rust, QuickJS and StarlingMonkey; execute all in the same Wasmtime release.
2. Test a host call for files or process execution and a long-running async call/cancellation. This answers whether capability-first integrations remain practical.
3. Compare one and several active instances using a persistent embedded host. Fresh CLI process startup alone cannot select the production runtime.
4. Validate save/build/reload and useful source errors with a realistic TS extension.

A successful result can justify replacing the managed Node/native-Rust-first decision. Until then, record Wasmtime-first as the reopened candidate rather than silently asserting that earlier architecture and compatibility promises remain unchanged.
