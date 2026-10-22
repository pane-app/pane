# Visual workbench comparison: selected-fill

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `root-rest`: 96 DPI, client [760, 518], opaque, perturbation selected-fill
- `root-selected`: 96 DPI, client [760, 518], opaque, perturbation selected-fill

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 48 | 0 | 0 |
| harness-native | 778 | 4 | 0 |
| parity | 1588 | 4 | 16 |

## Sensitivity: passed in the baseline, failing now

| Check | Baseline | Now | Expected | Delta | Limit |
|---|---|---|---|---|---|
| `harness-native/root-rest/rest/row:Figma/wash alpha (selected)` | 22.05 | 63.57 | 22 | 41.57 | 2.0 levels |
| `parity/root-rest/rest/row:Figma/wash alpha` | 22.05 | 63.57 | 21.98 | 41.59 | 2.0 levels |
| `harness-native/root-selected/rest/row:Figma/wash alpha (selected)` | 22.05 | 63.57 | 22 | 41.57 | 2.0 levels |
| `parity/root-selected/rest/row:Figma/wash alpha` | 22.05 | 63.57 | 21.98 | 41.59 | 2.0 levels |
| `harness-native/root-selected/down-1/row:Clipboard History/wash alpha (selected)` | 22.05 | 63.57 | 22 | 41.57 | 2.0 levels |
| `parity/root-selected/down-1/row:Clipboard History/wash alpha` | 22.05 | 63.57 | 21.96 | 41.61 | 2.0 levels |
| `harness-native/root-selected/selected/row:Left Half/wash alpha (selected)` | 22.05 | 63.57 | 22 | 41.57 | 2.0 levels |
| `parity/root-selected/selected/row:Left Half/wash alpha` | 22.05 | 63.57 | 22.0 | 41.57 | 2.0 levels |

## harness-native: 4 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-rest/rest/row:Figma/wash alpha (selected)` | 63.57 | 22 | 41.57 | 2.0 levels |  |
| `root-selected/rest/row:Figma/wash alpha (selected)` | 63.57 | 22 | 41.57 | 2.0 levels |  |
| `root-selected/down-1/row:Clipboard History/wash alpha (selected)` | 63.57 | 22 | 41.57 | 2.0 levels |  |
| `root-selected/selected/row:Left Half/wash alpha (selected)` | 63.57 | 22 | 41.57 | 2.0 levels |  |

## harness-reference: 0 failed


## parity: 4 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-rest/rest/row:Figma/wash alpha` | 63.57 | 21.98 | 41.59 | 2.0 levels |  |
| `root-selected/rest/row:Figma/wash alpha` | 63.57 | 21.98 | 41.59 | 2.0 levels |  |
| `root-selected/down-1/row:Clipboard History/wash alpha` | 63.57 | 21.96 | 41.61 | 2.0 levels |  |
| `root-selected/selected/row:Left Half/wash alpha` | 63.57 | 22.0 | 41.57 | 2.0 levels |  |

## Accepted discrepancies: 16 (failing, with a recorded disposition)

| Check | Native/measured | Reference/expected | Delta | Limit | Disposition |
|---|---|---|---|---|---|
| `root-rest/rest/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-rest/rest/sections/labels` | Pinned | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; both sides show the pinned strip's "Pinned" label (#101) |
| `root-rest/rest/slot:2/tile/glyph core pixels (stroke weight)` | 40.0 | 0 | 40.0 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-rest/rest/slot:3/tile/glyph core pixels (stroke weight)` | 25.35 | 0 | 25.35 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/rest/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/rest/sections/labels` | Pinned | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; both sides show the pinned strip's "Pinned" label (#101) |
| `root-selected/rest/slot:2/tile/glyph core pixels (stroke weight)` | 40.0 | 0 | 40.0 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/rest/slot:3/tile/glyph core pixels (stroke weight)` | 25.35 | 0 | 25.35 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/down-1/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/down-1/sections/labels` | Pinned | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; both sides show the pinned strip's "Pinned" label (#101) |
| `root-selected/down-1/slot:2/tile/glyph core pixels (stroke weight)` | 40.0 | 0 | 40.0 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/down-1/slot:3/tile/glyph core pixels (stroke weight)` | 25.35 | 0 | 25.35 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/selected/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/selected/sections/labels` | Pinned | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; both sides show the pinned strip's "Pinned" label (#101) |
| `root-selected/selected/slot:2/tile/glyph core pixels (stroke weight)` | 40.0 | 0 | 40.0 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/selected/slot:3/tile/glyph core pixels (stroke weight)` | 25.35 | 0 | 25.35 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |

## Pending scenarios (reference saved; native fixture not registered yet)

- `appearance-page` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/98
