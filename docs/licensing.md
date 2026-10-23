# Licensing

- The application is licensed under [GPL-3.0-or-later](../LICENSE-GPL). That covers everything not listed below, including `crates/`, `xtask/` and `scripts/`.
- The extension contract (`wit/`), everything under `guests/` (guest bindings, the SDKs, TypeScript declarations, sample extensions and fixtures) and Pane's own files in `tools/componentize-js/` are licensed under [Apache-2.0](../guests/LICENSE-APACHE) OR [MIT](../guests/LICENSE-MIT), at your option. Code compiled into an extension therefore puts no licence on it.
- `packages/create/` (what `npm create @pane-app` runs) and `packages/extension-template/` are Apache-2.0 OR MIT too, with their own licence files.
- The patches in `tools/componentize-js/patches/` modify componentize-qjs and are licensed under [Apache-2.0](../tools/componentize-js/patches/LICENSE-componentize-qjs), like it. The QuickJS runtime they build is part of every JS/TS component, so the prebuilt components in `guests/prebuilt/` also carry that runtime's terms; see [its notice](../guests/prebuilt/NOTICE.md).
- The WIT files in `wit/deps/` (`wasi:clocks`, `wasi:http` and the packages it names) are WASI's, copied unchanged from wasmtime-wasi-http 49.0.1 (Apache-2.0 WITH LLVM-exception).
- Contributions are accepted under the same terms with a DCO sign-off; see [CONTRIBUTING.md](../.github/CONTRIBUTING.md).

Third-party dependencies keep their own licences, and `cargo deny` checks them in CI ([audit](research/licensing-audit.md)).

## What each package carries

Every package Pane's release is made of (`cargo xtask package-windows`, `package-macos`, `package-linux`) holds, beside the program:

- `LICENSE.txt`: the GPL-3.0-or-later text of [LICENSE-GPL](../LICENSE-GPL).
- `THIRD-PARTY-NOTICES.txt`: each third-party part the program is built from, with its licence, and the texts of those licences. The packaging writes it for the system it builds on (`xtask/src/notices.rs`). The Rust crates come from Cargo: the crates a release build of `pane` compiles (`cargo tree -e normal`), reached through normal dependencies rather than procedural macros or build scripts, with their licences and authors from `cargo metadata` and the licence files each crate's package holds. A crate packaged without a licence file gets the standard text of the licence its manifest names, from `xtask/licenses/`; one whose licence has no text there fails the packaging, so a new licence is reviewed first. What Cargo can't see is listed by hand in `notices.rs`: the Rust standard library, the Geist fonts and reicon icons the program embeds, WASI's WIT files, and Pane's own Apache-2.0 OR MIT SDK the program embeds. A new embedded font, picture or bundled library is added there.
- `README.txt`, whose last section says the licence, names the notices, and says where the source is.

No JavaScript from npm ships in the program: the JS/TS SDK it embeds is Pane's own (`guests/js`).

## Where the source is

A release is built from a `v<version>` tag of this repository, and the source of that version is the tag, `https://github.com/pane-app/pane/tree/v<version>`, and the Source code archives on its release page, `https://github.com/pane-app/pane/releases/tag/v<version>`. The package's README gives those links for its own version. The third-party crates' sources are the versions the tag's `Cargo.lock` pins, from crates.io and the git revisions it names. The release itself carries them too: its `pane-<version>-source.tar.gz` asset (written by the release workflow, `scripts/release-source.sh`) holds the tag's tree and, under `vendor/`, every crate the `Cargo.lock` pins, and builds offline with `cargo build --release --offline --config vendor/config.toml -p pane`. Keep each release's tag and page up for as long as its packages are offered.
