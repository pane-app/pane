# Windows transparency patch

This is `crates/gpui_windows` from GPUI CE revision
`17d9c8e8fdb30a329d817ca06bff424e8e848f1a`, under its Apache-2.0 license.
The prototype patches only this package. Other GPUI packages remain pinned
to that original revision; no Cargo cache files are modified.

Two source edits in `src/directx_renderer.rs`, in `create_blend_state` and
`create_blend_state_for_path_sprite`, change destination alpha blending from
`D3D11_BLEND_ONE` to `D3D11_BLEND_INV_SRC_ALPHA`. This matches Roboco's
`hoangvu12/zui` revision `ec16c62b83caa58b61cd8039db4b5721bacf948f`:
layer opacity follows source-over instead of adding until it saturates.

`Cargo.toml` expands inherited workspace settings, replaces local GPUI paths
with the original pinned git dependencies, and declares an isolated workspace.
The upstream workspace lint inheritance is omitted. No other source files change.

This correction alone did not restore useful glass in Pane: its full-panel
outer shadows still obscured the backdrop. Pane also removes those shadows,
retaining its inner highlight and border. Native captures exercise both changes.

The local patch is prototype scaffolding; production should use an audited
upstream/fork revision containing the correction rather than retain a copied crate.
