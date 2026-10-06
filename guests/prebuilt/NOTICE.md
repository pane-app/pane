# Notice for the prebuilt JS/TS sample components

The components in this folder (`sample_js.wasm`, `sample_ts.wasm`,
`sample_settings_js.wasm`, `sample_settings_ts.wasm`, the operations,
applications, schedule, actions, preferences and arguments samples, and `sample_npm_js.wasm`, the component
of the npm sample, from `guests/sample-npm-js`) are built by
`cargo xtask js-guests`
(inputs in [manifest.json](manifest.json)). Each component combines:

| Part | License |
| --- | --- |
| The sample's own source (`guests/sample-js`, `guests/sample-ts`, `guests/sample-settings-js`, `guests/sample-settings-ts`, `guests/sample-schedule-js`, `guests/sample-schedule-ts`, `guests/sample-actions-js`, `guests/sample-actions-ts`, `guests/sample-preferences-js`, `guests/sample-preferences-ts`, `guests/sample-arguments-js`, `guests/sample-arguments-ts`, `guests/js`) | Apache-2.0 OR MIT |
| [zod](https://github.com/colinhacks/zod) 4.6.5, bundled into `sample_js.wasm` and `sample_ts.wasm` only | MIT |
| componentize-qjs runtime at the pinned commit, with Pane's patches in `tools/componentize-js/patches/` | Apache-2.0 ([text](../../tools/componentize-js/patches/LICENSE-componentize-qjs)) |
| QuickJS-ng via rquickjs | MIT |
| wasi-libc from wasi-sdk 34 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT (some parts BSD or CC0) |
| Rust standard library | MIT OR Apache-2.0 |

Redistributing a component means following each part's terms, including
keeping these notices. See the [licensing audit](../../docs/research/licensing-audit.md).
