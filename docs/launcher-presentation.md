# Launcher presentation ownership

[#62](https://github.com/hoangvu12/pane/issues/62) establishes the organization
accepted by [#61](https://github.com/hoangvu12/pane/issues/61) without changing
appearance or launcher behavior.

| Module in `crates/pane/src` | Responsibility |
| --- | --- |
| `main.rs` | Startup, runtime setup, application/window creation |
| `lib.rs` | Narrow public entry points and window/action re-exports |
| `app.rs` | Window state, navigation, dispatch, screen selection and shared frame; maps core rows to presentation values |
| `features/root_search/` | Editable query wiring and search presentation, also reused by command search |
| `extension_views/` | Form controls, validation/focus wiring and custom-view drawing/input adapters |
| `ui/input.rs` | Existing editable-control bindings, shared by query and forms |
| `ui/result_row.rs` | Existing row chrome and accessibility presentation, used for root and command rows |
| `links.rs` | Native link opening adapter |

Shared UI imports no core types and invokes no launcher actions. The app
passes display values into rows and attaches its existing click dispatch.
Core remains GPUI-free. Existing row/control IDs, selectors, key contexts,
public entry points, focus behavior and extension-authored colors are kept.
Only the fields/method used by sibling adapters have crate visibility;
scroll bookkeeping and window dispatch remain private.

No feature placeholders, new toolkit, separate UI crate, semantic palette,
material policy or visual redesign are introduced. Those have real consumers
in later slices; the present shared layer contains only already-used code.

[Retained prototype evidence](evidence/ui-prototype/README.md) keeps the
initial proof and subsequent glass follow-up independently recoverable,
including the authored reference and native captures.

## Prefactor validation (2026-10-02, Windows)

- `cargo fmt -p pane --check`: pass.
- `cargo check -p pane --tests --locked -j 1`: pass, no warnings.
- `cargo test -p pane --test window --test command_search --locked -j 1`:
  **49 window tests + 1 command-search test pass**; real Rust/JS/TS guest
  fixtures, simulated input/IME/accessibility, forms, custom views, query
  activation, scrolling and resize behavior. [Raw result](evidence/ui-prototype/prefactor-tests.log).
- Prototype bundle verified; all 189 overlay files match the saved manifest.
  The authored HTML's Git blob matches the documented SHA-256; a local
  `.gitattributes` rule prevents checkout newline conversion.

The first broad parallel test build exhausted host memory. Validation was
scoped to the required suites and serial compilation. A running retained
prototype locked the shared output executable; the parent renamed that
executable without stopping its windows, permitting the build to complete.
No compiler-cache cleanup or prototype source modification was needed.

Native startup/query/command smoke is handed to the parent for coordinated
desktop access using this commit's built executable. These automated checks
do not claim that native smoke, native IME/assistive technology, or other
platforms have been verified.
