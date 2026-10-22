# Visual workbench comparison: row-padding-plus-4

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `root-rest`: 96 DPI, client [760, 518], opaque, perturbation row-padding-plus-4
- `root-selected`: 96 DPI, client [760, 518], opaque, perturbation row-padding-plus-4

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 48 | 0 | 0 |
| harness-native | 264 | 140 | 0 |
| parity | 587 | 197 | 12 |

## Sensitivity: passed in the baseline, failing now

| Check | Baseline | Now | Expected | Delta | Limit |
|---|---|---|---|---|---|
| `harness-native/root-rest/rest/row:Figma/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-rest/rest/row:Figma/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `harness-native/root-rest/rest/row:Figma/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-rest/rest/row:Clipboard History/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-rest/rest/row:Clipboard History/alias chip width` | 26.0 | 12.0 | 25.2 | -13.2 | 1.0 px |
| `harness-native/root-rest/rest/row:Clipboard History/alias chip gap to what follows` | 12.0 | 21.0 | 12.0 | 9.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:ctrl-shift-v/cap 0 left` | 534.0 | 532.0 | 534.6 | -2.6 | 1.0 px |
| `harness-native/root-rest/rest/keys:ctrl-shift-v/cap 0 width` | 37.0 | 41.0 | 36.4 | 4.6 | 1.0 px |
| `harness-native/root-rest/rest/keys:ctrl-shift-v/cap 1 left` | 574.0 | 572.0 | 574.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:ctrl-shift-v/cap 1 width` | 43.0 | 47.0 | 43.0 | 4.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:ctrl-shift-v/cap 1 fill alpha` | 17.64 | 21.35 | 18 | 3.35 | 2.0 levels |
| `harness-native/root-rest/rest/keys:ctrl-shift-v/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/row:Left Half/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 0 left` | 554.0 | 555.5 | 554.4 | 1.1 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 0 top` | 204.0 | 211.0 | 204.0 | 7.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 29.8 | -11.3 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 0 fill alpha` | 17.64 | 109.64 | 18 | 91.64 | 2.0 levels |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 1 left` | 587.0 | 585.0 | 587.2 | -2.2 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 29.8 | 4.2 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/row:Search Files/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-rest/rest/row:Search Files/alias chip width` | 19.0 | 11.0 | 18.6 | -7.6 | 1.0 px |
| `harness-native/root-rest/rest/row:Search Files/alias chip gap to what follows` | 12.0 | 16.0 | 12.0 | 4.0 | 1.0 px |
| `harness-native/root-rest/rest/row:Plugin Store/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-rest/rest/row:Plugin Store/alias chip width` | 45.0 | 31.0 | 45.0 | -14.0 | 1.0 px |
| `harness-native/root-rest/rest/row:Plugin Store/alias chip gap to what follows` | 12.0 | 23.0 | 12.0 | 11.0 | 1.0 px |
| `harness-native/root-rest/rest/row:Toggle Dark Mode/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-rest/rest/row:Lock Screen/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-rest/rest/row:Settings/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-rest/rest/keys:ctrl-,/cap 0 left` | 580.0 | 578.0 | 580.6 | -2.6 | 1.0 px |
| `harness-native/root-rest/rest/keys:ctrl-,/cap 0 width` | 37.0 | 41.0 | 36.4 | 4.6 | 1.0 px |
| `harness-native/root-rest/rest/keys:ctrl-,/cap 1 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:ctrl-,/cap 1 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/tile/gradient top (max channel)` | 1.0 | 191.5 | 0 | 191.5 | 4.0 levels |
| `parity/root-rest/rest/row:Figma/tile/gradient bottom (max channel)` | 2.5 | 127.5 | 0 | 127.5 | 4.0 levels |
| `parity/root-rest/rest/row:Figma/tile/glyph ink left` | 8.0 | 12.0 | 9.0 | 3.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 50.0 | 5.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/tile/glyph ink left` | 9.0 | 13.0 | 9.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/alias chip left in row` | 486.0 | 489.0 | 487.0 | 2.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/alias chip top in row` | 13.0 | 20.0 | 13.0 | 7.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/alias chip width` | 26.0 | 12.0 | 26.0 | -14.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/alias chip height` | 18.0 | 6.0 | 18.0 | -12.0 | 1.0 px |
| `parity/root-rest/rest/keys:ctrl-shift-v/group width` | 106.0 | 104.0 | 105.41 | -1.41 | 1.0 px |
| `parity/root-rest/rest/keys:ctrl-shift-v/cap 0 width` | 37.0 | 41.0 | 36.0 | 5.0 | 1.0 px |
| `parity/root-rest/rest/keys:ctrl-shift-v/cap 1 width` | 43.0 | 47.0 | 43.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/keys:ctrl-shift-v/cap 1 label ink left` | 6.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/keys:ctrl-shift-v/cap 2 label ink left` | 6.0 | 4.0 | 7.0 | -3.0 | 1.0 px |
| `parity/root-rest/rest/row:Left Half/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Left Half/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Left Half/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/row:Left Half/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Left Half/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-rest/rest/keys:win-alt-left/group width` | 86.0 | 80.5 | 85.62 | -5.12 | 1.0 px |
| `parity/root-rest/rest/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 30.0 | -11.5 | 1.0 px |
| `parity/root-rest/rest/keys:win-alt-left/cap 0 fill alpha` | 17.64 | 109.64 | 19.15 | 90.49 | 2.0 levels |
| `parity/root-rest/rest/keys:win-alt-left/cap 0 label ink left` | 5.0 | 0.5 | 6.0 | -5.5 | 1.0 px |
| `parity/root-rest/rest/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 30.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/keys:win-alt-left/cap 1 label ink left` | 5.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/keys:win-alt-left/cap 2 label ink left` | 6.0 | 4.0 | 7.0 | -3.0 | 1.0 px |
| `parity/root-rest/rest/row:Search Files/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Search Files/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Search Files/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/row:Search Files/tile/glyph ink left` | 9.0 | 13.0 | 9.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Search Files/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-rest/rest/row:Search Files/alias chip left in row` | 611.0 | 615.0 | 611.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Search Files/alias chip width` | 19.0 | 11.0 | 19.0 | -8.0 | 1.0 px |
| `parity/root-rest/rest/section:Commands/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Figma/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Figma/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Figma/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/rest/row:Clipboard History/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/rest/row:Clipboard History/alias chip width` | 26.0 | 12.0 | 25.2 | -13.2 | 1.0 px |
| `harness-native/root-selected/rest/row:Clipboard History/alias chip gap to what follows` | 12.0 | 21.0 | 12.0 | 9.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:ctrl-shift-v/cap 0 left` | 534.0 | 532.0 | 534.6 | -2.6 | 1.0 px |
| `harness-native/root-selected/rest/keys:ctrl-shift-v/cap 0 width` | 37.0 | 41.0 | 36.4 | 4.6 | 1.0 px |
| `harness-native/root-selected/rest/keys:ctrl-shift-v/cap 1 left` | 574.0 | 572.0 | 574.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:ctrl-shift-v/cap 1 width` | 43.0 | 47.0 | 43.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:ctrl-shift-v/cap 1 fill alpha` | 17.64 | 21.35 | 18 | 3.35 | 2.0 levels |
| `harness-native/root-selected/rest/keys:ctrl-shift-v/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Left Half/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 0 left` | 554.0 | 555.5 | 554.4 | 1.1 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 0 top` | 204.0 | 211.0 | 204.0 | 7.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 29.8 | -11.3 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 0 fill alpha` | 17.64 | 109.64 | 18 | 91.64 | 2.0 levels |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 1 left` | 587.0 | 585.0 | 587.2 | -2.2 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 29.8 | 4.2 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Search Files/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/rest/row:Search Files/alias chip width` | 19.0 | 11.0 | 18.6 | -7.6 | 1.0 px |
| `harness-native/root-selected/rest/row:Search Files/alias chip gap to what follows` | 12.0 | 16.0 | 12.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Plugin Store/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/rest/row:Plugin Store/alias chip width` | 45.0 | 31.0 | 45.0 | -14.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Plugin Store/alias chip gap to what follows` | 12.0 | 23.0 | 12.0 | 11.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Toggle Dark Mode/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/rest/row:Lock Screen/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/rest/row:Settings/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/rest/keys:ctrl-,/cap 0 left` | 580.0 | 578.0 | 580.6 | -2.6 | 1.0 px |
| `harness-native/root-selected/rest/keys:ctrl-,/cap 0 width` | 37.0 | 41.0 | 36.4 | 4.6 | 1.0 px |
| `harness-native/root-selected/rest/keys:ctrl-,/cap 1 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:ctrl-,/cap 1 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/tile/gradient top (max channel)` | 1.0 | 191.5 | 0 | 191.5 | 4.0 levels |
| `parity/root-selected/rest/row:Figma/tile/gradient bottom (max channel)` | 2.5 | 127.5 | 0 | 127.5 | 4.0 levels |
| `parity/root-selected/rest/row:Figma/tile/glyph ink left` | 8.0 | 12.0 | 9.0 | 3.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 50.0 | 5.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/tile/glyph ink left` | 9.0 | 13.0 | 9.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/alias chip left in row` | 486.0 | 489.0 | 487.0 | 2.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/alias chip top in row` | 13.0 | 20.0 | 13.0 | 7.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/alias chip width` | 26.0 | 12.0 | 26.0 | -14.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/alias chip height` | 18.0 | 6.0 | 18.0 | -12.0 | 1.0 px |
| `parity/root-selected/rest/keys:ctrl-shift-v/group width` | 106.0 | 104.0 | 105.41 | -1.41 | 1.0 px |
| `parity/root-selected/rest/keys:ctrl-shift-v/cap 0 width` | 37.0 | 41.0 | 36.0 | 5.0 | 1.0 px |
| `parity/root-selected/rest/keys:ctrl-shift-v/cap 1 width` | 43.0 | 47.0 | 43.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/keys:ctrl-shift-v/cap 1 label ink left` | 6.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/keys:ctrl-shift-v/cap 2 label ink left` | 6.0 | 4.0 | 7.0 | -3.0 | 1.0 px |
| `parity/root-selected/rest/row:Left Half/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Left Half/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Left Half/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/row:Left Half/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Left Half/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/rest/keys:win-alt-left/group width` | 86.0 | 80.5 | 85.62 | -5.12 | 1.0 px |
| `parity/root-selected/rest/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 30.0 | -11.5 | 1.0 px |
| `parity/root-selected/rest/keys:win-alt-left/cap 0 fill alpha` | 17.64 | 109.64 | 19.15 | 90.49 | 2.0 levels |
| `parity/root-selected/rest/keys:win-alt-left/cap 0 label ink left` | 5.0 | 0.5 | 6.0 | -5.5 | 1.0 px |
| `parity/root-selected/rest/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 30.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/keys:win-alt-left/cap 1 label ink left` | 5.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/keys:win-alt-left/cap 2 label ink left` | 6.0 | 4.0 | 7.0 | -3.0 | 1.0 px |
| `parity/root-selected/rest/row:Search Files/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Search Files/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Search Files/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/row:Search Files/tile/glyph ink left` | 9.0 | 13.0 | 9.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Search Files/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/rest/row:Search Files/alias chip left in row` | 611.0 | 615.0 | 611.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Search Files/alias chip width` | 19.0 | 11.0 | 19.0 | -8.0 | 1.0 px |
| `parity/root-selected/rest/section:Commands/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Clipboard History/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Clipboard History/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Figma/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/down-1/row:Clipboard History/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/down-1/row:Clipboard History/alias chip width` | 26.0 | 12.0 | 25.2 | -13.2 | 1.0 px |
| `harness-native/root-selected/down-1/row:Clipboard History/alias chip gap to what follows` | 12.0 | 21.0 | 12.0 | 9.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:ctrl-shift-v/cap 0 left` | 534.0 | 532.0 | 534.6 | -2.6 | 1.0 px |
| `harness-native/root-selected/down-1/keys:ctrl-shift-v/cap 0 width` | 37.0 | 41.0 | 36.4 | 4.6 | 1.0 px |
| `harness-native/root-selected/down-1/keys:ctrl-shift-v/cap 1 left` | 574.0 | 572.0 | 574.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:ctrl-shift-v/cap 1 width` | 43.0 | 47.0 | 43.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:ctrl-shift-v/cap 1 fill alpha` | 18.1 | 21.76 | 18 | 3.76 | 2.0 levels |
| `harness-native/root-selected/down-1/keys:ctrl-shift-v/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Left Half/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 0 left` | 554.0 | 555.5 | 554.4 | 1.1 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 0 top` | 204.0 | 211.0 | 204.0 | 7.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 29.8 | -11.3 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 0 fill alpha` | 17.64 | 109.64 | 18 | 91.64 | 2.0 levels |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 1 left` | 587.0 | 585.0 | 587.2 | -2.2 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 29.8 | 4.2 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Search Files/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/down-1/row:Search Files/alias chip width` | 19.0 | 11.0 | 18.6 | -7.6 | 1.0 px |
| `harness-native/root-selected/down-1/row:Search Files/alias chip gap to what follows` | 12.0 | 16.0 | 12.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Plugin Store/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/down-1/row:Plugin Store/alias chip width` | 45.0 | 31.0 | 45.0 | -14.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Plugin Store/alias chip gap to what follows` | 12.0 | 23.0 | 12.0 | 11.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Toggle Dark Mode/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/down-1/row:Lock Screen/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/down-1/row:Settings/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/down-1/keys:ctrl-,/cap 0 left` | 580.0 | 578.0 | 580.6 | -2.6 | 1.0 px |
| `harness-native/root-selected/down-1/keys:ctrl-,/cap 0 width` | 37.0 | 41.0 | 36.4 | 4.6 | 1.0 px |
| `harness-native/root-selected/down-1/keys:ctrl-,/cap 1 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:ctrl-,/cap 1 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/tile/gradient top (max channel)` | 1.0 | 207.5 | 0 | 207.5 | 4.0 levels |
| `parity/root-selected/down-1/row:Figma/tile/gradient bottom (max channel)` | 2.5 | 140.0 | 0 | 140.0 | 4.0 levels |
| `parity/root-selected/down-1/row:Figma/tile/glyph ink left` | 8.0 | 12.0 | 9.0 | 3.0 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/tile/glyph ink height` | 11.0 | 12.0 | 10.0 | 2.0 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/tile/fill alpha` | 20.11 | 34.25 | 20.05 | 14.2 | 2.0 levels |
| `parity/root-selected/down-1/row:Clipboard History/tile/glyph ink left` | 9.0 | 13.0 | 9.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/alias chip left in row` | 486.0 | 489.0 | 487.0 | 2.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/alias chip top in row` | 13.0 | 20.0 | 13.0 | 7.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/alias chip width` | 26.0 | 12.0 | 26.0 | -14.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/alias chip height` | 18.0 | 6.0 | 18.0 | -12.0 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/group width` | 106.0 | 104.0 | 105.41 | -1.41 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/cap 0 width` | 37.0 | 41.0 | 36.0 | 5.0 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/cap 1 width` | 43.0 | 47.0 | 43.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/cap 1 fill alpha` | 18.1 | 21.76 | 19.17 | 2.59 | 2.0 levels |
| `parity/root-selected/down-1/keys:ctrl-shift-v/cap 1 label ink left` | 6.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/cap 2 label ink left` | 6.0 | 4.0 | 7.0 | -3.0 | 1.0 px |
| `parity/root-selected/down-1/row:Left Half/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Left Half/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Left Half/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/row:Left Half/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Left Half/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/down-1/keys:win-alt-left/group width` | 86.0 | 80.5 | 85.62 | -5.12 | 1.0 px |
| `parity/root-selected/down-1/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 30.0 | -11.5 | 1.0 px |
| `parity/root-selected/down-1/keys:win-alt-left/cap 0 fill alpha` | 17.64 | 109.64 | 19.15 | 90.49 | 2.0 levels |
| `parity/root-selected/down-1/keys:win-alt-left/cap 0 label ink left` | 5.0 | 0.5 | 6.0 | -5.5 | 1.0 px |
| `parity/root-selected/down-1/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 30.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/keys:win-alt-left/cap 1 label ink left` | 5.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/keys:win-alt-left/cap 2 label ink left` | 6.0 | 4.0 | 7.0 | -3.0 | 1.0 px |
| `parity/root-selected/down-1/row:Search Files/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Search Files/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Search Files/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/row:Search Files/tile/glyph ink left` | 9.0 | 13.0 | 9.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Search Files/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Search Files/alias chip left in row` | 611.0 | 615.0 | 611.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Search Files/alias chip width` | 19.0 | 11.0 | 19.0 | -8.0 | 1.0 px |
| `parity/root-selected/down-1/section:Commands/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Left Half/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Left Half/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Figma/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/selected/row:Clipboard History/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/selected/row:Clipboard History/alias chip width` | 26.0 | 12.0 | 25.2 | -13.2 | 1.0 px |
| `harness-native/root-selected/selected/row:Clipboard History/alias chip gap to what follows` | 12.0 | 21.0 | 12.0 | 9.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:ctrl-shift-v/cap 0 left` | 534.0 | 532.0 | 534.6 | -2.6 | 1.0 px |
| `harness-native/root-selected/selected/keys:ctrl-shift-v/cap 0 width` | 37.0 | 41.0 | 36.4 | 4.6 | 1.0 px |
| `harness-native/root-selected/selected/keys:ctrl-shift-v/cap 1 left` | 574.0 | 572.0 | 574.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:ctrl-shift-v/cap 1 width` | 43.0 | 47.0 | 43.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:ctrl-shift-v/cap 1 fill alpha` | 17.64 | 21.35 | 18 | 3.35 | 2.0 levels |
| `harness-native/root-selected/selected/keys:ctrl-shift-v/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Left Half/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 0 left` | 554.0 | 555.5 | 554.4 | 1.1 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 0 top` | 204.0 | 211.0 | 204.0 | 7.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 29.8 | -11.3 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 0 fill alpha` | 18.1 | 107.56 | 18 | 89.56 | 2.0 levels |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 1 left` | 587.0 | 585.0 | 587.2 | -2.2 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 29.8 | 4.2 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Search Files/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/selected/row:Search Files/alias chip width` | 19.0 | 11.0 | 18.6 | -7.6 | 1.0 px |
| `harness-native/root-selected/selected/row:Search Files/alias chip gap to what follows` | 12.0 | 16.0 | 12.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Plugin Store/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/selected/row:Plugin Store/alias chip width` | 45.0 | 31.0 | 45.0 | -14.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Plugin Store/alias chip gap to what follows` | 12.0 | 23.0 | 12.0 | 11.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Toggle Dark Mode/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/selected/row:Lock Screen/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/selected/row:Settings/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/selected/keys:ctrl-,/cap 0 left` | 580.0 | 578.0 | 580.6 | -2.6 | 1.0 px |
| `harness-native/root-selected/selected/keys:ctrl-,/cap 0 width` | 37.0 | 41.0 | 36.4 | 4.6 | 1.0 px |
| `harness-native/root-selected/selected/keys:ctrl-,/cap 1 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:ctrl-,/cap 1 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Figma/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Figma/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/row:Figma/tile/gradient top (max channel)` | 1.0 | 207.5 | 0 | 207.5 | 4.0 levels |
| `parity/root-selected/selected/row:Figma/tile/gradient bottom (max channel)` | 2.5 | 140.0 | 0 | 140.0 | 4.0 levels |
| `parity/root-selected/selected/row:Figma/tile/glyph ink left` | 8.0 | 12.0 | 9.0 | 3.0 | 1.0 px |
| `parity/root-selected/selected/row:Figma/tile/glyph ink height` | 11.0 | 12.0 | 10.0 | 2.0 | 1.0 px |
| `parity/root-selected/selected/row:Figma/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 50.0 | 5.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/tile/glyph ink left` | 9.0 | 13.0 | 9.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/alias chip left in row` | 486.0 | 489.0 | 487.0 | 2.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/alias chip top in row` | 13.0 | 20.0 | 13.0 | 7.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/alias chip width` | 26.0 | 12.0 | 26.0 | -14.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/alias chip height` | 18.0 | 6.0 | 18.0 | -12.0 | 1.0 px |
| `parity/root-selected/selected/keys:ctrl-shift-v/group width` | 106.0 | 104.0 | 105.41 | -1.41 | 1.0 px |
| `parity/root-selected/selected/keys:ctrl-shift-v/cap 0 width` | 37.0 | 41.0 | 36.0 | 5.0 | 1.0 px |
| `parity/root-selected/selected/keys:ctrl-shift-v/cap 1 width` | 43.0 | 47.0 | 43.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/keys:ctrl-shift-v/cap 1 label ink left` | 6.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/keys:ctrl-shift-v/cap 2 label ink left` | 6.0 | 4.0 | 7.0 | -3.0 | 1.0 px |
| `parity/root-selected/selected/row:Left Half/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Left Half/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-selected/selected/row:Left Half/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Left Half/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Left Half/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/row:Left Half/tile/fill alpha` | 20.11 | 34.25 | 20.05 | 14.2 | 2.0 levels |
| `parity/root-selected/selected/row:Left Half/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Left Half/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/selected/keys:win-alt-left/group width` | 86.0 | 80.5 | 85.62 | -5.12 | 1.0 px |
| `parity/root-selected/selected/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 30.0 | -11.5 | 1.0 px |
| `parity/root-selected/selected/keys:win-alt-left/cap 0 fill alpha` | 18.1 | 107.56 | 18.67 | 88.89 | 2.0 levels |
| `parity/root-selected/selected/keys:win-alt-left/cap 0 label ink left` | 5.0 | 0.5 | 6.0 | -5.5 | 1.0 px |
| `parity/root-selected/selected/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 30.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/keys:win-alt-left/cap 1 label ink left` | 5.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/keys:win-alt-left/cap 2 label ink left` | 6.0 | 4.0 | 7.0 | -3.0 | 1.0 px |
| `parity/root-selected/selected/row:Search Files/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Search Files/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Search Files/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/row:Search Files/tile/glyph ink left` | 9.0 | 13.0 | 9.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Search Files/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/selected/row:Search Files/alias chip left in row` | 611.0 | 615.0 | 611.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Search Files/alias chip width` | 19.0 | 11.0 | 19.0 | -8.0 | 1.0 px |
| `parity/root-selected/selected/section:Commands/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |

## harness-native: 140 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-rest/rest/row:Figma/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-rest/rest/row:Clipboard History/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-rest/rest/row:Clipboard History/alias chip width` | 12.0 | 25.2 | -13.2 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/alias chip gap to what follows` | 21.0 | 12.0 | 9.0 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 0 left` | 532.0 | 534.6 | -2.6 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 0 width` | 41.0 | 36.4 | 4.6 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 1 left` | 572.0 | 574.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 1 width` | 47.0 | 43.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 1 fill alpha` | 21.35 | 18 | 3.35 | 2.0 levels |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-rest/rest/keys:win-alt-left/cap 0 left` | 555.5 | 554.4 | 1.1 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 0 top` | 211.0 | 204.0 | 7.0 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 0 width` | 18.5 | 29.8 | -11.3 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 0 fill alpha` | 109.64 | 18 | 91.64 | 2.0 levels |  |
| `root-rest/rest/keys:win-alt-left/cap 1 left` | 585.0 | 587.2 | -2.2 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 1 width` | 34.0 | 29.8 | 4.2 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-rest/rest/row:Search Files/alias chip width` | 11.0 | 18.6 | -7.6 | 1.0 px |  |
| `root-rest/rest/row:Search Files/alias chip gap to what follows` | 16.0 | 12.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Plugin Store/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-rest/rest/row:Plugin Store/alias chip width` | 31.0 | 45.0 | -14.0 | 1.0 px |  |
| `root-rest/rest/row:Plugin Store/alias chip gap to what follows` | 23.0 | 12.0 | 11.0 | 1.0 px |  |
| `root-rest/rest/row:Toggle Dark Mode/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-rest/rest/row:Lock Screen/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-rest/rest/row:Settings/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-rest/rest/keys:ctrl-,/cap 0 left` | 578.0 | 580.6 | -2.6 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-,/cap 0 width` | 41.0 | 36.4 | 4.6 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-,/cap 1 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-,/cap 1 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/rest/row:Clipboard History/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/rest/row:Clipboard History/alias chip width` | 12.0 | 25.2 | -13.2 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/alias chip gap to what follows` | 21.0 | 12.0 | 9.0 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 0 left` | 532.0 | 534.6 | -2.6 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 0 width` | 41.0 | 36.4 | 4.6 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 1 left` | 572.0 | 574.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 1 width` | 47.0 | 43.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 1 fill alpha` | 21.35 | 18 | 3.35 | 2.0 levels |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/rest/keys:win-alt-left/cap 0 left` | 555.5 | 554.4 | 1.1 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 0 top` | 211.0 | 204.0 | 7.0 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 0 width` | 18.5 | 29.8 | -11.3 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 0 fill alpha` | 109.64 | 18 | 91.64 | 2.0 levels |  |
| `root-selected/rest/keys:win-alt-left/cap 1 left` | 585.0 | 587.2 | -2.2 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 1 width` | 34.0 | 29.8 | 4.2 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/rest/row:Search Files/alias chip width` | 11.0 | 18.6 | -7.6 | 1.0 px |  |
| `root-selected/rest/row:Search Files/alias chip gap to what follows` | 16.0 | 12.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Plugin Store/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/rest/row:Plugin Store/alias chip width` | 31.0 | 45.0 | -14.0 | 1.0 px |  |
| `root-selected/rest/row:Plugin Store/alias chip gap to what follows` | 23.0 | 12.0 | 11.0 | 1.0 px |  |
| `root-selected/rest/row:Toggle Dark Mode/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/rest/row:Lock Screen/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/rest/row:Settings/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/rest/keys:ctrl-,/cap 0 left` | 578.0 | 580.6 | -2.6 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-,/cap 0 width` | 41.0 | 36.4 | 4.6 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-,/cap 1 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-,/cap 1 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/down-1/row:Clipboard History/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/down-1/row:Clipboard History/alias chip width` | 12.0 | 25.2 | -13.2 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/alias chip gap to what follows` | 21.0 | 12.0 | 9.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 0 left` | 532.0 | 534.6 | -2.6 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 0 width` | 41.0 | 36.4 | 4.6 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 1 left` | 572.0 | 574.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 1 width` | 47.0 | 43.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 1 fill alpha` | 21.76 | 18 | 3.76 | 2.0 levels |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/down-1/keys:win-alt-left/cap 0 left` | 555.5 | 554.4 | 1.1 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 0 top` | 211.0 | 204.0 | 7.0 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 0 width` | 18.5 | 29.8 | -11.3 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 0 fill alpha` | 109.64 | 18 | 91.64 | 2.0 levels |  |
| `root-selected/down-1/keys:win-alt-left/cap 1 left` | 585.0 | 587.2 | -2.2 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 1 width` | 34.0 | 29.8 | 4.2 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/down-1/row:Search Files/alias chip width` | 11.0 | 18.6 | -7.6 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/alias chip gap to what follows` | 16.0 | 12.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Plugin Store/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/down-1/row:Plugin Store/alias chip width` | 31.0 | 45.0 | -14.0 | 1.0 px |  |
| `root-selected/down-1/row:Plugin Store/alias chip gap to what follows` | 23.0 | 12.0 | 11.0 | 1.0 px |  |
| `root-selected/down-1/row:Toggle Dark Mode/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/down-1/row:Lock Screen/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/down-1/row:Settings/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/down-1/keys:ctrl-,/cap 0 left` | 578.0 | 580.6 | -2.6 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-,/cap 0 width` | 41.0 | 36.4 | 4.6 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-,/cap 1 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-,/cap 1 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/selected/row:Clipboard History/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/selected/row:Clipboard History/alias chip width` | 12.0 | 25.2 | -13.2 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/alias chip gap to what follows` | 21.0 | 12.0 | 9.0 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 0 left` | 532.0 | 534.6 | -2.6 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 0 width` | 41.0 | 36.4 | 4.6 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 1 left` | 572.0 | 574.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 1 width` | 47.0 | 43.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 1 fill alpha` | 21.35 | 18 | 3.35 | 2.0 levels |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/selected/keys:win-alt-left/cap 0 left` | 555.5 | 554.4 | 1.1 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 0 top` | 211.0 | 204.0 | 7.0 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 0 width` | 18.5 | 29.8 | -11.3 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 0 fill alpha` | 107.56 | 18 | 89.56 | 2.0 levels |  |
| `root-selected/selected/keys:win-alt-left/cap 1 left` | 585.0 | 587.2 | -2.2 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 1 width` | 34.0 | 29.8 | 4.2 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/selected/row:Search Files/alias chip width` | 11.0 | 18.6 | -7.6 | 1.0 px |  |
| `root-selected/selected/row:Search Files/alias chip gap to what follows` | 16.0 | 12.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Plugin Store/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/selected/row:Plugin Store/alias chip width` | 31.0 | 45.0 | -14.0 | 1.0 px |  |
| `root-selected/selected/row:Plugin Store/alias chip gap to what follows` | 23.0 | 12.0 | 11.0 | 1.0 px |  |
| `root-selected/selected/row:Toggle Dark Mode/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/selected/row:Lock Screen/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/selected/row:Settings/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/selected/keys:ctrl-,/cap 0 left` | 578.0 | 580.6 | -2.6 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-,/cap 0 width` | 41.0 | 36.4 | 4.6 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-,/cap 1 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-,/cap 1 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |

## harness-reference: 0 failed


## parity: 197 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-rest/rest/row:Figma/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/top in client` | 100.0 | 242 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/tile/gradient top (max channel)` | 191.5 | 0 | 191.5 | 4.0 levels | native #383236 vs reference #F86996 |
| `root-rest/rest/row:Figma/tile/gradient bottom (max channel)` | 127.5 | 0 | 127.5 | 4.0 levels | native #583140 vs reference #D83A6E |
| `root-rest/rest/row:Figma/tile/glyph ink left` | 12.0 | 9.0 | 3.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/top in client` | 146.0 | 288 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Clipboard History/title ink left in row` | 55.0 | 50.0 | 5.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/alias chip left in row` | 489.0 | 487.0 | 2.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/alias chip top in row` | 20.0 | 13.0 | 7.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/alias chip width` | 12.0 | 26.0 | -14.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/alias chip height` | 6.0 | 18.0 | -12.0 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/group width` | 104.0 | 105.41 | -1.41 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 0 width` | 41.0 | 36.0 | 5.0 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 1 width` | 47.0 | 43.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/top in client` | 192.0 | 334 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/group width` | 80.5 | 85.62 | -5.12 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 0 width` | 18.5 | 30.0 | -11.5 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 0 fill alpha` | 109.64 | 19.15 | 90.49 | 2.0 levels |  |
| `root-rest/rest/keys:win-alt-left/cap 0 label ink left` | 0.5 | 6.0 | -5.5 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 1 width` | 34.0 | 30.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/top in client` | 238.0 | 380 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/alias chip left in row` | 615.0 | 611.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/alias chip width` | 11.0 | 19.0 | -8.0 | 1.0 px |  |
| `root-rest/rest/section:Commands/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/top in client` | 100.0 | 242 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/tile/gradient top (max channel)` | 191.5 | 0 | 191.5 | 4.0 levels | native #383236 vs reference #F86996 |
| `root-selected/rest/row:Figma/tile/gradient bottom (max channel)` | 127.5 | 0 | 127.5 | 4.0 levels | native #583140 vs reference #D83A6E |
| `root-selected/rest/row:Figma/tile/glyph ink left` | 12.0 | 9.0 | 3.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/top in client` | 146.0 | 288 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Clipboard History/title ink left in row` | 55.0 | 50.0 | 5.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/alias chip left in row` | 489.0 | 487.0 | 2.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/alias chip top in row` | 20.0 | 13.0 | 7.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/alias chip width` | 12.0 | 26.0 | -14.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/alias chip height` | 6.0 | 18.0 | -12.0 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/group width` | 104.0 | 105.41 | -1.41 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 0 width` | 41.0 | 36.0 | 5.0 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 1 width` | 47.0 | 43.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/top in client` | 192.0 | 334 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/group width` | 80.5 | 85.62 | -5.12 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 0 width` | 18.5 | 30.0 | -11.5 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 0 fill alpha` | 109.64 | 19.15 | 90.49 | 2.0 levels |  |
| `root-selected/rest/keys:win-alt-left/cap 0 label ink left` | 0.5 | 6.0 | -5.5 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 1 width` | 34.0 | 30.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/top in client` | 238.0 | 380 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/alias chip left in row` | 615.0 | 611.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/alias chip width` | 11.0 | 19.0 | -8.0 | 1.0 px |  |
| `root-selected/rest/section:Commands/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/top in client` | 100.0 | 242 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/tile/gradient top (max channel)` | 207.5 | 0 | 207.5 | 4.0 levels | native #282226 vs reference #F86996 |
| `root-selected/down-1/row:Figma/tile/gradient bottom (max channel)` | 140.0 | 0 | 140.0 | 4.0 levels | native #4C2634 vs reference #D83A6E |
| `root-selected/down-1/row:Figma/tile/glyph ink left` | 12.0 | 9.0 | 3.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/tile/glyph ink height` | 12.0 | 10.0 | 2.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/top in client` | 146.0 | 288 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Clipboard History/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/tile/fill alpha` | 34.25 | 20.05 | 14.2 | 2.0 levels |  |
| `root-selected/down-1/row:Clipboard History/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/alias chip left in row` | 489.0 | 487.0 | 2.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/alias chip top in row` | 20.0 | 13.0 | 7.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/alias chip width` | 12.0 | 26.0 | -14.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/alias chip height` | 6.0 | 18.0 | -12.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/group width` | 104.0 | 105.41 | -1.41 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 0 width` | 41.0 | 36.0 | 5.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 1 width` | 47.0 | 43.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 1 fill alpha` | 21.76 | 19.17 | 2.59 | 2.0 levels |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/top in client` | 192.0 | 334 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/group width` | 80.5 | 85.62 | -5.12 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 0 width` | 18.5 | 30.0 | -11.5 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 0 fill alpha` | 109.64 | 19.15 | 90.49 | 2.0 levels |  |
| `root-selected/down-1/keys:win-alt-left/cap 0 label ink left` | 0.5 | 6.0 | -5.5 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 1 width` | 34.0 | 30.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/top in client` | 238.0 | 380 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/alias chip left in row` | 615.0 | 611.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/alias chip width` | 11.0 | 19.0 | -8.0 | 1.0 px |  |
| `root-selected/down-1/section:Commands/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/top in client` | 100.0 | 242 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/tile/gradient top (max channel)` | 207.5 | 0 | 207.5 | 4.0 levels | native #282226 vs reference #F86996 |
| `root-selected/selected/row:Figma/tile/gradient bottom (max channel)` | 140.0 | 0 | 140.0 | 4.0 levels | native #4C2634 vs reference #D83A6E |
| `root-selected/selected/row:Figma/tile/glyph ink left` | 12.0 | 9.0 | 3.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/tile/glyph ink height` | 12.0 | 10.0 | 2.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/top in client` | 146.0 | 288 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Clipboard History/title ink left in row` | 55.0 | 50.0 | 5.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/alias chip left in row` | 489.0 | 487.0 | 2.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/alias chip top in row` | 20.0 | 13.0 | 7.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/alias chip width` | 12.0 | 26.0 | -14.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/alias chip height` | 6.0 | 18.0 | -12.0 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/group width` | 104.0 | 105.41 | -1.41 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 0 width` | 41.0 | 36.0 | 5.0 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 1 width` | 47.0 | 43.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/top in client` | 192.0 | 334 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/tile/fill alpha` | 34.25 | 20.05 | 14.2 | 2.0 levels |  |
| `root-selected/selected/row:Left Half/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/group width` | 80.5 | 85.62 | -5.12 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 0 width` | 18.5 | 30.0 | -11.5 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 0 fill alpha` | 107.56 | 18.67 | 88.89 | 2.0 levels |  |
| `root-selected/selected/keys:win-alt-left/cap 0 label ink left` | 0.5 | 6.0 | -5.5 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 1 width` | 34.0 | 30.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/top in client` | 238.0 | 380 | -142.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/alias chip left in row` | 615.0 | 611.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/alias chip width` | 11.0 | 19.0 | -8.0 | 1.0 px |  |
| `root-selected/selected/section:Commands/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |

## Accepted discrepancies: 12 (failing, with a recorded disposition)

| Check | Native/measured | Reference/expected | Delta | Limit | Disposition |
|---|---|---|---|---|---|
| `root-rest/rest/keys:win-alt-left/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-rest/rest/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-rest/rest/sections/labels` | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; the reference's pinned strip and its label are #101's |
| `root-selected/rest/keys:win-alt-left/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/rest/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/rest/sections/labels` | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; the reference's pinned strip and its label are #101's |
| `root-selected/down-1/keys:win-alt-left/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/down-1/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/down-1/sections/labels` | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; the reference's pinned strip and its label are #101's |
| `root-selected/selected/keys:win-alt-left/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/selected/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/selected/sections/labels` | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; the reference's pinned strip and its label are #101's |

## Pending scenarios (reference saved; native fixture not registered yet)

- `calculator-card` (calculator board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `empty-state` (empty board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `settings-shell` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/97
- `appearance-page` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/98
- `pinned-strip` (root board, 760x518): https://github.com/hoangvu12/pane/issues/101
- `clipboard-split` (clipboard board, 940x600): https://github.com/hoangvu12/pane/issues/102
