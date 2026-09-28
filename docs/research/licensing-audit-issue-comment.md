<!-- Draft comment for hoangvu12/pane#3. Not posted; the orchestrator decides. -->

**Licensing audit, bounded work done (not legal advice).** Full evidence: [`docs/research/licensing-audit.md`](https://github.com/hoangvu12/pane/blob/main/docs/research/licensing-audit.md) (link valid once merged). No license has been applied.

**Facts** (commit `b1a005d`; cargo-deny 0.20.2, cargo-about 0.9.2, `cargo metadata --filter-platform` for x86_64/aarch64 Windows, macOS and Linux, plus `wasm32-wasip2` for the guests):

- **App graph:** 381–488 shipped third-party crates per target. All are permissive or GPLv3-compatible, and cargo-deny reports `licenses ok` with the proposed allow-list. **GPUI CE @ `17d9c8e`** is Apache-2.0 (root `LICENSE.md` and per-crate `LICENSE-APACHE`, © Zed Industries), with no NOTICE file.
- **Items to handle:**
  - `self_cell` (`Apache-2.0 OR GPL-2.0-only`, Linux): elect the Apache-2.0 branch.
  - `option-ext` (MPL-2.0): make its source available.
  - `wgsl-rs`/`wgsl-rs-ir` have no license metadata; the repo `LICENSE` is MIT, so add a clarify entry.
  - `freetype-sys` bundles FreeType (FTL OR GPL-2.0+) C source that cargo tooling cannot see. It is compiled only when system freetype is missing and needs a manual notice in that case.
  - `rav1e` (pulled in by `image` defaults) has an AOM patent license file.
  - The Rust std/compiler-rt notices and the Windows VC++ runtime need manual handling.
- **Nothing found** under GPL-2.0-only (without an alternative), AGPL, LGPL-static, proprietary or unknown licenses.
- **Fonts:** GPUI CE's OFL fonts are test-only and not embedded in the binary.
- **Guest components** embed only wit-bindgen, wasip3, dlmalloc, cfg-if and Rust core, all permissive. First-party `pane-guest` and `wit/` are unlicensed today.
- **JS candidate (not selected):** componentize-qjs is Apache-2.0; QuickJS/rquickjs are MIT; wasi-libc is Apache/MIT with musl/BSD/CC0 parts. It would be embedded per component.

**Proposal:**

- App: GPL-3.0-or-later.
- `wit/`, `pane-guest`, the future JS SDK and samples: `Apache-2.0 OR MIT`. A componentize-qjs fork stays Apache-2.0.
- Pane owns the app notices, generated per target by cargo-about plus a manual supplement. Extension authors own their component notices, and the SDK documents what it embeds.
- Each release publishes a Corresponding Source asset (tag archive + `cargo vendor`).
- A CI job runs cargo-deny and cargo-about for both workspaces.

This proposal keeps paid forks allowed and does not force GPL onto extensions through incorporated code. It takes no position on WIT/WASI and derivative works.

**Decisions needed:**

1. Confirm GPL-3.0-or-later for the app (versus `-only` or AGPL).
2. SDK/WIT/sample license: `Apache-2.0 OR MIT`, Apache-2.0 only, or the Bytecode Alliance triple (optionally MIT-0 for samples).
3. Explicit extension permission: none (Zed-like), a GPLv3 §7 permission for components using the WIT contract, or a catalog OSI requirement.
4. Contribution terms (DCO or CLA; only a CLA keeps relicensing possible) and the copyright-holder name.

Minor: xtask/docs license, Windows CRT and Linux bundling, upstream hygiene requests, and trademark (deferred).
