# Maintained GPUI CE dependency

Pane uses [hoangvu12/gpui-ce](https://github.com/hoangvu12/gpui-ce), branch
`pane/source-over-alpha`, to carry the Windows alpha correction, macOS startup ABI repair and
editable-text empty-copy propagation required by
[#63](https://github.com/pane-app/pane/issues/63), under
[#61](https://github.com/pane-app/pane/issues/61). Cargo uses an immutable commit,
not the branch tip: `70ed267181dc56e4739d5782eb56fae51a2afd29`. All four declarations in `crates/pane/Cargo.toml` (including
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
no additional Zui blur engine is imported. macOS still selects the old Selection
material; its material change and native verification belong to later work.
No Linux compositor integration is added.

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

## Pop-up tracking area return type

Fork `5d27954ce5305447bb97d1d7b89b0db9b7a2c59c` (also on the fork's `main`)
types a pop-up window's `-[NSTrackingArea initWithRect:options:owner:userInfo:]`
as returning the area, not `()`. objc2's debug signature verification panicked on
the old `()` inside a macOS callback, which aborted every debug build the first
time it opened a pop-up: Pane's HUD, which the macOS smoke reached after opening a
found file (release run 37730452204). Release builds skip the check and were not
affected.

## Empty editable-text copy propagates

Fork `70ed267181dc56e4739d5782eb56fae51a2afd29` (a fast-forward on
`pane/source-over-alpha` above `5d27954ce5305447bb97d1d7b89b0db9b7a2c59c`)
makes `EditableTextState::copy` in `gpui_ce_elements` call `cx.propagate()`
when the field's selection is empty, instead of ending the action having
done nothing. A copy with a selection still writes it and stops the action
as before; `cut` and every other editable-text action are unchanged, and no
keybinding changes.

Pane's launcher rule ([#251](https://github.com/pane-app/pane/issues/251))
needs that fall-through: the keymap dispatches a focused field's own copy
first, so Ctrl+C with a non-empty selection copies the text, and with none
the key must reach the launcher's chord handling, which runs the selected
row's action bound to the same chord. Pane's `crates/pane/tests/chords.rs`
covers both halves through the window. The fork's in-file unit test
(`test_copy_without_selection_copies_nothing`) pins that an empty selection
writes nothing to the clipboard:

```sh
cargo +1.98.1 test --locked -p gpui_ce_elements --lib test_copy_without
```

No platform, renderer or layout code changes; the same 19 lockfile source
entries move to the new commit.
