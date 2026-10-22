# Visual workbench comparison: selected-fill

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `root-rest`: 96 DPI, client [760, 518], opaque, perturbation selected-fill
- `root-selected`: 96 DPI, client [760, 518], opaque, perturbation selected-fill

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 48 | 0 | 0 |
| harness-native | 400 | 4 | 0 |
| parity | 766 | 22 | 8 |

## Sensitivity: passed in the baseline, failing now

| Check | Baseline | Now | Expected | Delta | Limit |
|---|---|---|---|---|---|
| `harness-native/root-rest/rest/row:Figma/wash alpha (selected)` | 21.72 | 63.56 | 22 | 41.56 | 2.0 levels |
| `parity/root-rest/rest/row:Figma/wash alpha` | 21.72 | 63.56 | 21.98 | 41.59 | 2.0 levels |
| `parity/root-rest/rest/row:Figma/tile/height` | 28.0 | 22.0 | 28.0 | -6.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Figma/wash alpha (selected)` | 21.72 | 63.56 | 22 | 41.56 | 2.0 levels |
| `parity/root-selected/rest/row:Figma/wash alpha` | 21.72 | 63.56 | 21.98 | 41.59 | 2.0 levels |
| `parity/root-selected/rest/row:Figma/tile/height` | 28.0 | 22.0 | 28.0 | -6.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Clipboard History/wash alpha (selected)` | 22.05 | 63.57 | 22 | 41.57 | 2.0 levels |
| `parity/root-selected/down-1/row:Clipboard History/wash alpha` | 22.05 | 63.57 | 21.96 | 41.61 | 2.0 levels |
| `harness-native/root-selected/selected/row:Left Half/wash alpha (selected)` | 22.05 | 63.57 | 22 | 41.57 | 2.0 levels |
| `parity/root-selected/selected/row:Left Half/wash alpha` | 22.05 | 63.57 | 22.0 | 41.57 | 2.0 levels |

## harness-native: 4 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-rest/rest/row:Figma/wash alpha (selected)` | 63.56 | 22 | 41.56 | 2.0 levels |  |
| `root-selected/rest/row:Figma/wash alpha (selected)` | 63.56 | 22 | 41.56 | 2.0 levels |  |
| `root-selected/down-1/row:Clipboard History/wash alpha (selected)` | 63.57 | 22 | 41.57 | 2.0 levels |  |
| `root-selected/selected/row:Left Half/wash alpha (selected)` | 63.57 | 22 | 41.57 | 2.0 levels |  |

## harness-reference: 0 failed


## parity: 22 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-rest/rest/row:Figma/wash alpha` | 63.56 | 21.98 | 41.59 | 2.0 levels |  |
| `root-rest/rest/row:Figma/top in client` | 100.0 | 242 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Figma/tile/height` | 22.0 | 28.0 | -6.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/top in client` | 146.0 | 288 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Left Half/top in client` | 192.0 | 334 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Search Files/top in client` | 238.0 | 380 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Figma/wash alpha` | 63.56 | 21.98 | 41.59 | 2.0 levels |  |
| `root-selected/rest/row:Figma/top in client` | 100.0 | 242 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Figma/tile/height` | 22.0 | 28.0 | -6.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/top in client` | 146.0 | 288 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Left Half/top in client` | 192.0 | 334 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Search Files/top in client` | 238.0 | 380 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Figma/top in client` | 100.0 | 242 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Clipboard History/wash alpha` | 63.57 | 21.96 | 41.61 | 2.0 levels |  |
| `root-selected/down-1/row:Clipboard History/top in client` | 146.0 | 288 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Left Half/top in client` | 192.0 | 334 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Search Files/top in client` | 238.0 | 380 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Figma/top in client` | 100.0 | 242 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Clipboard History/top in client` | 146.0 | 288 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Left Half/wash alpha` | 63.57 | 22.0 | 41.57 | 2.0 levels |  |
| `root-selected/selected/row:Left Half/top in client` | 192.0 | 334 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Search Files/top in client` | 238.0 | 380 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |

## Accepted discrepancies: 8 (failing, with a recorded disposition)

| Check | Native/measured | Reference/expected | Delta | Limit | Disposition |
|---|---|---|---|---|---|
| `root-rest/rest/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-rest/rest/sections/labels` | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; the reference's pinned strip and its label are #101's |
| `root-selected/rest/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/rest/sections/labels` | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; the reference's pinned strip and its label are #101's |
| `root-selected/down-1/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/down-1/sections/labels` | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; the reference's pinned strip and its label are #101's |
| `root-selected/selected/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/selected/sections/labels` | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; the reference's pinned strip and its label are #101's |

## Pending scenarios (reference saved; native fixture not registered yet)

- `calculator-card` (calculator board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `empty-state` (empty board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `settings-shell` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/97
- `appearance-page` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/98
- `pinned-strip` (root board, 760x518): https://github.com/hoangvu12/pane/issues/101
- `clipboard-split` (clipboard board, 940x600): https://github.com/hoangvu12/pane/issues/102
