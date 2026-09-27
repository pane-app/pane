# GPUI CE renderer audit

Date: 2026-09-27. User selected GPUI CE, Pi-style standard helpers plus custom UI, trusted extensions, and JS/TS plus Rust authoring at launch. This report evaluates that choice; it does not select a plugin transport or loading ABI.

Source: shallow clone of [gpui-ce/gpui-ce](https://github.com/gpui-ce/gpui-ce), pinned to `17d9c8e8fdb30a329d817ca06bff424e8e848f1a`. Inspected source, manifests, examples and CI. No dependency installation, build or app benchmark was performed. Local checkout: `%TEMP%/kyoko-gpui-ce-audit`.

## Finding

GPUI CE supplies the Rust desktop rendering primitives needed for standard launcher views and genuinely custom layouts/drawing. It does not by itself establish our extension SDK, JS bridge, installation model or hot reload. Those remain launcher responsibilities.

## Platform evidence

The platform factory selects `MacPlatform`, `WindowsPlatform` and the Linux platform implementation using target conditionals. Linux contains X11 and Wayland clients behind features. This is concrete implementation evidence for all three requested desktop systems. It does not establish parity for every launcher integration such as global shortcuts, window activation, clipboard observation or file indexing. [Platform factory](https://github.com/gpui-ce/gpui-ce/blob/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/crates/gpui_platform/src/gpui_platform.rs), [Linux selection](https://github.com/gpui-ce/gpui-ce/blob/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/crates/gpui_linux/src/linux.rs).

The CI workflow defines Linux, macOS and Windows build/test jobs. Its `just build` recipe builds workspace targets and `just test` runs workspace tests. This audit inspected workflow definitions, not a successful execution of each job or a manual GUI test. Some visual tests in the platform crate are ignored by default because they need macOS main-thread execution. [CI](https://github.com/gpui-ce/gpui-ce/blob/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/.github/workflows/ci.yml), [Recipes](https://github.com/gpui-ce/gpui-ce/blob/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/justfile).

## Standard and custom UI

Views implement `Render` and return an element tree. The lower-level `Element` trait exposes layout, prepaint, paint and accessibility hooks. This supports custom layouts and interaction while keeping drawing inside the launcher window. A `canvas` helper exposes custom drawing without requiring a full custom element implementation. [Element source](https://github.com/gpui-ce/gpui-ce/blob/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/crates/gpui/src/element.rs), [Canvas](https://github.com/gpui-ce/gpui-ce/blob/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/crates/gpui/src/elements/canvas.rs).

Inference for the launcher: standard lists/forms can be convenient SDK components, with lower-level layout/drawing/input primitives for custom interfaces. A custom interface need not imply HTML, a browser engine, or a separate window. The exact primitives exposed to extensions still need design and validation, particularly keyboard focus, text input, accessibility, event callbacks and lifecycle cleanup.

Rust being the host language does not automatically make arbitrary installed Rust plugins compatible with GPUI types or safely reloadable. Passing an ordinary Rust `Element` object across a plugin/process boundary is a separate ABI/ownership problem. A view/event protocol is one candidate, but any such protocol limits custom capabilities to what it exposes; it should not be described as complete GPUI API parity.

## JS bindings, extension loading and hot reload

Scoped search of README, docs, crates and workflows for JavaScript, bindings, plugin, dylib and hot-reload terms did not reveal a documented desktop JS extension SDK or a plugin hot-reload facility. The repository does contain `wasm-bindgen` and a `gpui_web` backend: these support running GPUI on the web and are not evidence of a desktop Node/JS plugin bridge. This is a scoped finding, not a claim that no third-party integration exists. [Web backend](https://github.com/gpui-ce/gpui-ce/tree/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/crates/gpui_web), [Platform setup](https://github.com/gpui-ce/gpui-ce/blob/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/crates/gpui_platform/src/gpui_platform.rs).

Consequently, a Pi-like authoring experience needs our own extension lifecycle, registration API and UI bridge. Reload should rebuild/restart the relevant extension and clean up its registered contributions; state preservation is a separate product contract, not a property established by GPUI CE.

## Version and API maturity

At the pinned commit, the core package manifest reports `gpui-ce` version `0.2.2`, library import name `gpui`, and Rust minimum `1.95`. These are checkout manifest values, not a claim about the newest published package. The README says compatibility with upstream GPUI is changing. CI includes a public API comparison against a pull request's base, but that does not make upstream examples or downstream dependencies universally interchangeable. Pin the initial renderer revision and put our public extension contract behind an adapter so framework upgrades do not automatically change extension APIs. [Manifest](https://github.com/gpui-ce/gpui-ce/blob/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/crates/gpui/Cargo.toml), [README](https://github.com/gpui-ce/gpui-ce/blob/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/README.md).

## Resource usage

GPU rendering and avoiding a mandatory browser renderer are architectural characteristics, not measured memory or latency guarantees. The repository contains rendering benchmark examples and performance tooling, but this audit obtained no representative launcher memory, GPU allocation, idle CPU or startup measurement. Its README's performance ambitions should not become our product numbers. [Benchmark examples](https://github.com/gpui-ce/gpui-ce/tree/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/crates/gpui/examples/bench), [Performance tooling](https://github.com/gpui-ce/gpui-ce/blob/17d9c8e8fdb30a329d817ca06bff424e8e848f1a/tooling/perf/src/main.rs).

The next useful experiment is one launcher window with search, a large list, one form, one custom interactive view and equivalent JS/TS and Rust extensions. Measure the host and all extension processes together, including hidden/idle state, first invocation, steady-state interaction and repeated reloads. Test platform behavior separately from performance; a Windows-only measurement does not validate macOS/Linux parity.
