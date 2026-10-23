# Licensing

- The application is licensed under [GPL-3.0-or-later](../LICENSE-GPL). That covers everything not listed below, including `crates/`, `xtask/` and `scripts/`.
- The extension contract (`wit/`), everything under `guests/` (guest bindings, the SDKs, TypeScript declarations, sample extensions and fixtures) and Pane's own files in `tools/componentize-js/` are licensed under [Apache-2.0](../guests/LICENSE-APACHE) OR [MIT](../guests/LICENSE-MIT), at your option. Code compiled into an extension therefore puts no licence on it.
- `packages/create/` (what `npm create @pane-app` runs) and `packages/extension-template/` are Apache-2.0 OR MIT too, with their own licence files.
- The patches in `tools/componentize-js/patches/` modify componentize-qjs and are licensed under [Apache-2.0](../tools/componentize-js/patches/LICENSE-componentize-qjs), like it. The QuickJS runtime they build is part of every JS/TS component, so the prebuilt components in `guests/prebuilt/` also carry that runtime's terms; see [its notice](../guests/prebuilt/NOTICE.md).
- The WIT files in `wit/deps/` (`wasi:clocks`, `wasi:http` and the packages it names) are WASI's, copied unchanged from wasmtime-wasi-http 49.0.1 (Apache-2.0 WITH LLVM-exception).
- Contributions are accepted under the same terms with a DCO sign-off; see [CONTRIBUTING.md](../.github/CONTRIBUTING.md).

Third-party dependencies keep their own licences, and `cargo deny` checks them in CI ([audit](research/licensing-audit.md)). Release notice bundles and the per-release source procedure follow that audit and aren't produced yet.
