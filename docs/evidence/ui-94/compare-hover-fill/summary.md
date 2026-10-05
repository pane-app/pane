# Visual workbench comparison: hover-fill

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `root-pointer-keys`: 96 DPI, client [760, 518], opaque, perturbation hover-fill

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 38 | 0 | 0 |
| harness-native | 255 | 1 | 0 |
| parity | 479 | 19 | 6 |

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


## parity: 19 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-pointer-keys/pointed/row:Figma/top in client` | 100.0 | 242 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/pointed/row:Clipboard History/top in client` | 146.0 | 288 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/pointed/row:Left Half/top in client` | 192.0 | 334 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/pointed/row:Search Files/top in client` | 238.0 | 380 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/pointed/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-pointer-keys/pointed/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-pointer-keys/down-under-pointer/row:Figma/top in client` | 100.0 | 242 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/down-under-pointer/row:Clipboard History/wash alpha` | 51.07 | 8.4 | 42.67 | 2.0 levels |  |
| `root-pointer-keys/down-under-pointer/row:Clipboard History/top in client` | 146.0 | 288 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/down-under-pointer/row:Left Half/top in client` | 192.0 | 334 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/down-under-pointer/row:Search Files/top in client` | 238.0 | 380 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/down-under-pointer/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-pointer-keys/down-under-pointer/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-pointer-keys/moved-again/row:Figma/top in client` | 100.0 | 242 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/moved-again/row:Clipboard History/top in client` | 146.0 | 288 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/moved-again/row:Left Half/top in client` | 192.0 | 334 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/moved-again/row:Search Files/top in client` | 238.0 | 380 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-pointer-keys/moved-again/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-pointer-keys/moved-again/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |

## Accepted discrepancies: 6 (failing, with a recorded disposition)

| Check | Native/measured | Reference/expected | Delta | Limit | Disposition |
|---|---|---|---|---|---|
| `root-pointer-keys/pointed/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-pointer-keys/pointed/sections/labels` | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; the reference's pinned strip and its label are #101's |
| `root-pointer-keys/down-under-pointer/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-pointer-keys/down-under-pointer/sections/labels` | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; the reference's pinned strip and its label are #101's |
| `root-pointer-keys/moved-again/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-pointer-keys/moved-again/sections/labels` | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; the reference's pinned strip and its label are #101's |

## Pending scenarios (reference saved; native fixture not registered yet)

- `actions-panel` (actions board, 760x518): https://github.com/hoangvu12/pane/issues/95
- `calculator-card` (calculator board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `empty-state` (empty board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `settings-shell` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/97
- `appearance-page` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/98
- `pinned-strip` (root board, 760x518): https://github.com/hoangvu12/pane/issues/101
- `clipboard-split` (clipboard board, 940x600): https://github.com/hoangvu12/pane/issues/102
