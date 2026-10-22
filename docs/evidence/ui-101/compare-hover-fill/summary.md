# Visual workbench comparison: hover-fill

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `root-pointer-keys`: 96 DPI, client [760, 518], opaque, perturbation hover-fill

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 38 | 0 | 0 |
| harness-native | 586 | 1 | 0 |
| parity | 1193 | 1 | 12 |

## Sensitivity: passed in the baseline, failing now

| Check | Baseline | Now | Expected | Delta | Limit |
|---|---|---|---|---|---|
| `harness-native/root-pointer-keys/down-under-pointer/row:Clipboard History/wash alpha (hovered)` | 8.82 | 51.07 | 9 | 42.07 | 2.0 levels |
| `parity/root-pointer-keys/down-under-pointer/row:Clipboard History/wash alpha` | 8.82 | 51.07 | 8.4 | 42.67 | 2.0 levels |

## harness-native: 1 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-pointer-keys/down-under-pointer/row:Clipboard History/wash alpha (hovered)` | 51.07 | 9 | 42.07 | 2.0 levels |  |

## harness-reference: 0 failed


## parity: 1 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-pointer-keys/down-under-pointer/row:Clipboard History/wash alpha` | 51.07 | 8.4 | 42.67 | 2.0 levels |  |

## Accepted discrepancies: 12 (failing, with a recorded disposition)

| Check | Native/measured | Reference/expected | Delta | Limit | Disposition |
|---|---|---|---|---|---|
| `root-pointer-keys/pointed/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-pointer-keys/pointed/sections/labels` | Pinned | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; both sides show the pinned strip's "Pinned" label (#101) |
| `root-pointer-keys/pointed/slot:2/tile/glyph core pixels (stroke weight)` | 40.0 | 0 | 40.0 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-pointer-keys/pointed/slot:3/tile/glyph core pixels (stroke weight)` | 25.35 | 0 | 25.35 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-pointer-keys/down-under-pointer/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-pointer-keys/down-under-pointer/sections/labels` | Pinned | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; both sides show the pinned strip's "Pinned" label (#101) |
| `root-pointer-keys/down-under-pointer/slot:2/tile/glyph core pixels (stroke weight)` | 40.0 | 0 | 40.0 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-pointer-keys/down-under-pointer/slot:3/tile/glyph core pixels (stroke weight)` | 25.35 | 0 | 25.35 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-pointer-keys/moved-again/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-pointer-keys/moved-again/sections/labels` | Pinned | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; both sides show the pinned strip's "Pinned" label (#101) |
| `root-pointer-keys/moved-again/slot:2/tile/glyph core pixels (stroke weight)` | 40.0 | 0 | 40.0 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-pointer-keys/moved-again/slot:3/tile/glyph core pixels (stroke weight)` | 25.35 | 0 | 25.35 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |

## Pending scenarios (reference saved; native fixture not registered yet)

- `appearance-page` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/98
