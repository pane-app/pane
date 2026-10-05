# Visual workbench comparison: hover-fill

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `root-hover`: 96 DPI, client [760, 518], opaque, perturbation hover-fill

| Section | Passed | Failed |
|---|---|---|
| harness-reference | 28 | 0 |
| harness-native | 43 | 1 |
| parity | 53 | 31 |

## Sensitivity: passed in the baseline, failing now

| Check | Baseline | Now | Expected | Delta | Limit |
|---|---|---|---|---|---|
| `harness-native/root-hover/hover/row:Clipboard History/wash alpha (hovered)` | 8.98 | 51.03 | 9 | 42.03 | 2.0 levels |

## harness-native: 1 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-hover/hover/row:Clipboard History/wash alpha (hovered)` | 51.03 | 9 | 42.03 | 2.0 levels |  |

## harness-reference: 0 failed


## parity: 31 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-hover/rest/row:Figma/wash width` | 738.0 | 740.0 | -2.0 | 1.0 px |  |
| `root-hover/rest/row:Figma/top in client` | 69.0 | 242 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/rest/row:Clipboard History/top in client` | 115.0 | 288 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/rest/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-hover/rest/row:Left Half/top in client` | 161.0 | 334 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/rest/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-hover/rest/row:Search Files/top in client` | 207.0 | 380 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/rest/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-hover/rest/search-header/query/placeholder ink top` | 26.0 | 24.0 | 2.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-hover/rest/search-header/query/placeholder ink height` | 13.0 | 17.0 | -4.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-hover/rest/footer/tint alpha` | 35.75 | 37.8 | -2.04 | 2.0 levels |  |
| `root-hover/rest/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-hover/rest/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-hover/hover/selection/selected rows` | Figma | Clipboard History | None | 0  | the reference selects the row the pointer moves over; production only washes it |
| `root-hover/hover/selection/hover-only rows` | Clipboard History |  | None | 0  |  |
| `root-hover/hover/row:Figma/wash alpha` | 21.65 | 0.0 | 21.65 | 2.0 levels |  |
| `root-hover/hover/row:Figma/top in client` | 69.0 | 242 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/hover/row:Clipboard History/wash alpha` | 51.03 | 21.96 | 29.07 | 2.0 levels |  |
| `root-hover/hover/row:Clipboard History/wash width` | 738.0 | 740.0 | -2.0 | 1.0 px |  |
| `root-hover/hover/row:Clipboard History/top in client` | 115.0 | 288 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/hover/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-hover/hover/row:Left Half/top in client` | 161.0 | 334 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/hover/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-hover/hover/row:Search Files/top in client` | 207.0 | 380 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/hover/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-hover/hover/search-header/query/placeholder ink top` | 26.0 | 24.0 | 2.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-hover/hover/search-header/query/placeholder ink height` | 13.0 | 17.0 | -4.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-hover/hover/footer/tint alpha` | 35.75 | 37.8 | -2.04 | 2.0 levels |  |
| `root-hover/hover/footer-primary/label` | Open Application | Run Command | None | 0  |  |
| `root-hover/hover/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-hover/hover/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |

## Pending scenarios (reference saved; native fixture not registered yet)

- `launcher-frame` (root board, 760x518): https://github.com/hoangvu12/pane/issues/92
- `actions-panel` (actions board, 760x518): https://github.com/hoangvu12/pane/issues/95
- `calculator-card` (calculator board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `empty-state` (empty board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `settings-shell` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/97
- `appearance-page` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/98
- `pinned-strip` (root board, 760x518): https://github.com/hoangvu12/pane/issues/101
- `clipboard-split` (clipboard board, 940x600): https://github.com/hoangvu12/pane/issues/102
