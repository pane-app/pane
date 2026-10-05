# Visual workbench comparison: row-padding-plus-4

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `root-rest`: 96 DPI, client [760, 518], opaque, perturbation row-padding-plus-4
- `root-selected`: 96 DPI, client [760, 518], opaque, perturbation row-padding-plus-4

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 48 | 0 | 0 |
| harness-native | 72 | 8 | 0 |
| parity | 108 | 60 | 0 |

## Sensitivity: passed in the baseline, failing now

| Check | Baseline | Now | Expected | Delta | Limit |
|---|---|---|---|---|---|
| `harness-native/root-rest/rest/row:Figma/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-rest/rest/row:Figma/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 50.0 | 5.0 | 1.0 px |
| `parity/root-rest/rest/row:Left Half/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Search Files/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Figma/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Figma/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 50.0 | 5.0 | 1.0 px |
| `parity/root-selected/rest/row:Left Half/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Search Files/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Clipboard History/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Clipboard History/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Left Half/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Search Files/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Left Half/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Left Half/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-selected/selected/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 50.0 | 5.0 | 1.0 px |
| `parity/root-selected/selected/row:Left Half/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Left Half/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-selected/selected/row:Left Half/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Search Files/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |

## harness-native: 8 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-rest/rest/row:Figma/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |

## harness-reference: 0 failed


## parity: 60 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-rest/rest/row:Figma/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Clipboard History/title ink left in row` | 55.0 | 50.0 | 5.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-rest/rest/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-rest/rest/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-selected/rest/row:Figma/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Clipboard History/title ink left in row` | 55.0 | 50.0 | 5.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-selected/rest/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-selected/rest/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-selected/down-1/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Clipboard History/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-selected/down-1/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-selected/down-1/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-selected/selected/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Clipboard History/title ink left in row` | 55.0 | 50.0 | 5.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-selected/selected/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-selected/selected/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |

## Pending scenarios (reference saved; native fixture not registered yet)

- `actions-panel` (actions board, 760x518): https://github.com/hoangvu12/pane/issues/95
- `calculator-card` (calculator board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `empty-state` (empty board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `settings-shell` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/97
- `appearance-page` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/98
- `pinned-strip` (root board, 760x518): https://github.com/hoangvu12/pane/issues/101
- `clipboard-split` (clipboard board, 940x600): https://github.com/hoangvu12/pane/issues/102
