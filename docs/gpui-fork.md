# Maintained GPUI CE dependency

Pane uses [hoangvu12/gpui-ce](https://github.com/hoangvu12/gpui-ce), branch
`pane/source-over-alpha`, to carry the Windows alpha correction, the macOS
startup ABI repair and the under-window vibrancy material selection required by
[#63](https://github.com/pane-app/pane/issues/63), under
[#61](https://github.com/pane-app/pane/issues/61). Cargo uses an immutable commit,
not the branch tip: `beb5a6dada242a4aad9f972546021a6fb6c869ba`. All four declarations in `crates/pane/Cargo.toml` (including
the test dependency) move together. The fork's internal path dependencies resolve
to that same Git source, preserving one GPUI type identity across renderer,
platforms and editable controls. `Cargo.lock` records the full closure, and
`deny.toml` permits the maintained source. No vendored platform crate or local
path override is required.

## Provenance and scope

The base is upstream GPUI CE `17d9c8e8fdb30a329d817ca06bff424e8e848f1a`.
The source reference is Zui `ec16c62b83caa58b61cd8039db4b5721bacf948f`, specifically
[its DirectX blend functions](https://github.com/hoangvu12/zui/blob/ec16c62b83caa58b61cd8039db4b5721bacf948f/crates/gpui_windows/src/directx_renderer.rs).
Zui is a separate Zed GPUI lineage, not the dependency used by Pane. Both Windows
renderer packages are Apache-2.0; their license and attribution files are retained.
The fork adds its patch rationale and maintenance instructions in `PANE-FORK.md`.

The Windows correction changes ordinary scene blending and path-sprite destination alpha from
additive to source-over (`As + Ad * (1 - As)`). RGB behavior is unchanged.
Path rasterization already uses source-over and subpixel text intentionally does
not write alpha; neither needs a patch. CE already has in-window blur support;
no additional Zui blur engine is imported. macOS now selects the under-window
vibrancy material, adapted from Zui (see the macOS section below). No Linux
compositor integration is added.

Linux needs none of it. #67's audit of CE's own Linux backends found X11
window transparency — engaged only for a non-opaque window appearance — and
the KDE `org_kde_kwin_blur` Wayland protocol, requested only for the Blurred
appearance, plus in-scene backdrop blur. The launcher never requests a
blurred window on Linux: the material policy normalizes a glass request to
the opaque window and solid panel at construction, so no Zui blur patch or
compositor-specific integration is imported for Linux (see
[docs/launcher-ui-validation.md](launcher-ui-validation.md)).

This renderer correction alone does not establish useful native glass. The
prototype also removed outer panel shadows that obscured the desktop. Its branch,
working tree and copied Windows dependency are preserved independently by #62;
this ticket neither rewrites nor removes them.

## Reproduce

From Pane, with the repository's Rust toolchain and Windows native prerequisites:

```powershell
cargo fetch --locked
cargo build --locked -j1 -p pane
cargo xtask guests
cargo test --locked -j1 -p pane --test integration -- window:: command_search::
```

For renderer regression, clone the maintained fork separately and check out the
exact revision in Pane's manifest. From that checkout:

```powershell
$env:GPUI_ALPHA_OUTPUT_DIR = "$PWD/target/alpha-output"
cargo +1.98.1 test --locked -j1 -p gpui_ce_windows --features test-support --lib translucent_ -- --nocapture
cargo +1.98.1 test --locked -j1 -p gpui_ce_windows --features test-support --lib
```

The fixture uses a hidden HWND and real Direct3D rendering and texture readback.
It never presents, changes focus or captures the desktop. It tests transparent
clear pixels, each single layer, and red/blue overlap through both ordinary quad
and path-sprite rendering. Two 50% layers should yield approximately premultiplied
RGBA `[64, 0, 128, 191]`, leaving 25% of the backdrop visible. Optional PNGs retain
actual GPU RGBA and side-by-side black/green composite previews; the previews
illustrate alpha and do not simulate or certify native blur.

To prove sensitivity, in an isolated fork checkout restore the two destination
alpha values to `ONE`, leaving tests in place: both should fail with alpha 254/255 (8-bit rounding).
Restore only the ordinary blend fix: the quad case should pass and the path case
should still fail. Restore both and run the whole renderer suite.

## Update or retire

Review a new CE revision and the fork diff; port only fixes still needed. Run the
renderer positive/negative controls, launcher suites and native Windows launch.
Publish a new immutable fork commit, update all renderer/platform/control pins
and regenerate the lockfile without unrelated dependency upgrades. Check
`cargo metadata --locked` for one `gpui-ce` package and one CE source revision.
Keep platform packages in the same closure even when testing only Windows.

When upstream contains the correction and output regression, retire the fork by
moving all pins together to that verified upstream revision and updating the
source allow list. Do not repair a dependency by editing Cargo's cache.
Native macOS/Linux results remain separate work; compiling their source or
passing GPUI's test platform does not establish native runtime behavior.

## Recorded validation

[The renderer evidence record](research/gpui-alpha/README.md) retains native GPU
readbacks, positive/negative test output and SHA-256 artifact hashes. Cargo fetched
the published fork; metadata resolves 19 CE packages to that single source and one
`gpui-ce` identity. Lockfile review confirms that only those 19 source entries
changed, with no package version or dependency-list changes.

On 2026-10-02, Windows 11 build 26200 / RTX 5050 / Rust 1.98.1:

- `cargo test --locked -j1 -p pane --test window --test command_search`: 49
  launcher-window tests and one command-search test passed (exit 0). Tests used
  copies of the existing assembled Rust/JavaScript/TypeScript guest fixtures;
  guest toolchains were not rebuilt by this renderer-only slice.
- `cargo build --locked -j1 -p pane`: normal non-test build passed (exit 0).
  It compiled the remote fork into this worktree's own target directory, with
  no copied platform crate, local dependency override or manual Cargo-cache edit.
- `cargo fmt --all -- --check`, fork package formatting and `git diff --check`
  passed. `cargo-deny` was not installed locally; final integrated CI owns its
  license/source-policy execution and the macOS/Linux builds and tests.

[Application test output](research/gpui-alpha/application-tests.txt) is retained.
The test-platform IME/accessibility checks are not native IME or screen-reader
certification. Native compositor checks and the styled launcher remain separate.

The [native fork integration capture](research/gpui-alpha/native/README.md)
confirms that the normal binary opens on this Windows host, typing `rust`
filters the real sample results, and Escape restores root search. The capture
records the source revision, binary hash, OS, 96 DPI, verified focus and isolated
process cleanup. This is the existing launcher appearance, before presentation
integration; no native-glass claim is made.

## macOS default-startup ABI repair

Final CI run [36952982441](https://github.com/pane-app/pane/actions/runs/36952982441)
(artifact 11206345698) exposed a default-glass startup abort in the previous pin
`2b9e644e3f89a38eacebc713fb0d1807c618c76c`. The retained installed-stderr log reports:

```text
invalid message send to -[NSViewBackingLayer setBackgroundColor:]: expected argument at index 0 to have type code '^{CGColor=}', but found '@'
panic in a function that cannot unwind
```

Fork `bcf3a0acd047c1873293069d0ed42085a38f699b` changes that argument in
`remove_layer_background` to `ptr::null::<objc2_core_graphics::CGColor>()`.
The old Objective-C `NIL` encoded an object; CALayer requires a CGColor pointer.
objc2 debug signature verification consequently panicked inside the non-unwinding
`blurred_view_update_layer` callback. The direct macOS CGColor dependency uses an
already-locked package/version. The sibling NSWindow background setter takes an
NSColor object and correctly retains its object argument. The audit found no
other raw background/border/shadow-color layer setters in gpui_macos/gpui_apple.

The focused macOS test calls actual recursive clearing on colored parent/child
CALayers, asserting both backgrounds clear and child opacity is preserved:

```sh
cargo +1.98.1 test --locked -j1 -p gpui_ce_macos --features font-kit --lib remove_layer_background_clears_cgcolor_recursively
```

For a negative control on macOS, retain the test and restore only the production
argument to `NIL`; the signature mismatch should fail the test before clearing.
This test and the application's separate default-startup CI smoke must validate
the repair natively. The Windows development host ran formatting/manifest checks,
not macOS compilation or runtime; no native positive result is claimed yet.
The Windows renderer checkout in CI moves with all four Pane pins and retains
its full 15-test command. Windows source-over code and historical alpha evidence
are unchanged; deny.toml already permits the same maintained repository.
This bounded ABI repair changes no material selection or blur policy and does
not complete #66 or establish native desktop-blur quality.

## macOS under-window vibrancy material (Pane #66)

The pin `beb5a6dada242a4aad9f972546021a6fb6c869ba` selects
`NSVisualEffectMaterial::UnderWindowBackground` with behind-window blending
in the fork's `blurred_view_init_with_frame`, replacing `Selection`: on
macOS 26+, `Selection` no longer vends the `CABackdropLayer` that
`remove_layer_background` walks, so a blur request rendered with no blur at
all. `blurred_view_update_layer` additionally lays an opaque black base under
the backdrop sublayer, so Mission Control and Spaces window snapshots (which
omit backdrop layers) read as a solid dark surface rather than fully
transparent glass. The correction is adapted from Zui's
`blurred_view_init_with_frame` — the wingleeio/zed GPUI lineage, read from
Zui's current `crates/gpui_macos/src/window.rs` (the spec's `ec16c62b`
evidence anchor is no longer fetchable); only the material selection and its
snapshot base are adapted, no Zui blur engine or other Zui change.

Two fork tests pin the seam. One reads `material`, `blendingMode` and `state`
back from the real registered subclass; the other exercises the real
layer-tree function, checking the root layer holds the opaque black base
while the recursed sublayer stays cleared:

```sh
cargo +1.98.1 test --locked -j1 -p gpui_ce_macos --features font-kit --lib blurred_view
cargo +1.98.1 test --locked -j1 -p gpui_ce_macos --features font-kit --lib remove_layer_background_clears_cgcolor_recursively
```

The release matrix's macos-renderer job runs both on macos-15 against the
pinned revision (and the windows-renderer job now also derives its fork
checkout from that pin, replacing its hardcoded revision), the verify tier's
Check (macos-15) compiles the fork, and the macOS default-startup smoke
exercises the default-glass startup path post-merge. None of this establishes
native vibrancy quality: the dark/light desktop-blur validation with an
external edge pattern, window movement/resize/activation, scaled/Retina
displays and reduced-transparency behavior remain outstanding native work,
recorded in [docs/launcher-ui-validation.md](launcher-ui-validation.md).

The pin now sits above `70ed267181dc56e4739d5782eb56fae51a2afd29` (Pane
#251's empty-copy propagation, consumed by the in-flight launcher-polish
PR), carrying every prior fix forward as one superset. The Windows
source-over correction and its regression coverage are unchanged.

## Pop-up tracking area return type

Fork `5d27954ce5305447bb97d1d7b89b0db9b7a2c59c` (also on the fork's `main`)
types a pop-up window's `-[NSTrackingArea initWithRect:options:owner:userInfo:]`
as returning the area, not `()`. objc2's debug signature verification panicked on
the old `()` inside a macOS callback, which aborted every debug build the first
time it opened a pop-up: Pane's HUD, which the macOS smoke reached after opening a
found file (release run 37730452204). Release builds skip the check and were not
affected.
