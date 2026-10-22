# Rust plugin precedents

Research date: 2026-09-27. Evidence for the Rust runtime discussion; no runtime choice is adopted here. No builds or resource benchmarks were performed.

## Nushell: executable plugins

Nushell plugins are separate executables launched when needed. They exchange JSON or MessagePack over standard streams or local sockets. Registration runs the executable to discover commands and caches those signatures, so discovery and command execution are distinct. Plugins stay alive while used; the documented default stops an inactive plugin after ten seconds, configurable globally and per plugin. Users can also explicitly stop a plugin. [Nushell plugin guide](https://www.nushell.sh/book/plugins.html)

The Rust SDK provides a `Plugin` implementation and `serve_plugin` entry point. Local sockets mean Unix sockets or Windows named pipes; the standard-stream transport remains supported. A plugin may retain shared state between calls. These are executable processes, not a documented interface for loading third-party Rust shared libraries into Nushell itself. [Contributor guide](https://www.nushell.sh/contributor-book/plugins.html)

Official core plugins are usually shipped with Nushell releases or its packages. Third-party installation documentation commonly uses `cargo install`; Nushell therefore does not establish a universal no-toolchain installation experience. A versioned protocol also does not automatically mean broad backwards compatibility: its guide requires matching `nu-plugin` versions and updating plugins with Nushell. [Installation and compatibility](https://www.nushell.sh/book/plugins.html)

For this launcher, a native-executable design could provide ordinary Rust crates, native operating-system integration and restartable extension processes. This is a design inference, not a claim that Nushell supplies our desktop UI protocol. Supplying prebuilt platform packages would be our responsibility.

## Native dependencies still need packaging

Rust can produce executables and can statically or dynamically link dependencies. Shared system libraries and C runtimes may still be required depending on the target and build configuration. An executable plugin may use native libraries, but that is different from exposing the host's Rust objects across a dynamic-library boundary. “Users need no Rust compiler” is achievable through prebuilt packages; it is not a guarantee that every Rust executable is a dependency-free file. [Rust linkage reference](https://doc.rust-lang.org/reference/linkage.html)

## Zed: Rust compiled to WebAssembly

Current Zed documentation uses Rust's `wasm32-wasip2` target for procedural extensions. Authors implement `zed_extension_api::Extension`. The documentation explicitly warns that host platform/environment assumptions need supplied APIs rather than ordinary native Rust behavior. Developers need Rust, while published-extension installation is a separate path. [Developing extensions](https://zed.dev/docs/extensions/developing-extensions)

Zed's first-party architecture article explains that its publishing system compiles extensions, uploads archives, and downloads compiled Wasm to users. WIT and `wit-bindgen` connect the guest API to a Wasmtime host. That article dates to October 2024 and shows the older `wasm32-wasip1` target; use current development documentation for the target. [Zed architecture article](https://zed.dev/blog/zed-decoded-extensions)

The inspected public `zed_extension_api` 0.7.0 trait exposes language-server, debugger, context-server, slash-command and documentation hooks. It does not expose general GPUI view construction or a custom window renderer. Therefore, choosing GPUI CE does not automatically give our extensions a ready-made Rust UI API, and Zed's extension system is not evidence that our desired custom interactive views already exist. [Extension trait](https://docs.rs/zed_extension_api/0.7.0/zed_extension_api/trait.Extension.html)

## Implication for the current discussion

Native executable plugins are a concrete precedent worth evaluating against this project's trusted, capability-first requirements. Rust-to-Wasm is also proven in a Rust desktop host, but needs deliberate host APIs and compatible dependencies. Neither route supplies the proposed shared GPUI view/event contract automatically. No comparative CPU, startup, memory or reload measurements were obtained in this research.
