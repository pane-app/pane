# Licensing audit: dependencies, split proposal and notice procedure

Audited 2026-09-27 against commit `b1a005d` for planning prerequisite [#3](https://github.com/hoangvu12/pane/issues/3).

> **Not legal advice.** This is an engineering inventory and a proposal to support a decision. It is not a legal opinion. It draws no conclusion about whether an extension is a derivative work of Pane, and it draws none from the use of WIT/WASI alone. Get the consequential choices in [section 3](#3-decisions-needed-from-the-user) reviewed by a qualified lawyer before release.

No license has been applied. There is no `LICENSE` file, no `license` field has been added to any manifest, and every first-party crate has `publish = false`. The document has three parts: **facts found** (section 1), **proposal** (section 2) and **decisions needed from the user** (section 3). The direction it starts from is the provisional one recorded in [Q40](pane-license-options-q40.md) and spec decision 22 (#1): Zed-style, primarily GPL-3.0-or-later. That direction does not prohibit compliant paid forks and does not automatically assign one license to every third-party extension.

## 1. Facts found

### 1.1 Method and tools

| Tool | Version | Use |
| --- | --- | --- |
| cargo / rustc | 1.98.1 (pinned by `rust-toolchain.toml`) | `cargo metadata --locked --format-version 1 --filter-platform <target>` for each of the six app targets, and `wasm32-wasip2` for `guests/`. `cargo tree -e normal --target <t> -i <crate>` to trace paths. |
| cargo-deny | 0.20.2 (`cargo install --locked cargo-deny`) | `cargo deny --locked --workspace --exclude-dev --config <audit deny.toml> check -s licenses`, for the root workspace (six targets) and for `guests/` (`wasm32-wasip2`). |
| cargo-about | 0.9.2 (`cargo install --locked cargo-about --features cli`) | `cargo about generate --locked --workspace -c about.toml <template>`, as a trial of notice generation. |
| wasm-tools | 1.259.0 | `wasm-tools metadata show target/guests/sample_rust.wasm`. |
| Python | 3.14.4 | A throwaway classifier over the `cargo metadata` output. |

The classifier walks the dependency graph from first-party roots. It counts a package as **shipped** when a chain of normal (non-dev, non-build) edges reaches it and it is not a proc-macro. It counts a package as **build-time** when it is reached only through build dependencies or proc-macros. The shipped count is an upper bound: it is the resolved graph for the target, and the linker can still discard unused code. The scratch configs and outputs were kept under `target/license-audit/`, which is gitignored and was not committed. The configs to adopt are given in [section 2.4](#24-generating-and-checking-notices-in-ci).

Targets: `x86_64`/`aarch64` for each of `pc-windows-msvc`, `apple-darwin` and `unknown-linux-gnu`.

### 1.2 (a) Application binary (`crates/pane` + `crates/pane-core`)

Direct dependencies: GPUI CE (git rev `17d9c8e8`), `wasmtime`/`wasmtime-wasi` `=49.0.1`, `tokio` 1.53.1.

| Target | Shipped (third-party) | Build-time only |
| --- | --- | --- |
| x86_64-pc-windows-msvc | 384 | 88 |
| aarch64-pc-windows-msvc | 381 | 88 |
| x86_64-apple-darwin | 415 | 94 |
| aarch64-apple-darwin | 412 | 94 |
| x86_64-unknown-linux-gnu | 488 | 96 |
| aarch64-unknown-linux-gnu | 485 | 96 |

**License expressions seen in the shipped graph (all targets, union):** MIT; Apache-2.0; Apache-2.0 WITH LLVM-exception (Wasmtime, Cranelift, cap-std and related crates); many dual `MIT OR Apache-2.0` variants, including the legacy `/` spellings; BSD-2-Clause; BSD-3-Clause; ISC; Zlib; Unicode-3.0; CC0-1.0; `Unlicense OR MIT`; `Apache-2.0 OR BSL-1.0`; `CC0-1.0 OR MIT-0 OR Apache-2.0`; `0BSD OR MIT OR Apache-2.0`; MPL-2.0; and `Apache-2.0 OR GPL-2.0-only`.

cargo-deny result with the allow-list in section 2.4: **`licenses ok: 0 errors`** for the app graph over all six targets. The only warning was that the `BSL-1.0` allowance was never needed, because `ryu`'s `OR` resolves to Apache-2.0. cargo-about produced notices without errors, with one warning for `wgsl-rs-macros`, a build-time proc-macro (see below).

**Items that need attention:**

| Item | Where / path | Finding | GPL-3.0-or-later relevance |
| --- | --- | --- | --- |
| **GPUI CE** (all `gpui_ce_*` crates) | git rev `17d9c8e8`, checked in `~/.cargo/git/checkouts/gpui-ce-*/17d9c8e` | The root `LICENSE.md` is the Apache License 2.0. Each crate has `LICENSE-APACHE` with "Copyright 2022 - 2025 Zed Industries, Inc." and `license = "Apache-2.0"`. There is no `NOTICE` file. | Apache-2.0 can be combined into a GPLv3 work (one direction only). The notices are required. |
| **self_cell 1.3.0** | Linux only, via `cosmic-text` ← `gpui_ce_wgpu` | `Apache-2.0 OR GPL-2.0-only` | **Take the Apache-2.0 branch.** GPL-2.0-only cannot be combined with GPL-3.0. Record the choice explicitly in the notices. |
| **option-ext 0.2.0** | Linux and macOS, via `dirs-sys` ← `dirs` ← `zed-font-kit` | MPL-2.0 (weak, file-level copyleft). No "Incompatible With Secondary Licenses" notice was found in its source files. | MPL-2.0 §3.3 permits combination with GPL. Source for the MPL-covered files must be available; the source procedure covers this. |
| **dwrote 0.11.5** | cargo-about lists it (MPL-2.0), but `cargo tree --target <each>` shows it on **none** of the six targets | cargo-about's target filtering is more conservative | Harmless if it is listed in the notices. |
| **wgsl-rs, wgsl-rs-ir** (shipped), **wgsl-rs-macros** (build) | git `schell/wgsl-rs` rev `b2de16a`, via GPUI CE | **No `license` field** in the manifests. The repo root `LICENSE` is MIT ("Copyright (c) 2025 Schell Carl Scivally"). GPUI CE's own `deny.toml` already clarifies these crates as MIT. | Compatible, but it needs a `clarify` entry (sha256 `3877f478…ccf254`). Ask upstream to add `license` fields. |
| **freetype-sys 0.20.1** | Linux only, via `zed-font-kit` | The crate declares MIT, but it **bundles FreeType C sources**. Its `build.rs` links the system `freetype2` when pkg-config finds version ≥ 24.3.18, and otherwise compiles the bundled copy statically. FreeType is `FTL OR GPL-2.0-or-later`. Its bundled gzip module is zlib-licensed, the BDF/PCF drivers use an X11-style license, and `ft-hb.c` uses Old MIT. **cargo metadata/cargo-about cannot see this.** | Both branches are compatible with GPLv3; FreeType's own `LICENSE.TXT` says FTL is compatible with GPLv3. If it is statically built, add a FreeType notice manually, including the FTL credit. The local Linux debug binary (see the fonts bullet below) contained no FreeType symbols and did not link `libfreetype`. |
| **yeslogic-fontconfig-sys**, **wayland-sys**, xcb/xkbcommon | Linux | Bindings (MIT). The system libraries are loaded or linked dynamically. `ldd` on the local debug build: `libxcb`, `libxkbcommon(-x11)`, `libXau`, `libXdmcp`, `libxcb-xkb`, glibc and `libgcc_s`. | These are OS-provided libraries. If a future AppImage or Flatpak bundles them, their notices (and LGPL obligations for glibc, if bundled) must be added. |
| **rav1e 0.8.1** (AV1 encoder) | all targets, via `ravif` ← `image` (default features) ← `gpui-ce` | BSD-2-Clause, plus a `PATENTS` file (Alliance for Open Media Patent License 1.0, which has a defensive-termination clause) | This is a patent grant, not a copyright restriction. Whether it is an "additional restriction" is a question for counsel. It is unnecessary for a launcher, so trimming GPUI CE's `image` features upstream would remove it. |
| `COPYRIGHT` files | 24 crates (`rustix`, `cap-primitives`, `rand*`, `encoding_rs`, `unicode-*`, `euclid`, `core-text`, …) | These state the dual-license grant | Include them in the notices. cargo-about includes license texts but does not pick up `COPYRIGHT` files by default. |
| Apache `NOTICE` files | none found among the 578 shipped crate versions in the registry cache; none in GPUI CE | — | Re-check whenever dependencies change. |

**No dependency in the app graph was found under GPL-2.0-only (without an alternative), AGPL, LGPL (statically), SSPL, BUSL or any proprietary or unknown license. There are two missing-metadata cases, `wgsl-rs` and `wgsl-rs-ir`, and their license file resolves them to MIT.**

Components that are not Cargo crates but still end up in, or next to, the binary:

- **Rust standard library** (`std`/`core`/`alloc`, MIT OR Apache-2.0) and `compiler_builtins`, which includes LLVM compiler-rt code under Apache-2.0 WITH LLVM-exception. These are statically linked. cargo-about does not list them, so the notices need a manual entry.
- **Windows:** the MSVC target links the Visual C++ runtime (`vcruntime140.dll`) dynamically by default. It is Microsoft-licensed and proprietary. The GPLv3 "System Libraries" definition excludes it from Corresponding Source, but if the installer redistributes the VC++ runtime, that redistribution is governed by Microsoft's terms. Counsel should confirm the chosen approach (static CRT with `+crt-static`, or relying on the OS/redist). GPUI CE compiles its DX11 shaders at build time with the OS `D3DCompile`, and no DXC DLL is bundled.
- **macOS:** Apple system frameworks only. No bundled binaries.
- **Fonts/assets:** GPUI CE ships IBM Plex Sans and Lilex under SIL OFL 1.1 in `assets/fonts`. They are used **only in `#[cfg(test)]` code and benches**. A scan of a local x86_64 Linux debug `pane` binary found no font bytes; only the family names are present as strings. That binary was a build of `main` in the primary checkout, and it was searched for a 256-byte slice from each TTF. Repeat this on release builds for each OS. When this audit was made Pane had no fonts, icons or images of its own. If Pane later embeds fonts or icons, add their notices; OFL fonts may not be sold on their own but may be bundled.
- **Pane's own embedded fonts and icons (manual-notice items):** `crates/pane` now compiles these into the binary with `include_bytes!`, where cargo tools cannot see them, so each needs a manual notice entry in the app's supplement (2.2):
  - **reicon 1.2.5** (MIT, Copyright (c) 2025 REICON; npm `reicon`, <https://github.com/dqev/reicon>): the glyphs vendored in [d67ceb5](https://github.com/hoangvu12/pane/commit/d67ceb57bfa250213a23cc1b4f2c37fdb39cf224) under `crates/pane/assets/icons/reicon/`, licence text in `crates/pane/assets/icons/reicon/LICENSE`, and, since #139, the whole set (every icon in both weights, which extensions draw by name) compiled into `pane-core` from `crates/pane-core/assets/reicon/`, the same licence text in `crates/pane-core/assets/reicon/LICENSE`. Both come from the same pinned tarball through `scripts/icons/vendor-reicon.py`. Its notice is that MIT licence text, once.
  - **Geist and Geist Mono** (SIL OFL 1.1, Copyright 2024 The Geist Project Authors): the TTFs under `crates/pane/assets/fonts/`, licence text in `crates/pane/assets/fonts/OFL.txt`. Bundling is allowed; the fonts may not be sold on their own.
  - The rest of `crates/pane/assets/icons/` (`pane/` and the mark) is Pane's own, under the app's licence.

### 1.3 (b) Guest SDK code inside extension components (`guests/`, target `wasm32-wasip2`, `no_std`)

The shipped content of `sample_rust.wasm`:

| Package | License |
| --- | --- |
| wit-bindgen 0.62.0 (runtime support + generated bindings) | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wasip3 0.9.0+wasi-0.3.0 (WASI 0.3 bindings; WIT from WebAssembly/WASI) | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| dlmalloc 0.2.14 (Rust port of Doug Lea's allocator) | MIT/Apache-2.0 |
| cfg-if 1.0.5 | MIT OR Apache-2.0 |
| Rust `core`/`alloc` + `compiler_builtins` (toolchain, not in Cargo.lock) | MIT OR Apache-2.0; compiler-rt parts Apache-2.0 WITH LLVM-exception |
| First-party `pane-guest`, the `wit/extension.wit` contract, sample code | **unlicensed today** |

There are 31 build-time-only guest packages (wit-bindgen macro/codegen, `wit-parser`, `wasm-encoder`, `indexmap`, …), all permissive. cargo-deny: `licenses ok: 0 errors`. `wasm-tools metadata show` reports the component as `processed-by wit-component [0.252.0]`, which is the componentization step of the toolchain's `wasm-component-ld`. The component is `no_std`, so wasi-libc is not linked. The `fixtures/` crates are test-only and are not distributed.

**Consequence:** today every third-party-code dependency in a Rust extension component is permissive. The only license an extension author inherits from Pane is the one Pane chooses for `pane-guest` and the WIT contract.

### 1.4 (c) Build-only tooling (not distributed with artifacts)

| Tool | License | Note |
| --- | --- | --- |
| `xtask` | first-party, **zero** third-party dependencies | Build/CI only |
| Rust toolchain 1.98.1 incl. `wasm-component-ld` | MIT OR Apache-2.0 (LLVM parts Apache-2.0 WITH LLVM-exception) | |
| wasm-tools 1.259.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Validation |
| wasi-sdk 34.0 (wasi-libc `2e6fb9d8ee0c`, LLVM 23.1.0) | LLVM: Apache-2.0 WITH LLVM-exception. wasi-libc: Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT; bundled parts are musl (MIT), cloudlibc (BSD-2-Clause), dlmalloc (CC0), emmalloc (MIT) and musl-fts (BSD-3-Clause), [per upstream LICENSE](https://github.com/WebAssembly/wasi-libc/blob/main/LICENSE) | Only the JS candidate uses it. The **libc part is linked into JS components**, so it is not purely build-time for them. |
| Build-time crates (proc-macros, `cc`, `pkg-config`, `wit-parser`, `codespan-reporting`, …) | all permissive (tables in 1.2/1.3) | Their code does not ship, except as generated code |
| CI: GitHub Actions, xvfb, xdotool, ImageMagick | various | Smoke tests only; not distributed |

### 1.5 JS backend candidate (**candidate, not yet selected**; see #2, [QJS spike](qjs-p3-port-spike/README.md))

| Component | License found | Ends up in |
| --- | --- | --- |
| componentize-qjs 0.4.5 (commit `e563c6d`), including the `componentize-qjs-runtime` crate | Apache-2.0 (root `LICENSE`, workspace `license`) | Each JS extension component (runtime) and the build tool (componentizer). Pane's `runtime-port.patch` would be a modification of Apache-2.0 code: §4(b) requires the modified files to carry change notices. |
| QuickJS via `rquickjs-sys` 0.13.0 (quickjs-ng sources) | MIT (Bellard, Gordon, Noordhuis, Ibarra Corretgé). `libregexp`, `libunicode` and `dtoa` are also MIT (Bellard). Unicode-derived tables may carry Unicode-3.0 terms. | Each JS extension component |
| rquickjs / rquickjs-core 0.13.0 | MIT | Each JS extension component |
| wasi-libc (wasi-sdk 34) | see 1.4 | Each JS extension component |
| Wizer / Wasmtime 47.0.4 (in the componentizer) | Apache-2.0 WITH LLVM-exception | Build-time only. The snapshot contains runtime state, not Wizer code. |
| Author's bundled npm packages (e.g. Fuse.js, Zod in the spike) | per package | The extension author is responsible |

Nothing copyleft was found in the candidate. If it is selected, its notices belong to **each JS component**, not to the Pane app, unless Pane ships a shared runtime.

### 1.6 Precedent checked

Zed's README: "Zed source code is licensed primarily under GPL-3.0-or-later, with Apache-2.0 components where marked," and it uses `cargo-about` with CI failing on unaccepted or missing licenses ([Zed README](https://github.com/zed-industries/zed#licensing)). Zed's extension API crate `zed_extension_api` (0.7.0 on crates.io) is licensed `Apache-2.0`. In GPUI CE each crate carries its own `LICENSE-APACHE`, the same per-crate marking pattern.

## 2. Proposal

### 2.1 Component split

| Component | Paths (today) | Proposed license | Why |
| --- | --- | --- | --- |
| **Application** (launcher, host runtime, UI, installer/packaging scripts) | `crates/pane`, `crates/pane-core`, future host crates, `scripts/` | **GPL-3.0-or-later** | Q40/Zed direction. Distributed modifications keep the copyleft source rights. Paid forks are allowed if they comply. |
| **Extension contract** | `wit/` (and future published WIT packages) | **Apache-2.0 OR MIT** (option: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`, matching wit-bindgen/wasip3) | Every extension embeds bindings generated from it. |
| **Guest SDKs** | `guests/pane-guest`, future JS/TS SDK, any Pane-maintained patch of componentize-qjs | **Apache-2.0 OR MIT** (the componentize-qjs fork stays **Apache-2.0** to match upstream) | This code is compiled *into* third-party components. A permissive license means Pane does not choose the extension's license. |
| **Sample extensions / templates** | `guests/sample-rust`, future templates | **Apache-2.0 OR MIT** (option: MIT-0 / 0BSD, which lets authors copy without keeping attribution) | Meant to be copied. |
| **Test fixtures** | `guests/fixtures/*` | same as the SDK | Not distributed. |
| **Build tooling** | `xtask/` | GPL-3.0-or-later (with the app) or Apache-2.0 OR MIT; low stakes | Not distributed. |
| **Docs** | `docs/`, `*.md` | Same as the app by default (option: CC-BY-4.0) | Low stakes. |

Mechanics, to be applied **only after the user decides**: add `LICENSE-GPL` at the root and `LICENSE-APACHE`/`LICENSE-MIT` in the permissive directories (or per-crate symlinks, as Zed and GPUI CE do). Add a `license = "…"` SPDX field in each first-party `Cargo.toml` and an SPDX header or a directory-level `LICENSE` in `wit/`. Add a README "Licensing" section that states the split, including that paths not marked otherwise are GPL-3.0-or-later.

**Checked against spec decision 22:**

- *Does not automatically assign one license to every third-party extension.* The things an extension must embed are the SDK, the generated bindings from the WIT, and the optional JS runtime (QuickJS, wasi-libc). With the split above they are all permissive, so no Pane-imposed license flows into a component through incorporated code. The split does **not** claim that every extension may use any license in every case. That question depends on the nature of the combination, and this audit takes no position on it (see decision D3). What it avoids is Pane *forcing* GPL through the SDK.
- *Does not prohibit compliant paid forks.* GPL-3.0-or-later permits charging for copies and modified versions, provided recipients get source and GPL rights. The permissive SDK/WIT can be reused commercially, including in paid extensions. Nothing in the split adds a non-commercial clause.

### 2.2 Notice ownership

| Artifact | Notice owner | Contents |
| --- | --- | --- |
| App binary + installer, per OS/arch | Pane project (release maintainer). Generated in CI from the tagged `Cargo.lock`. | `LICENSE` (GPL-3.0-or-later text) + `THIRD_PARTY_NOTICES.html` (cargo-about, per target) + a manual supplement: Rust std/compiler-rt, FreeType (Linux, if statically built), explicit `self_cell` Apache-2.0 election, rav1e `PATENTS`, `COPYRIGHT` files, the embedded reicon 1.2.5 icons (MIT, `crates/pane/assets/icons/reicon/LICENSE`; the whole set in `pane-core`, `crates/pane-core/assets/reicon/LICENSE`, is the same notice) and Geist fonts (OFL 1.1, `crates/pane/assets/fonts/OFL.txt`), and the JS runtime notices if the app ever ships a shared runtime. |
| Sample extension components (e.g. `sample_rust.wasm`) | Pane project, as its author/distributor | Distributed with the app today, so they are covered by the app's notice bundle. Published on their own, they need a sidecar `NOTICE`/`LICENSE` listing wit-bindgen, wasip3, dlmalloc, cfg-if and Rust core/compiler-rt. |
| Third-party extension components | **The extension author**. Pane documents what the SDK embeds and provides a generator. | The SDK README lists the licenses of embedded code. The package format should (later) include a license/notice file and an SPDX `license` field. |
| JS runtime (if selected) | Pane for the patched componentize-qjs source, the change notices and the per-component notice template; the extension author for the components they distribute | Notices for componentize-qjs (Apache-2.0), QuickJS, rquickjs (MIT) and wasi-libc (with its sub-components), generated by the JS toolchain into each component's package. |

Placement per OS (proposal): **Windows** puts `LICENSE.txt` and `THIRD_PARTY_NOTICES.html` in the install directory and links them from the installer license page. **macOS** puts them in `Pane.app/Contents/Resources/`. **Linux** places them in `share/doc/pane/` in tarball/deb/rpm, and inside the AppImage/Flatpak. Optionally the app also shows an in-app "Open-source licenses" view that renders the same file.

### 2.3 Source-obligation procedure (per release)

1. Tag the exact commit. Release notes name the tag, the pinned toolchain and the lockfile hash.
2. Publish a **Corresponding Source** asset next to the binaries on the same release page: `git archive` of the tag, plus `cargo vendor` output for both workspaces and the GPUI CE and wgsl-rs git dependencies. Include the build scripts, the installer/packaging scripts and `rust-toolchain.toml`. Vendoring is conservative: it also meets MPL-2.0 file-source availability and removes dependence on crates.io and GitHub availability.
3. Keep that source available for as long as the binaries are offered from the same place. This follows GPLv3 §6(d) (equivalent access from the same place); counsel should confirm it for the chosen hosts.
4. If binaries are code-signed or notarized, record that the signing keys are not Installation Information for general-purpose desktop OSs (GPLv3 §6 "User Product" wording). **Have counsel confirm.**
5. For a JS runtime fork, publish the patched componentize-qjs source (Apache-2.0, with change notices) in the same way. Apache does not require source, but publishing it keeps JS components reproducible.
6. Changing any dependency or `Cargo.lock` re-runs the checks in 2.4. Adding a new language runtime, bundled binary, font or icon requires a manual notice review, because cargo tools cannot see it (this is the #3 exit condition: "targeted notice/obligation recheck").

### 2.4 Generating and checking notices in CI

Proposed (not applied): a `license` CI job on one runner, since cargo metadata supports cross-target filtering.

- `cargo deny --locked --workspace --exclude-dev check licenses bans sources` with a root `deny.toml` covering the six targets. A second run with `--manifest-path guests/Cargo.toml` and `targets = ["wasm32-wasip2"]`. It fails on any license outside the allow-list, on missing license metadata without a `clarify`, and on unknown git sources.
- `cargo about generate --locked --workspace -c about.toml about.hbs > THIRD_PARTY_NOTICES.html` (and the guests equivalent). It fails if cargo-about reports an unaccepted or unresolvable license. At release time it is regenerated from the tag; it is also committed or diffed so changes show up in review.
- Once first-party `license` fields exist, the job checks that each first-party crate declares the license expected for its path (the app is GPL, and `pane-guest`/samples are permissive). This is a small xtask check.
- A check that fails if new `-sys`/bundled C crates, `include_bytes!` of font or image assets, or `.dll`/`.dylib`/`.so` files in packaging appear without a manual-notice entry.

The audit allow-list is the set of expressions found, all of which were compatible above:

```toml
[licenses]
allow = ["MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0", "0BSD",
  "CC0-1.0", "BSL-1.0", "Unlicense", "MPL-2.0"]
confidence-threshold = 0.9
private = { ignore = true }
# plus [[licenses.clarify]] for wgsl-rs, wgsl-rs-ir, wgsl-rs-macros = "MIT"
```

Keep `GPL-2.0-only`, `AGPL-*`, `LGPL-*`, `SSPL-*`, `BUSL-*` and `OFL-1.1` out of the allow-list. Adding any of them should require a manual review. OFL becomes acceptable only if fonts are deliberately bundled. For cargo-about, the `clarify` for `wgsl-rs` uses `[[wgsl-rs.clarify.git]] path = "LICENSE"` plus its sha256. Once first-party crates carry license fields, keep `private = { ignore = true }` or add `GPL-3.0-or-later` to the app-side allow-list.

## 3. Decisions needed from the user

Consequential decisions are marked ★.

1. ★ **D1: confirm GPL-3.0-or-later for the application** (versus GPL-3.0-only, or AGPL-3.0-or-later if source rights for users of modified network services are a goal). The Q40 direction is GPL-3.0-or-later.
2. ★ **D2: license for the SDK, the WIT contract and samples.** (a) `Apache-2.0 OR MIT`, as proposed. (b) `Apache-2.0` only, as Zed's `zed_extension_api` does. (c) the Bytecode Alliance triple `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. (d) GPL, which would effectively require GPL-compatible extensions. Samples could additionally be MIT-0/0BSD.
3. ★ **D3: an explicit extension permission, yes or no.** (a) No additional permission: rely on the permissive SDK and contract, and state no position on extension licensing (Zed-like). (b) Add a GPLv3 §7 additional permission saying that components that interact with Pane only through the published WIT contract may use any license. That gives paid or proprietary extension authors more certainty, but downstream forks may remove it (§7). (c) Require extensions in any future Pane catalog to be under an OSI license. That would be a catalog policy, not a code license. Counsel should review whichever is chosen.
4. ★ **D4: contribution terms and copyright holder.** Choose inbound = outbound with a DCO sign-off, or a CLA. Only a CLA or copyright assignment keeps the ability to relicense or dual-license the app later. Also decide the name used in notices ("Pane contributors" or a named person or entity).
5. **D5: build tooling and docs.** Options are xtask under GPL or permissive, and docs under the app license or CC-BY-4.0. Low stakes.
6. **D6: Windows CRT and packaging.** Choose a static CRT or dependence on the VC++ redistributable, and decide whether Linux packages bundle system libraries (AppImage/Flatpak). This changes the notice bundle; it is not a policy choice.
7. **D7: whether to ask GPUI CE upstream** to trim `image` features (removing rav1e and its AOM `PATENTS`) and to add `license` fields to wgsl-rs. This is optional hygiene.
8. **D8: trademark policy for the name "Pane"** (the GPL grants no trademark rights). This is separate from #3 and can be deferred, and name availability has not yet been established (Q41).

Once D1–D4 are answered, the #3 exit steps are: apply the licenses to their components, add the CI job from 2.4, and do a dry run of the 2.3 procedure for one OS artifact.
