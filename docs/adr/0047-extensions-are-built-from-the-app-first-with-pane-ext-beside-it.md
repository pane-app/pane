# Extensions are built from the app first, with a thin `pane-ext` CLI beside it, and JS/TS authors need only Node and npm

Accepted 2026-10-08 by the user's decisions, after comparing how Raycast, Zed, PowerToys Command Palette, Obsidian, Vicinae, Spin and the Shopify CLI let people build extensions. It settles the open questions about names and the CLI's shape in the extension-authoring specification (#128).

Writing an extension today needs a Pane checkout. A Rust author points `pane-guest` at a path inside it. A JS/TS author runs `tools/componentize-js/pane_js.py`, which needs Python 3.12+, nightly Rust with rust-src and wasi-sdk 34, takes about five minutes to build its toolchain the first time, and has only ever run on Linux x86_64. Whatever an extension prints is thrown away (the host gives each guest an empty WASI context).

**The app is the main way in.** Pane gets a Create Extension command (a name, Rust or TypeScript, and a template: list, detail, form or no-view) that writes the folder and starts development mode on it, an Import Extension command for source that already exists, a Logs screen showing the extension's standard output and error beside Pane's own messages about it, and an error overlay with the stack trace. Raycast and PowerToys Command Palette start authors from the app; Zed's Install Dev Extension builds Rust to WASM itself.

**A thin CLI is the terminal companion.** Its command is `pane-ext`, because the application's executable is already `pane`. It has `new` (the same templates as the app), `dev`, `check` and `pack`; `publish` comes later. `pane-ext dev` builds in the terminal, so cargo and tsc errors appear there, hands the result to the running Pane (starting Pane if it is not running), and streams the extension's log back over a per-user named pipe on Windows or Unix socket elsewhere. The CLI calls cargo, tsc and eslint; it never reimplements them. Templates run it through `npm run dev`.

**One build crate.** The build and reload logic lives in one crate that both the app's development mode and `pane-ext` use, so the two can never build a package differently.

**JS/TS authors need only Node and npm.** `@pane-app/cli` carries Pane's own patched componentize-qjs (the build that produces WASI 0.3 components) prebuilt for Windows, macOS and Linux on x64 and arm64, in per-platform `optionalDependencies` packages chosen by a JS shim, as esbuild, biome and turbo ship. `runtime.wasm` and the WASI 0.3 `libc.so` are wasm and run on every system, so the CLI embeds them; esbuild is an ordinary dependency. This replaces the Python pipeline. Building the componentizer for Windows and macOS has never been done and is the first real unknown.

**Rust authors need only rustup stable and the `wasm32-wasip2` target**, and depend on the SDK published to crates.io as `pane-extension`.

**Names.** npm scope `@pane-app`, matching the GitHub organization: `@pane-app/extension` (the SDK and types), `@pane-app/cli` (the tool) and `@pane-app/create` (so `npm create @pane-app` starts a project). The CLI stays out of the types package; Raycast's `ray` inside `@raycast/api` makes that package about 30 MB. `@pane`, `pane`, `create-pane` on npm and `pane` on crates.io were taken on 2026-10-08; `pane-extension` was free. Publishing is a person's step, never CI's.

**A template repository**, `pane-app/extension-template`, uses the layout of the official extension repositories ([ADR 0045](0045-official-extensions-live-in-their-own-repositories.md)), builds a release revision on each `v<semver>` tag, and has an AGENTS.md pointing at the docs, the WIT files and the examples, so an AI asked to write an extension can.

**Rejected.**
- jco/StarlingMonkey and stock componentize-qjs 0.4.5: both produce WASI 0.2 components, which Pane refuses ([ADR 0013](0013-require-wasi-03.md)), and stock componentize-qjs shares one `Math.random` state across instances through its snapshot.
- Javy: it produces core modules, not components.
- The CLI inside the types package, as Raycast (`ray` inside `@raycast/api`) and Vicinae (`vici` inside `@vicinae/api`) do: it makes the package an extension depends on for its types about 30 MB.

**Evidence (2026-10-08).** Raycast has both an in-app Create Extension command with templates and `npm init raycast-extension`; `ray develop` hot-deploys into the app through URLs and a PID file, sends `console` output to the terminal and shows errors as an in-app overlay; its CLI stopped shipping per-platform binaries in 1.86. Zed's Install Dev Extension builds Rust to WASM in the app, but its logs are hard to find. PowerToys Command Palette has an in-app Create extension command, but reloads only by hand. Obsidian has had a CLI built into the app since 1.12 (`plugin:reload`, `dev:errors`). Spin dropped its native js2wasm for an npm-only route (`j2w`, wrapping ComponentizeJS). The Shopify CLI downloads Javy on demand.
