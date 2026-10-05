# Visual workbench comparison: selected-fill

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `root-rest`: 96 DPI, client [760, 518], opaque, perturbation selected-fill
- `root-selected`: 96 DPI, client [760, 518], opaque, perturbation selected-fill

| Section | Passed | Failed |
|---|---|---|
| harness-reference | 48 | 0 |
| harness-native | 76 | 4 |
| parity | 112 | 56 |

## Sensitivity: passed in the baseline, failing now

| Check | Baseline | Now | Expected | Delta | Limit |
|---|---|---|---|---|---|
| `harness-native/root-rest/rest/row:Figma/wash alpha (selected)` | 21.65 | 63.59 | 22 | 41.59 | 2.0 levels |
| `parity/root-rest/rest/row:Figma/wash alpha` | 21.65 | 63.59 | 21.98 | 41.62 | 2.0 levels |
| `harness-native/root-selected/rest/row:Figma/wash alpha (selected)` | 21.65 | 63.59 | 22 | 41.59 | 2.0 levels |
| `parity/root-selected/rest/row:Figma/wash alpha` | 21.65 | 63.59 | 21.98 | 41.62 | 2.0 levels |
| `harness-native/root-selected/down-1/row:Clipboard History/wash alpha (selected)` | 22.26 | 63.93 | 22 | 41.93 | 2.0 levels |
| `parity/root-selected/down-1/row:Clipboard History/wash alpha` | 22.26 | 63.93 | 21.96 | 41.98 | 2.0 levels |
| `harness-native/root-selected/selected/row:Left Half/wash alpha (selected)` | 22.05 | 63.57 | 22 | 41.57 | 2.0 levels |
| `parity/root-selected/selected/row:Left Half/wash alpha` | 22.05 | 63.57 | 22.0 | 41.57 | 2.0 levels |

## harness-native: 4 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-rest/rest/row:Figma/wash alpha (selected)` | 63.59 | 22 | 41.59 | 2.0 levels |  |
| `root-selected/rest/row:Figma/wash alpha (selected)` | 63.59 | 22 | 41.59 | 2.0 levels |  |
| `root-selected/down-1/row:Clipboard History/wash alpha (selected)` | 63.93 | 22 | 41.93 | 2.0 levels |  |
| `root-selected/selected/row:Left Half/wash alpha (selected)` | 63.57 | 22 | 41.57 | 2.0 levels |  |

## harness-reference: 0 failed


## parity: 56 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-rest/rest/row:Figma/wash alpha` | 63.59 | 21.98 | 41.62 | 2.0 levels |  |
| `root-rest/rest/row:Figma/wash width` | 738.0 | 740.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/top in client` | 69.0 | 242 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Clipboard History/top in client` | 115.0 | 288 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/top in client` | 161.0 | 334 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/top in client` | 207.0 | 380 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-rest/rest/search-header/query/placeholder ink top` | 26.0 | 24.0 | 2.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-rest/rest/search-header/query/placeholder ink height` | 13.0 | 17.0 | -4.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-rest/rest/footer/tint alpha` | 35.75 | 37.8 | -2.04 | 2.0 levels |  |
| `root-rest/rest/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-rest/rest/footer-primary/button fill alpha` | 63.75 | -0.37 | 64.12 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-selected/rest/row:Figma/wash alpha` | 63.59 | 21.98 | 41.62 | 2.0 levels |  |
| `root-selected/rest/row:Figma/wash width` | 738.0 | 740.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/top in client` | 69.0 | 242 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Clipboard History/top in client` | 115.0 | 288 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/top in client` | 161.0 | 334 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/top in client` | 207.0 | 380 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-selected/rest/search-header/query/placeholder ink top` | 26.0 | 24.0 | 2.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-selected/rest/search-header/query/placeholder ink height` | 13.0 | 17.0 | -4.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-selected/rest/footer/tint alpha` | 35.75 | 37.8 | -2.04 | 2.0 levels |  |
| `root-selected/rest/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-selected/rest/footer-primary/button fill alpha` | 63.75 | -0.37 | 64.12 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-selected/down-1/row:Figma/top in client` | 69.0 | 242 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Clipboard History/wash alpha` | 63.93 | 21.96 | 41.98 | 2.0 levels |  |
| `root-selected/down-1/row:Clipboard History/wash width` | 738.0 | 740.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/top in client` | 115.0 | 288 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/top in client` | 161.0 | 334 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/top in client` | 207.0 | 380 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-selected/down-1/search-header/query/placeholder ink top` | 26.0 | 24.0 | 2.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-selected/down-1/search-header/query/placeholder ink height` | 13.0 | 17.0 | -4.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-selected/down-1/footer/tint alpha` | 35.75 | 37.8 | -2.04 | 2.0 levels |  |
| `root-selected/down-1/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-selected/down-1/footer-primary/button fill alpha` | 63.75 | -0.37 | 64.12 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-selected/selected/row:Figma/top in client` | 69.0 | 242 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Clipboard History/top in client` | 115.0 | 288 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/wash alpha` | 63.57 | 22.0 | 41.57 | 2.0 levels |  |
| `root-selected/selected/row:Left Half/wash width` | 738.0 | 740.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/top in client` | 161.0 | 334 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/top in client` | 207.0 | 380 | -173.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-selected/selected/search-header/query/placeholder ink top` | 26.0 | 24.0 | 2.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-selected/selected/search-header/query/placeholder ink height` | 13.0 | 17.0 | -4.0 | 1.0 px | placeholder copy differs: 'Search commands' vs 'Search apps, commands, plugins…' |
| `root-selected/selected/footer/tint alpha` | 35.75 | 37.8 | -2.04 | 2.0 levels |  |
| `root-selected/selected/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-selected/selected/footer-primary/button fill alpha` | 63.75 | -0.37 | 64.12 | 2.0 levels | the reference's footer buttons are transparent at rest |

## Pending scenarios (reference saved; native fixture not registered yet)

- `launcher-frame` (root board, 760x518): https://github.com/hoangvu12/pane/issues/92
- `actions-panel` (actions board, 760x518): https://github.com/hoangvu12/pane/issues/95
- `calculator-card` (calculator board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `empty-state` (empty board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `settings-shell` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/97
- `appearance-page` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/98
- `pinned-strip` (root board, 760x518): https://github.com/hoangvu12/pane/issues/101
- `clipboard-split` (clipboard board, 940x600): https://github.com/hoangvu12/pane/issues/102
