# Notice for the prebuilt JS/TS sample components

`sample_js.wasm` and `sample_ts.wasm` are built by `cargo xtask js-guests`
(inputs in [manifest.json](manifest.json)). Each component combines:

| Part | License |
| --- | --- |
| The sample's own source (`guests/sample-js`, `guests/sample-ts`, `guests/js`) | Apache-2.0 OR MIT |
| [zod](https://github.com/colinhacks/zod) 4.6.5, bundled | MIT |
| componentize-qjs runtime at the pinned commit, with Pane's patches in `tools/componentize-js/patches/` | Apache-2.0 ([text](../../tools/componentize-js/patches/LICENSE-componentize-qjs)) |
| QuickJS-ng via rquickjs | MIT |
| wasi-libc from wasi-sdk 34 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT (some parts BSD or CC0) |
| Rust standard library | MIT OR Apache-2.0 |

Redistributing a component means following each part's terms, including
keeping these notices. See the [licensing audit](../../docs/research/licensing-audit.md).
