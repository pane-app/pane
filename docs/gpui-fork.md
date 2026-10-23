# Maintained GPUI CE dependency

Pane draws its window with GPUI CE, but it builds against a maintained fork:
[hoangvu12/gpui-ce](https://github.com/hoangvu12/gpui-ce), branch
`pane/source-over-alpha`. This page is for whoever updates that dependency.
It says what the fork changes, how to check it, and how to move the pin or
retire the fork.

The fork carries these fixes, required by
[#63](https://github.com/pane-app/pane/issues/63) under
[#61](https://github.com/pane-app/pane/issues/61):

- the Windows alpha correction
  ([provenance and scope](#provenance-and-scope));
- the macOS startup ABI repair
  ([macOS default-startup ABI repair](#macos-default-startup-abi-repair));
- the under-window vibrancy material selection
  ([macOS under-window vibrancy material](#macos-under-window-vibrancy-material-pane-66));
- the pop-up tracking area return type
  ([pop-up tracking area return type](#pop-up-tracking-area-return-type));
- the editable-text empty-copy propagation
  ([empty editable-text copy propagates](#empty-editable-text-copy-propagates)).

## How Pane pins the fork

Cargo uses an immutable commit, not the branch tip:
`beb5a6dada242a4aad9f972546021a6fb6c869ba`. All four declarations in
`crates/pane/Cargo.toml` (including the test dependency) move together.

The fork's internal path dependencies resolve to that same Git source. That
keeps one GPUI type identity across renderer, platforms and editable
controls. `Cargo.lock` records the full closure, and `deny.toml` permits the
maintained source. No vendored platform crate or local path override is
required.

## Provenance and scope

The base is upstream GPUI CE `17d9c8e8fdb30a329d817ca06bff424e8e848f1a`.
The source reference is Zui `ec16c62b83caa58b61cd8039db4b5721bacf948f`,
specifically
[its DirectX blend functions](https://github.com/hoangvu12/zui/blob/ec16c62b83caa58b61cd8039db4b5721bacf948f/crates/gpui_windows/src/directx_renderer.rs).
Zui is a separate Zed GPUI lineage, and Pane does not depend on it.

Both Windows renderer packages are Apache-2.0, and their license and
attribution files are retained. The fork explains its patches and how to
maintain them in `PANE-FORK.md`.

### The Windows correction

The Windows correction changes ordinary scene blending and path-sprite
destination alpha from additive to source-over (`As + Ad * (1 - As)`). RGB
behavior is unchanged.

Path rasterization already uses source-over, and subpixel text intentionally
does not write alpha, so neither needs a patch. CE already has in-window blur
support, so no additional Zui blur engine is imported. macOS now selects the
under-window vibrancy material, adapted from Zui (see the macOS section
below). No Linux compositor integration is added.

### Linux

Linux needs none of it. #67's audit of CE's own Linux backends found:

- X11 window transparency, engaged only for a non-opaque window appearance;
- the KDE `org_kde_kwin_blur` Wayland protocol, requested only for the
  Blurred appearance;
- in-scene backdrop blur.

The launcher never requests a blurred window on Linux. The material policy
normalizes a glass request to the opaque window and solid panel at
construction. So no Zui blur patch or compositor-specific integration is
imported for Linux (see
[the archived launcher UI validation](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/launcher-ui-validation.md)).

### What the correction does not do

This renderer correction alone does not establish useful native glass. The
prototype also removed outer panel shadows that obscured the desktop. Its
branch, working tree and copied Windows dependency are preserved
independently by #62; this ticket neither rewrites nor removes them.

## Reproduce

From Pane, with the repository's Rust toolchain and Windows native
prerequisites:

```powershell
cargo fetch --locked
cargo build --locked -j1 -p pane
cargo xtask guests
cargo test --locked -j1 -p pane --test integration -- window:: command_search::
```

### Renderer regression

Clone the maintained fork separately and check out the exact revision in
Pane's manifest. From that checkout:

```powershell
$env:GPUI_ALPHA_OUTPUT_DIR = "$PWD/target/alpha-output"
cargo +1.98.1 test --locked -j1 -p gpui_ce_windows --features test-support --lib translucent_ -- --nocapture
cargo +1.98.1 test --locked -j1 -p gpui_ce_windows --features test-support --lib
```

The fixture uses a hidden HWND with real Direct3D rendering and texture
readback. It never presents, changes focus or captures the desktop.

It tests transparent clear pixels, each single layer, and red/blue overlap,
through both ordinary quad and path-sprite rendering. Two 50% layers should
yield approximately premultiplied RGBA `[64, 0, 128, 191]`, leaving 25% of
the backdrop visible.

Optional PNGs keep the actual GPU RGBA and side-by-side black/green
composite previews. The previews illustrate alpha; they do not simulate or
certify native blur.

### Proving the tests catch a regression

In an isolated fork checkout, leave the tests in place and:

1. Restore the two destination alpha values to `ONE`. Both cases should fail
   with alpha 254/255 (8-bit rounding).
2. Restore only the ordinary blend fix. The quad case should pass and the
   path case should still fail.
3. Restore both and run the whole renderer suite.

## Update or retire

### Moving to a new CE revision

1. Review the new CE revision and the fork diff. Port only fixes that are
   still needed.
2. Run the renderer positive/negative controls, the launcher suites and a
   native Windows launch.
3. Publish a new immutable fork commit.
4. Update all renderer/platform/control pins and regenerate the lockfile
   without unrelated dependency upgrades.
5. Check `cargo metadata --locked` for one `gpui-ce` package and one CE
   source revision.

Keep platform packages in the same closure even when testing only Windows.

### Retiring the fork

When upstream contains the correction and output regression, retire the
fork. Move all pins together to that verified upstream revision and update
the source allow list. Do not repair a dependency by editing Cargo's cache.

Native macOS/Linux results remain separate work. Compiling their source, or
passing GPUI's test platform, does not establish native runtime behavior.

## Recorded validation

[The renderer evidence record](research/gpui-alpha/README.md) keeps native
GPU readbacks, positive/negative test output and SHA-256 artifact hashes.
Cargo fetched the published fork, and metadata resolves 19 CE packages to
that single source and one `gpui-ce` identity. Lockfile review confirms that
only those 19 source entries changed, with no package version or
dependency-list changes.

On 2026-10-02, Windows 11 build 26200 / RTX 5050 / Rust 1.98.1:

- `cargo test --locked -j1 -p pane --test window --test command_search`: 49
  launcher-window tests and one command-search test passed (exit 0). Tests
  used copies of the existing assembled Rust/JavaScript/TypeScript guest
  fixtures; this renderer-only slice did not rebuild guest toolchains.
- `cargo build --locked -j1 -p pane`: normal non-test build passed (exit 0).
  It compiled the remote fork into this worktree's own target directory,
  with no copied platform crate, local dependency override or manual
  Cargo-cache edit.
- `cargo fmt --all -- --check`, fork package formatting and
  `git diff --check` passed. `cargo-deny` was not installed locally; final
  integrated CI owns its license/source-policy execution and the macOS/Linux
  builds and tests.

[Application test output](research/gpui-alpha/application-tests.txt) is
retained. The test-platform IME/accessibility checks are not native IME or
screen-reader certification. Native compositor checks and the styled
launcher remain separate.

The [native fork integration capture](research/gpui-alpha/native/README.md)
confirms three things on this Windows host: the normal binary opens, typing
`rust` filters the real sample results, and Escape restores root search. The
capture records the source revision, binary hash, OS, 96 DPI, verified focus
and isolated process cleanup. It shows the existing launcher appearance,
before presentation integration, and makes no native-glass claim.

## macOS default-startup ABI repair

Final CI run
[36952982441](https://github.com/pane-app/pane/actions/runs/36952982441)
(artifact 11206345698) exposed a default-glass startup abort in the previous
pin `2b9e644e3f89a38eacebc713fb0d1807c618c76c`. The retained installed-stderr
log reports:

```text
invalid message send to -[NSViewBackingLayer setBackgroundColor:]: expected argument at index 0 to have type code '^{CGColor=}', but found '@'
panic in a function that cannot unwind
```

### The fix

Fork `bcf3a0acd047c1873293069d0ed42085a38f699b` changes that argument in
`remove_layer_background` to `ptr::null::<objc2_core_graphics::CGColor>()`.
The old Objective-C `NIL` encoded an object, but CALayer requires a CGColor
pointer. So objc2's debug signature verification panicked inside the
non-unwinding `blurred_view_update_layer` callback.

The direct macOS CGColor dependency uses an already-locked package/version.
The sibling NSWindow background setter takes an NSColor object and correctly
keeps its object argument. The audit found no other raw
background/border/shadow-color layer setters in gpui_macos/gpui_apple.

### Testing it

The focused macOS test calls the actual recursive clearing on colored
parent/child CALayers. It asserts that both backgrounds clear and that child
opacity is preserved:

```sh
cargo +1.98.1 test --locked -j1 -p gpui_ce_macos --features font-kit --lib remove_layer_background_clears_cgcolor_recursively
```

For a negative control on macOS, keep the test and restore only the
production argument to `NIL`. The signature mismatch should fail the test
before clearing.

This test and the application's separate default-startup CI smoke must
validate the repair natively. The Windows development host ran
formatting/manifest checks, not macOS compilation or runtime, so no native
positive result is claimed yet.

### What it leaves unchanged

The Windows renderer checkout in CI moves with all four Pane pins and keeps
its full 15-test command. Windows source-over code and historical alpha
evidence are unchanged, and deny.toml already permits the same maintained
repository.

This bounded ABI repair changes no material selection or blur policy. It
does not complete #66 or establish native desktop-blur quality.

## macOS under-window vibrancy material (Pane #66)

The pin `beb5a6dada242a4aad9f972546021a6fb6c869ba` selects
`NSVisualEffectMaterial::UnderWindowBackground` with behind-window blending
in the fork's `blurred_view_init_with_frame`, replacing `Selection`. On
macOS 26+, `Selection` no longer vends the `CABackdropLayer` that
`remove_layer_background` walks, so a blur request rendered with no blur at
all.

`blurred_view_update_layer` also lays an opaque black base under the
backdrop sublayer. Mission Control and Spaces window snapshots omit backdrop
layers, so with the base they read as a solid dark surface rather than fully
transparent glass.

The correction is adapted from Zui's `blurred_view_init_with_frame`, from the
wingleeio/zed GPUI lineage. It was read from Zui's current
`crates/gpui_macos/src/window.rs`, because the spec's `ec16c62b` evidence
anchor is no longer fetchable. Only the material selection and its snapshot
base are adapted; no Zui blur engine or other Zui change comes with it.

### Tests

Two fork tests pin the seam. One reads `material`, `blendingMode` and `state`
back from the real registered subclass. The other exercises the real
layer-tree function, checking that the root layer holds the opaque black
base while the recursed sublayer stays cleared:

```sh
cargo +1.98.1 test --locked -j1 -p gpui_ce_macos --features font-kit --lib blurred_view
cargo +1.98.1 test --locked -j1 -p gpui_ce_macos --features font-kit --lib remove_layer_background_clears_cgcolor_recursively
```

In CI:

- The release matrix's macos-renderer job runs both on macos-15 against the
  pinned revision. The windows-renderer job now also derives its fork
  checkout from that pin, replacing its hardcoded revision.
- The verify tier's Check (macos-15) compiles the fork.
- The macOS default-startup smoke exercises the default-glass startup path
  post-merge.

None of this establishes native vibrancy quality. The outstanding native
work is recorded in
[the archived launcher UI validation](https://github.com/pane-app/pane/blob/archive/docs-history-2026-10-23/docs/launcher-ui-validation.md):
the dark/light desktop-blur validation with an external edge pattern, window
movement/resize/activation, scaled/Retina displays and reduced-transparency
behavior.

### Where the pin sits

The pin now sits above `70ed267181dc56e4739d5782eb56fae51a2afd29` (Pane
#251's empty-copy propagation, consumed by the in-flight launcher-polish
PR), carrying every prior fix forward as one superset. The Windows
source-over correction and its regression coverage are unchanged.

## Pop-up tracking area return type

Fork `5d27954ce5305447bb97d1d7b89b0db9b7a2c59c` (also on the fork's `main`)
types a pop-up window's `-[NSTrackingArea initWithRect:options:owner:userInfo:]`
as returning the area instead of `()`.

With the old `()`, objc2's debug signature verification panicked inside a
macOS callback. That aborted every debug build the first time it opened a
pop-up: Pane's HUD, which the macOS smoke reached after opening a found file
(release run 37730452204). Release builds skip the check and were not
affected.

## Empty editable-text copy propagates

Fork `70ed267181dc56e4739d5782eb56fae51a2afd29` is a fast-forward on
`pane/source-over-alpha` above `5d27954ce5305447bb97d1d7b89b0db9b7a2c59c`.
It makes `EditableTextState::copy` in `gpui_ce_elements` call
`cx.propagate()` when the field's selection is empty. Before, the action
ended having done nothing.

A copy with a selection still writes it and stops the action as before.
`cut` and every other editable-text action are unchanged, and no keybinding
changes.

### Why Pane needs it

Pane's launcher rule ([#251](https://github.com/pane-app/pane/issues/251))
needs that fall-through. The keymap dispatches a focused field's own copy
first. So Ctrl+C with a non-empty selection copies the text, and with none
the key must reach the launcher's chord handling, which runs the selected
row's action bound to the same chord.

Pane's `crates/pane/tests/chords.rs` covers both halves through the window.
The fork's in-file unit test (`test_copy_without_selection_copies_nothing`)
pins that an empty selection writes nothing to the clipboard:

```sh
cargo +1.98.1 test --locked -p gpui_ce_elements --lib test_copy_without
```

No platform, renderer or layout code changes, and the same 19 lockfile
source entries move to the new commit.
