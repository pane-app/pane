# Visual workbench comparison: well-fill

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `settings-keyboard`: 96 DPI, client [1120, 720], opaque, perturbation well-fill

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 37 | 0 | 0 |
| harness-native | 108 | 25 | 0 |

## Sensitivity: passed in the baseline, failing now

| Check | Baseline | Now | Expected | Delta | Limit |
|---|---|---|---|---|---|
| `harness-native/settings-keyboard/keyboard/search/fill alpha` | 62.11 | 130.18 | 61 | 69.18 | 2.0 levels |
| `harness-native/settings-keyboard/keyboard/well:previous-result/well width` | 96.0 | 94.0 | 96.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:previous-result/well height` | 30.0 | 28.0 | 30.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:previous-result/well fill alpha` | 61.11 | 129.35 | 61.0 | 68.35 | 2.0 levels |
| `harness-native/settings-keyboard/keyboard/well:next-result/well width` | 96.0 | 94.0 | 96.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:next-result/well height` | 30.0 | 28.0 | 30.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:next-result/well fill alpha` | 61.11 | 129.35 | 61.0 | 68.35 | 2.0 levels |
| `harness-native/settings-keyboard/keyboard/well:invoke-selected-action/well width` | 96.0 | 94.0 | 96.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:invoke-selected-action/well height` | 30.0 | 28.0 | 30.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:invoke-selected-action/well fill alpha` | 61.11 | 129.35 | 61.0 | 68.35 | 2.0 levels |
| `harness-native/settings-keyboard/keyboard/well:open-actions/well width` | 96.0 | 94.0 | 96.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:open-actions/well height` | 30.0 | 28.0 | 30.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:open-actions/well fill alpha` | 61.11 | 129.35 | 61.0 | 68.35 | 2.0 levels |
| `harness-native/settings-keyboard/keyboard/well:back/well width` | 96.0 | 94.0 | 96.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:back/well height` | 30.0 | 28.0 | 30.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:back/well fill alpha` | 61.11 | 129.35 | 61.0 | 68.35 | 2.0 levels |
| `harness-native/settings-keyboard/keyboard/well:return-to-root/well width` | 96.0 | 94.0 | 96.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:return-to-root/well height` | 30.0 | 28.0 | 30.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:return-to-root/well fill alpha` | 61.11 | 129.35 | 61.0 | 68.35 | 2.0 levels |
| `harness-native/settings-keyboard/keyboard/well:dismiss-launcher/well width` | 96.0 | 94.0 | 96.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:dismiss-launcher/well height` | 30.0 | 28.0 | 30.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:dismiss-launcher/well fill alpha` | 61.11 | 129.35 | 61.0 | 68.35 | 2.0 levels |
| `harness-native/settings-keyboard/keyboard/well:open-settings/well width` | 96.0 | 94.0 | 96.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:open-settings/well height` | 30.0 | 28.0 | 30.0 | -2.0 | 1.0 px |
| `harness-native/settings-keyboard/keyboard/well:open-settings/well fill alpha` | 61.11 | 129.35 | 61.0 | 68.35 | 2.0 levels |

## harness-native: 25 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `settings-keyboard/keyboard/search/fill alpha` | 130.18 | 61 | 69.18 | 2.0 levels | a black overlay over the sidebar below it |
| `settings-keyboard/keyboard/well:previous-result/well width` | 94.0 | 96.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:previous-result/well height` | 28.0 | 30.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:previous-result/well fill alpha` | 129.35 | 61.0 | 68.35 | 2.0 levels | a black overlay over the page beside it |
| `settings-keyboard/keyboard/well:next-result/well width` | 94.0 | 96.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:next-result/well height` | 28.0 | 30.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:next-result/well fill alpha` | 129.35 | 61.0 | 68.35 | 2.0 levels | a black overlay over the page beside it |
| `settings-keyboard/keyboard/well:invoke-selected-action/well width` | 94.0 | 96.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:invoke-selected-action/well height` | 28.0 | 30.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:invoke-selected-action/well fill alpha` | 129.35 | 61.0 | 68.35 | 2.0 levels | a black overlay over the page beside it |
| `settings-keyboard/keyboard/well:open-actions/well width` | 94.0 | 96.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:open-actions/well height` | 28.0 | 30.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:open-actions/well fill alpha` | 129.35 | 61.0 | 68.35 | 2.0 levels | a black overlay over the page beside it |
| `settings-keyboard/keyboard/well:back/well width` | 94.0 | 96.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:back/well height` | 28.0 | 30.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:back/well fill alpha` | 129.35 | 61.0 | 68.35 | 2.0 levels | a black overlay over the page beside it |
| `settings-keyboard/keyboard/well:return-to-root/well width` | 94.0 | 96.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:return-to-root/well height` | 28.0 | 30.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:return-to-root/well fill alpha` | 129.35 | 61.0 | 68.35 | 2.0 levels | a black overlay over the page beside it |
| `settings-keyboard/keyboard/well:dismiss-launcher/well width` | 94.0 | 96.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:dismiss-launcher/well height` | 28.0 | 30.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:dismiss-launcher/well fill alpha` | 129.35 | 61.0 | 68.35 | 2.0 levels | a black overlay over the page beside it |
| `settings-keyboard/keyboard/well:open-settings/well width` | 94.0 | 96.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:open-settings/well height` | 28.0 | 30.0 | -2.0 | 1.0 px |  |
| `settings-keyboard/keyboard/well:open-settings/well fill alpha` | 129.35 | 61.0 | 68.35 | 2.0 levels | a black overlay over the page beside it |

## harness-reference: 0 failed


## parity: 0 failed


## Native-only captures (no reference counterpart; not compared)

- `settings-keyboard`: settings-keyboard/keyboard.png
