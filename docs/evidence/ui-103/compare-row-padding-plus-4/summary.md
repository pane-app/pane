# Visual workbench comparison: row-padding-plus-4

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `root-rest`: 96 DPI, client [760, 518], opaque, perturbation row-padding-plus-4
- `root-selected`: 96 DPI, client [760, 518], opaque, perturbation row-padding-plus-4

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 101 | 0 | 0 |
| harness-native | 570 | 212 | 0 |
| parity | 1326 | 266 | 20 |

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
| `harness-native/root-rest/rest/keys:ctrl-shift-v/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/row:Left Half/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 0 left` | 554.0 | 555.5 | 554.4 | 1.1 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 0 top` | 346.0 | 353.0 | 346.0 | 7.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 29.8 | -11.3 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 1 left` | 587.0 | 585.0 | 587.2 | -2.2 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 29.8 | 4.2 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/row:Search Files/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-rest/rest/row:Search Files/alias chip width` | 19.0 | 11.0 | 18.6 | -7.6 | 1.0 px |
| `harness-native/root-rest/rest/row:Search Files/alias chip gap to what follows` | 12.0 | 16.0 | 12.0 | 4.0 | 1.0 px |
| `harness-native/root-rest/rest/slot:1/box left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-rest/rest/slot:1/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-rest/rest/slot:1/tile left` | 60.0 | 63.0 | 59.8 | 3.2 | 1.0 px |
| `harness-native/root-rest/rest/slot:1/tile width` | 42.0 | 40.0 | 42.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:quick-slot-1/cap 0 left` | 92.0 | 94.0 | 91.6 | 2.4 | 1.5 px |
| `harness-native/root-rest/rest/keys:quick-slot-1/cap 1 left` | 127.0 | 124.0 | 126.6 | -2.6 | 1.5 px |
| `harness-native/root-rest/rest/keys:quick-slot-1/cap 1 width` | 17.0 | 21.0 | 17.0 | 4.0 | 1.0 px |
| `harness-native/root-rest/rest/slot:1/title ink center` | 80.5 | 83.5 | 80.8 | 2.7 | 1.5 px |
| `harness-native/root-rest/rest/slot:2/box left` | 160.0 | 162.0 | 159.6 | 2.4 | 1.0 px |
| `harness-native/root-rest/rest/slot:2/box width` | 141.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-rest/rest/slot:2/tile left` | 209.0 | 211.0 | 209.4 | 1.6 | 1.0 px |
| `harness-native/root-rest/rest/slot:3/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-rest/rest/slot:4/box width` | 141.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-rest/rest/slot:4/tile left` | 509.0 | 507.0 | 508.6 | -1.6 | 1.0 px |
| `harness-native/root-rest/rest/keys:quick-slot-4/cap 0 left` | 540.0 | 538.0 | 540.4 | -2.4 | 1.5 px |
| `harness-native/root-rest/rest/keys:quick-slot-4/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:quick-slot-4/cap 1 left` | 575.0 | 573.0 | 575.4 | -2.4 | 1.5 px |
| `harness-native/root-rest/rest/slot:4/title ink center` | 530.0 | 528.0 | 529.6 | -1.6 | 1.5 px |
| `harness-native/root-rest/rest/slot:5/box left` | 608.0 | 606.0 | 608.4 | -2.4 | 1.0 px |
| `harness-native/root-rest/rest/slot:5/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-rest/rest/slot:5/tile left` | 658.0 | 656.0 | 658.2 | -2.2 | 1.0 px |
| `harness-native/root-rest/rest/keys:quick-slot-5/cap 0 left` | 690.0 | 688.0 | 690.0 | -2.0 | 1.5 px |
| `harness-native/root-rest/rest/keys:quick-slot-5/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `harness-native/root-rest/rest/keys:quick-slot-5/cap 1 left` | 725.0 | 723.0 | 725.0 | -2.0 | 1.5 px |
| `harness-native/root-rest/rest/keys:quick-slot-5/cap 1 width` | 17.0 | 15.0 | 17.0 | -2.0 | 1.0 px |
| `harness-native/root-rest/rest/slot:5/title ink center` | 679.5 | 676.5 | 679.2 | -2.7 | 1.5 px |
| `harness-native/root-rest/rest/keys:quick-slots/cap 0 left` | 669.0 | 667.0 | 669.96 | -2.96 | 1.5 px |
| `harness-native/root-rest/rest/keys:quick-slots/cap 0 width` | 37.0 | 41.0 | 36.88 | 4.12 | 1.0 px |
| `harness-native/root-rest/rest/keys:quick-slots/cap 1 left` | 709.0 | 707.0 | 709.84 | -2.84 | 1.5 px |
| `harness-native/root-rest/rest/keys:quick-slots/cap 1 width` | 31.0 | 29.0 | 30.16 | -1.16 | 1.0 px |
| `parity/root-rest/rest/row:Figma/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Figma/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 50.0 | 5.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/row:Clipboard History/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
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
| `parity/root-rest/rest/row:Left Half/tile/glyph ink left` | 7.0 | 11.0 | 7.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/row:Left Half/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-rest/rest/keys:win-alt-left/group width` | 86.0 | 80.5 | 85.62 | -5.12 | 1.0 px |
| `parity/root-rest/rest/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 30.0 | -11.5 | 1.0 px |
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
| `parity/root-rest/rest/section:Pinned/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |
| `parity/root-rest/rest/section:Pinned/note ink right in label` | 724.0 | 720.0 | 725 | -5.0 | 1.5 px |
| `parity/root-rest/rest/section:Commands/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |
| `parity/root-rest/rest/keys:quick-slots/group width` | 71.0 | 69.0 | 70.06 | -1.06 | 1.0 px |
| `parity/root-rest/rest/keys:quick-slots/cap 0 width` | 37.0 | 41.0 | 37.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/slot:1/box left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/slot:1/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/slot:1/title ink center in slot` | 71.0 | 74.0 | 70.5 | 3.5 | 1.0 px |
| `parity/root-rest/rest/slot:1/tile/left in slot` | 50.0 | 53.0 | 50.0 | 3.0 | 1.0 px |
| `parity/root-rest/rest/slot:1/tile/width` | 42.0 | 40.0 | 42.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/keys:quick-slot-1/cap 1 width` | 17.0 | 21.0 | 17.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/keys:quick-slot-1/cap 1 label ink left` | 6.0 | 11.0 | 5.0 | 6.0 | 1.0 px |
| `parity/root-rest/rest/slot:2/box left` | 160.0 | 162.0 | 160.0 | 2.0 | 1.0 px |
| `parity/root-rest/rest/slot:2/tile/left in slot` | 49.4 | 51.4 | 49.41 | 1.99 | 1.0 px |
| `parity/root-rest/rest/slot:3/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/slot:4/title ink center in slot` | 70.7 | 68.7 | 70.7 | -2.0 | 1.0 px |
| `parity/root-rest/rest/slot:4/tile/left in slot` | 50.2 | 48.2 | 50.2 | -2.0 | 1.0 px |
| `parity/root-rest/rest/keys:quick-slot-4/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/slot:5/box left` | 608.0 | 606.0 | 608.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/slot:5/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/slot:5/title ink center in slot` | 71.1 | 68.1 | 71.11 | -3.01 | 1.0 px |
| `parity/root-rest/rest/slot:5/tile/left in slot` | 49.6 | 47.6 | 49.61 | -2.01 | 1.0 px |
| `parity/root-rest/rest/keys:quick-slot-5/group width` | 52.0 | 50.0 | 52 | -2.0 | 1.0 px |
| `parity/root-rest/rest/keys:quick-slot-5/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `parity/root-rest/rest/keys:quick-slot-5/cap 1 width` | 17.0 | 15.0 | 17.0 | -2.0 | 1.0 px |
| `parity/root-rest/rest/keys:quick-slot-5/cap 1 label ink left` | 6.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
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
| `harness-native/root-selected/rest/keys:ctrl-shift-v/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Left Half/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 0 left` | 554.0 | 555.5 | 554.4 | 1.1 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 0 top` | 346.0 | 353.0 | 346.0 | 7.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 29.8 | -11.3 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 1 left` | 587.0 | 585.0 | 587.2 | -2.2 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 29.8 | 4.2 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/row:Search Files/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/rest/row:Search Files/alias chip width` | 19.0 | 11.0 | 18.6 | -7.6 | 1.0 px |
| `harness-native/root-selected/rest/row:Search Files/alias chip gap to what follows` | 12.0 | 16.0 | 12.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/rest/slot:1/box left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/rest/slot:1/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/rest/slot:1/tile left` | 60.0 | 63.0 | 59.8 | 3.2 | 1.0 px |
| `harness-native/root-selected/rest/slot:1/tile width` | 42.0 | 40.0 | 42.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:quick-slot-1/cap 0 left` | 92.0 | 94.0 | 91.6 | 2.4 | 1.5 px |
| `harness-native/root-selected/rest/keys:quick-slot-1/cap 1 left` | 127.0 | 124.0 | 126.6 | -2.6 | 1.5 px |
| `harness-native/root-selected/rest/keys:quick-slot-1/cap 1 width` | 17.0 | 21.0 | 17.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/rest/slot:1/title ink center` | 80.5 | 83.5 | 80.8 | 2.7 | 1.5 px |
| `harness-native/root-selected/rest/slot:2/box left` | 160.0 | 162.0 | 159.6 | 2.4 | 1.0 px |
| `harness-native/root-selected/rest/slot:2/box width` | 141.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/rest/slot:2/tile left` | 209.0 | 211.0 | 209.4 | 1.6 | 1.0 px |
| `harness-native/root-selected/rest/slot:3/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/rest/slot:4/box width` | 141.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/rest/slot:4/tile left` | 509.0 | 507.0 | 508.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/rest/keys:quick-slot-4/cap 0 left` | 540.0 | 538.0 | 540.4 | -2.4 | 1.5 px |
| `harness-native/root-selected/rest/keys:quick-slot-4/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:quick-slot-4/cap 1 left` | 575.0 | 573.0 | 575.4 | -2.4 | 1.5 px |
| `harness-native/root-selected/rest/slot:4/title ink center` | 530.0 | 528.0 | 529.6 | -1.6 | 1.5 px |
| `harness-native/root-selected/rest/slot:5/box left` | 608.0 | 606.0 | 608.4 | -2.4 | 1.0 px |
| `harness-native/root-selected/rest/slot:5/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/rest/slot:5/tile left` | 658.0 | 656.0 | 658.2 | -2.2 | 1.0 px |
| `harness-native/root-selected/rest/keys:quick-slot-5/cap 0 left` | 690.0 | 688.0 | 690.0 | -2.0 | 1.5 px |
| `harness-native/root-selected/rest/keys:quick-slot-5/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/rest/keys:quick-slot-5/cap 1 left` | 725.0 | 723.0 | 725.0 | -2.0 | 1.5 px |
| `harness-native/root-selected/rest/keys:quick-slot-5/cap 1 width` | 17.0 | 15.0 | 17.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/rest/slot:5/title ink center` | 679.5 | 676.5 | 679.2 | -2.7 | 1.5 px |
| `harness-native/root-selected/rest/keys:quick-slots/cap 0 left` | 669.0 | 667.0 | 669.96 | -2.96 | 1.5 px |
| `harness-native/root-selected/rest/keys:quick-slots/cap 0 width` | 37.0 | 41.0 | 36.88 | 4.12 | 1.0 px |
| `harness-native/root-selected/rest/keys:quick-slots/cap 1 left` | 709.0 | 707.0 | 709.84 | -2.84 | 1.5 px |
| `harness-native/root-selected/rest/keys:quick-slots/cap 1 width` | 31.0 | 29.0 | 30.16 | -1.16 | 1.0 px |
| `parity/root-selected/rest/row:Figma/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Figma/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 50.0 | 5.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/row:Clipboard History/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
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
| `parity/root-selected/rest/row:Left Half/tile/glyph ink left` | 7.0 | 11.0 | 7.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/row:Left Half/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/rest/keys:win-alt-left/group width` | 86.0 | 80.5 | 85.62 | -5.12 | 1.0 px |
| `parity/root-selected/rest/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 30.0 | -11.5 | 1.0 px |
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
| `parity/root-selected/rest/section:Pinned/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |
| `parity/root-selected/rest/section:Pinned/note ink right in label` | 724.0 | 720.0 | 725 | -5.0 | 1.5 px |
| `parity/root-selected/rest/section:Commands/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |
| `parity/root-selected/rest/keys:quick-slots/group width` | 71.0 | 69.0 | 70.06 | -1.06 | 1.0 px |
| `parity/root-selected/rest/keys:quick-slots/cap 0 width` | 37.0 | 41.0 | 37.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/slot:1/box left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/slot:1/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/slot:1/title ink center in slot` | 71.0 | 74.0 | 70.5 | 3.5 | 1.0 px |
| `parity/root-selected/rest/slot:1/tile/left in slot` | 50.0 | 53.0 | 50.0 | 3.0 | 1.0 px |
| `parity/root-selected/rest/slot:1/tile/width` | 42.0 | 40.0 | 42.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/keys:quick-slot-1/cap 1 width` | 17.0 | 21.0 | 17.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/keys:quick-slot-1/cap 1 label ink left` | 6.0 | 11.0 | 5.0 | 6.0 | 1.0 px |
| `parity/root-selected/rest/slot:2/box left` | 160.0 | 162.0 | 160.0 | 2.0 | 1.0 px |
| `parity/root-selected/rest/slot:2/tile/left in slot` | 49.4 | 51.4 | 49.41 | 1.99 | 1.0 px |
| `parity/root-selected/rest/slot:3/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/slot:4/title ink center in slot` | 70.7 | 68.7 | 70.7 | -2.0 | 1.0 px |
| `parity/root-selected/rest/slot:4/tile/left in slot` | 50.2 | 48.2 | 50.2 | -2.0 | 1.0 px |
| `parity/root-selected/rest/keys:quick-slot-4/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/slot:5/box left` | 608.0 | 606.0 | 608.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/slot:5/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/slot:5/title ink center in slot` | 71.1 | 68.1 | 71.11 | -3.01 | 1.0 px |
| `parity/root-selected/rest/slot:5/tile/left in slot` | 49.6 | 47.6 | 49.61 | -2.01 | 1.0 px |
| `parity/root-selected/rest/keys:quick-slot-5/group width` | 52.0 | 50.0 | 52 | -2.0 | 1.0 px |
| `parity/root-selected/rest/keys:quick-slot-5/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `parity/root-selected/rest/keys:quick-slot-5/cap 1 width` | 17.0 | 15.0 | 17.0 | -2.0 | 1.0 px |
| `parity/root-selected/rest/keys:quick-slot-5/cap 1 label ink left` | 6.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
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
| `harness-native/root-selected/down-1/keys:ctrl-shift-v/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Left Half/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 0 left` | 554.0 | 555.5 | 554.4 | 1.1 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 0 top` | 346.0 | 353.0 | 346.0 | 7.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 29.8 | -11.3 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 1 left` | 587.0 | 585.0 | 587.2 | -2.2 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 29.8 | 4.2 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/row:Search Files/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/down-1/row:Search Files/alias chip width` | 19.0 | 11.0 | 18.6 | -7.6 | 1.0 px |
| `harness-native/root-selected/down-1/row:Search Files/alias chip gap to what follows` | 12.0 | 16.0 | 12.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/down-1/slot:1/box left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/down-1/slot:1/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/down-1/slot:1/tile left` | 60.0 | 63.0 | 59.8 | 3.2 | 1.0 px |
| `harness-native/root-selected/down-1/slot:1/tile width` | 42.0 | 40.0 | 42.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:quick-slot-1/cap 0 left` | 92.0 | 94.0 | 91.6 | 2.4 | 1.5 px |
| `harness-native/root-selected/down-1/keys:quick-slot-1/cap 1 left` | 127.0 | 124.0 | 126.6 | -2.6 | 1.5 px |
| `harness-native/root-selected/down-1/keys:quick-slot-1/cap 1 width` | 17.0 | 21.0 | 17.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/down-1/slot:1/title ink center` | 80.5 | 83.5 | 80.8 | 2.7 | 1.5 px |
| `harness-native/root-selected/down-1/slot:2/box left` | 160.0 | 162.0 | 159.6 | 2.4 | 1.0 px |
| `harness-native/root-selected/down-1/slot:2/box width` | 141.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/down-1/slot:2/tile left` | 209.0 | 211.0 | 209.4 | 1.6 | 1.0 px |
| `harness-native/root-selected/down-1/slot:3/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/down-1/slot:4/box width` | 141.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/down-1/slot:4/tile left` | 509.0 | 507.0 | 508.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/down-1/keys:quick-slot-4/cap 0 left` | 540.0 | 538.0 | 540.4 | -2.4 | 1.5 px |
| `harness-native/root-selected/down-1/keys:quick-slot-4/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:quick-slot-4/cap 1 left` | 575.0 | 573.0 | 575.4 | -2.4 | 1.5 px |
| `harness-native/root-selected/down-1/slot:4/title ink center` | 530.0 | 528.0 | 529.6 | -1.6 | 1.5 px |
| `harness-native/root-selected/down-1/slot:5/box left` | 608.0 | 606.0 | 608.4 | -2.4 | 1.0 px |
| `harness-native/root-selected/down-1/slot:5/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/down-1/slot:5/tile left` | 658.0 | 656.0 | 658.2 | -2.2 | 1.0 px |
| `harness-native/root-selected/down-1/keys:quick-slot-5/cap 0 left` | 690.0 | 688.0 | 690.0 | -2.0 | 1.5 px |
| `harness-native/root-selected/down-1/keys:quick-slot-5/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/down-1/keys:quick-slot-5/cap 1 left` | 725.0 | 723.0 | 725.0 | -2.0 | 1.5 px |
| `harness-native/root-selected/down-1/keys:quick-slot-5/cap 1 width` | 17.0 | 15.0 | 17.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/down-1/slot:5/title ink center` | 679.5 | 676.5 | 679.2 | -2.7 | 1.5 px |
| `harness-native/root-selected/down-1/keys:quick-slots/cap 0 left` | 669.0 | 667.0 | 669.96 | -2.96 | 1.5 px |
| `harness-native/root-selected/down-1/keys:quick-slots/cap 0 width` | 37.0 | 41.0 | 36.88 | 4.12 | 1.0 px |
| `harness-native/root-selected/down-1/keys:quick-slots/cap 1 left` | 709.0 | 707.0 | 709.84 | -2.84 | 1.5 px |
| `harness-native/root-selected/down-1/keys:quick-slots/cap 1 width` | 31.0 | 29.0 | 30.16 | -1.16 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Figma/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/wash left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/wash width` | 740.0 | 732.0 | 740.0 | -8.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/tile/fill alpha` | 20.11 | 34.25 | 20.05 | 14.2 | 2.0 levels |
| `parity/root-selected/down-1/row:Clipboard History/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/alias chip left in row` | 486.0 | 489.0 | 487.0 | 2.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/alias chip top in row` | 13.0 | 20.0 | 13.0 | 7.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/alias chip width` | 26.0 | 12.0 | 26.0 | -14.0 | 1.0 px |
| `parity/root-selected/down-1/row:Clipboard History/alias chip height` | 18.0 | 6.0 | 18.0 | -12.0 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/group width` | 106.0 | 104.0 | 105.41 | -1.41 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/cap 0 width` | 37.0 | 41.0 | 36.0 | 5.0 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/cap 1 width` | 43.0 | 47.0 | 43.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/cap 1 label ink left` | 6.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/keys:ctrl-shift-v/cap 2 label ink left` | 6.0 | 4.0 | 7.0 | -3.0 | 1.0 px |
| `parity/root-selected/down-1/row:Left Half/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Left Half/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Left Half/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/row:Left Half/tile/glyph ink left` | 7.0 | 11.0 | 7.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/row:Left Half/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/down-1/keys:win-alt-left/group width` | 86.0 | 80.5 | 85.62 | -5.12 | 1.0 px |
| `parity/root-selected/down-1/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 30.0 | -11.5 | 1.0 px |
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
| `parity/root-selected/down-1/section:Pinned/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/section:Pinned/note ink right in label` | 724.0 | 720.0 | 725 | -5.0 | 1.5 px |
| `parity/root-selected/down-1/section:Commands/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/keys:quick-slots/group width` | 71.0 | 69.0 | 70.06 | -1.06 | 1.0 px |
| `parity/root-selected/down-1/keys:quick-slots/cap 0 width` | 37.0 | 41.0 | 37.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/slot:1/box left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/slot:1/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/slot:1/title ink center in slot` | 71.0 | 74.0 | 70.5 | 3.5 | 1.0 px |
| `parity/root-selected/down-1/slot:1/tile/left in slot` | 50.0 | 53.0 | 50.0 | 3.0 | 1.0 px |
| `parity/root-selected/down-1/slot:1/tile/width` | 42.0 | 40.0 | 42.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/keys:quick-slot-1/cap 1 width` | 17.0 | 21.0 | 17.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/keys:quick-slot-1/cap 1 label ink left` | 6.0 | 11.0 | 5.0 | 6.0 | 1.0 px |
| `parity/root-selected/down-1/slot:2/box left` | 160.0 | 162.0 | 160.0 | 2.0 | 1.0 px |
| `parity/root-selected/down-1/slot:2/tile/left in slot` | 49.4 | 51.4 | 49.41 | 1.99 | 1.0 px |
| `parity/root-selected/down-1/slot:3/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/slot:4/title ink center in slot` | 70.7 | 68.7 | 70.7 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/slot:4/tile/left in slot` | 50.2 | 48.2 | 50.2 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/keys:quick-slot-4/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/slot:5/box left` | 608.0 | 606.0 | 608.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/slot:5/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/slot:5/title ink center in slot` | 71.1 | 68.1 | 71.11 | -3.01 | 1.0 px |
| `parity/root-selected/down-1/slot:5/tile/left in slot` | 49.6 | 47.6 | 49.61 | -2.01 | 1.0 px |
| `parity/root-selected/down-1/keys:quick-slot-5/group width` | 52.0 | 50.0 | 52 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/keys:quick-slot-5/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `parity/root-selected/down-1/keys:quick-slot-5/cap 1 width` | 17.0 | 15.0 | 17.0 | -2.0 | 1.0 px |
| `parity/root-selected/down-1/keys:quick-slot-5/cap 1 label ink left` | 6.0 | 4.0 | 6.0 | -2.0 | 1.0 px |
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
| `harness-native/root-selected/selected/keys:ctrl-shift-v/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:ctrl-shift-v/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Left Half/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 0 left` | 554.0 | 555.5 | 554.4 | 1.1 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 0 top` | 346.0 | 353.0 | 346.0 | 7.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 29.8 | -11.3 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 1 left` | 587.0 | 585.0 | 587.2 | -2.2 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 1 width` | 30.0 | 34.0 | 29.8 | 4.2 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 2 left` | 620.0 | 618.0 | 620.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:win-alt-left/cap 2 width` | 20.0 | 18.0 | 20.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/row:Search Files/kind ink right` | 739.0 | 735.0 | 740.0 | -5.0 | 1.5 px |
| `harness-native/root-selected/selected/row:Search Files/alias chip width` | 19.0 | 11.0 | 18.6 | -7.6 | 1.0 px |
| `harness-native/root-selected/selected/row:Search Files/alias chip gap to what follows` | 12.0 | 16.0 | 12.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/selected/slot:1/box left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/selected/slot:1/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/selected/slot:1/tile left` | 60.0 | 63.0 | 59.8 | 3.2 | 1.0 px |
| `harness-native/root-selected/selected/slot:1/tile width` | 42.0 | 40.0 | 42.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:quick-slot-1/cap 0 left` | 92.0 | 94.0 | 91.6 | 2.4 | 1.5 px |
| `harness-native/root-selected/selected/keys:quick-slot-1/cap 1 left` | 127.0 | 124.0 | 126.6 | -2.6 | 1.5 px |
| `harness-native/root-selected/selected/keys:quick-slot-1/cap 1 width` | 17.0 | 21.0 | 17.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/selected/slot:1/title ink center` | 80.5 | 83.5 | 80.8 | 2.7 | 1.5 px |
| `harness-native/root-selected/selected/slot:2/box left` | 160.0 | 162.0 | 159.6 | 2.4 | 1.0 px |
| `harness-native/root-selected/selected/slot:2/box width` | 141.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/selected/slot:2/tile left` | 209.0 | 211.0 | 209.4 | 1.6 | 1.0 px |
| `harness-native/root-selected/selected/slot:3/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/selected/slot:4/box width` | 141.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/selected/slot:4/tile left` | 509.0 | 507.0 | 508.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/selected/keys:quick-slot-4/cap 0 left` | 540.0 | 538.0 | 540.4 | -2.4 | 1.5 px |
| `harness-native/root-selected/selected/keys:quick-slot-4/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:quick-slot-4/cap 1 left` | 575.0 | 573.0 | 575.4 | -2.4 | 1.5 px |
| `harness-native/root-selected/selected/slot:4/title ink center` | 530.0 | 528.0 | 529.6 | -1.6 | 1.5 px |
| `harness-native/root-selected/selected/slot:5/box left` | 608.0 | 606.0 | 608.4 | -2.4 | 1.0 px |
| `harness-native/root-selected/selected/slot:5/box width` | 142.0 | 140.0 | 141.6 | -1.6 | 1.0 px |
| `harness-native/root-selected/selected/slot:5/tile left` | 658.0 | 656.0 | 658.2 | -2.2 | 1.0 px |
| `harness-native/root-selected/selected/keys:quick-slot-5/cap 0 left` | 690.0 | 688.0 | 690.0 | -2.0 | 1.5 px |
| `harness-native/root-selected/selected/keys:quick-slot-5/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `harness-native/root-selected/selected/keys:quick-slot-5/cap 1 left` | 725.0 | 723.0 | 725.0 | -2.0 | 1.5 px |
| `harness-native/root-selected/selected/keys:quick-slot-5/cap 1 width` | 17.0 | 15.0 | 17.0 | -2.0 | 1.0 px |
| `harness-native/root-selected/selected/slot:5/title ink center` | 679.5 | 676.5 | 679.2 | -2.7 | 1.5 px |
| `harness-native/root-selected/selected/keys:quick-slots/cap 0 left` | 669.0 | 667.0 | 669.96 | -2.96 | 1.5 px |
| `harness-native/root-selected/selected/keys:quick-slots/cap 0 width` | 37.0 | 41.0 | 36.88 | 4.12 | 1.0 px |
| `harness-native/root-selected/selected/keys:quick-slots/cap 1 left` | 709.0 | 707.0 | 709.84 | -2.84 | 1.5 px |
| `harness-native/root-selected/selected/keys:quick-slots/cap 1 width` | 31.0 | 29.0 | 30.16 | -1.16 | 1.0 px |
| `parity/root-selected/selected/row:Figma/title ink left in row` | 51.0 | 55.0 | 51.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Figma/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Figma/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/row:Figma/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Figma/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/title ink left in row` | 51.0 | 55.0 | 50.0 | 5.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/tile/left in row` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/tile/width` | 28.0 | 26.0 | 28.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/row:Clipboard History/tile/glyph ink left` | 8.0 | 12.0 | 8.0 | 4.0 | 1.0 px |
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
| `parity/root-selected/selected/row:Left Half/tile/glyph ink left` | 7.0 | 11.0 | 7.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/row:Left Half/kind ink right in row` | 729.0 | 725.0 | 729.0 | -4.0 | 1.0 px |
| `parity/root-selected/selected/keys:win-alt-left/group width` | 86.0 | 80.5 | 85.62 | -5.12 | 1.0 px |
| `parity/root-selected/selected/keys:win-alt-left/cap 0 width` | 30.0 | 18.5 | 30.0 | -11.5 | 1.0 px |
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
| `parity/root-selected/selected/section:Pinned/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |
| `parity/root-selected/selected/section:Pinned/note ink right in label` | 724.0 | 720.0 | 725 | -5.0 | 1.5 px |
| `parity/root-selected/selected/section:Commands/title ink left in label` | 11.0 | 15.0 | 11 | 4.0 | 1.0 px |
| `parity/root-selected/selected/keys:quick-slots/group width` | 71.0 | 69.0 | 70.06 | -1.06 | 1.0 px |
| `parity/root-selected/selected/keys:quick-slots/cap 0 width` | 37.0 | 41.0 | 37.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/slot:1/box left` | 10.0 | 14.0 | 10.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/slot:1/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/slot:1/title ink center in slot` | 71.0 | 74.0 | 70.5 | 3.5 | 1.0 px |
| `parity/root-selected/selected/slot:1/tile/left in slot` | 50.0 | 53.0 | 50.0 | 3.0 | 1.0 px |
| `parity/root-selected/selected/slot:1/tile/width` | 42.0 | 40.0 | 42.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/keys:quick-slot-1/cap 1 width` | 17.0 | 21.0 | 17.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/keys:quick-slot-1/cap 1 label ink left` | 6.0 | 11.0 | 5.0 | 6.0 | 1.0 px |
| `parity/root-selected/selected/slot:2/box left` | 160.0 | 162.0 | 160.0 | 2.0 | 1.0 px |
| `parity/root-selected/selected/slot:2/tile/left in slot` | 49.4 | 51.4 | 49.41 | 1.99 | 1.0 px |
| `parity/root-selected/selected/slot:3/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/slot:4/title ink center in slot` | 70.7 | 68.7 | 70.7 | -2.0 | 1.0 px |
| `parity/root-selected/selected/slot:4/tile/left in slot` | 50.2 | 48.2 | 50.2 | -2.0 | 1.0 px |
| `parity/root-selected/selected/keys:quick-slot-4/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/slot:5/box left` | 608.0 | 606.0 | 608.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/slot:5/box width` | 142.0 | 140.0 | 142.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/slot:5/title ink center in slot` | 71.1 | 68.1 | 71.11 | -3.01 | 1.0 px |
| `parity/root-selected/selected/slot:5/tile/left in slot` | 49.6 | 47.6 | 49.61 | -2.01 | 1.0 px |
| `parity/root-selected/selected/keys:quick-slot-5/group width` | 52.0 | 50.0 | 52 | -2.0 | 1.0 px |
| `parity/root-selected/selected/keys:quick-slot-5/cap 0 width` | 32.0 | 36.0 | 32.0 | 4.0 | 1.0 px |
| `parity/root-selected/selected/keys:quick-slot-5/cap 1 width` | 17.0 | 15.0 | 17.0 | -2.0 | 1.0 px |
| `parity/root-selected/selected/keys:quick-slot-5/cap 1 label ink left` | 6.0 | 4.0 | 6.0 | -2.0 | 1.0 px |

## harness-native: 212 failed

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
| `root-rest/rest/keys:ctrl-shift-v/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-rest/rest/keys:win-alt-left/cap 0 left` | 555.5 | 554.4 | 1.1 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 0 top` | 353.0 | 346.0 | 7.0 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 0 width` | 18.5 | 29.8 | -11.3 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 1 left` | 585.0 | 587.2 | -2.2 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 1 width` | 34.0 | 29.8 | 4.2 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-rest/rest/row:Search Files/alias chip width` | 11.0 | 18.6 | -7.6 | 1.0 px |  |
| `root-rest/rest/row:Search Files/alias chip gap to what follows` | 16.0 | 12.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/slot:1/box left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/slot:1/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-rest/rest/slot:1/tile left` | 63.0 | 59.8 | 3.2 | 1.0 px |  |
| `root-rest/rest/slot:1/tile width` | 40.0 | 42.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-1/cap 0 left` | 94.0 | 91.6 | 2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-rest/rest/keys:quick-slot-1/cap 1 left` | 124.0 | 126.6 | -2.6 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-rest/rest/keys:quick-slot-1/cap 1 width` | 21.0 | 17.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/slot:1/title ink center` | 83.5 | 80.8 | 2.7 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-rest/rest/slot:2/box left` | 162.0 | 159.6 | 2.4 | 1.0 px |  |
| `root-rest/rest/slot:2/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-rest/rest/slot:2/tile left` | 211.0 | 209.4 | 1.6 | 1.0 px |  |
| `root-rest/rest/slot:3/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-rest/rest/slot:4/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-rest/rest/slot:4/tile left` | 507.0 | 508.6 | -1.6 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-4/cap 0 left` | 538.0 | 540.4 | -2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-rest/rest/keys:quick-slot-4/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-4/cap 1 left` | 573.0 | 575.4 | -2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-rest/rest/slot:4/title ink center` | 528.0 | 529.6 | -1.6 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-rest/rest/slot:5/box left` | 606.0 | 608.4 | -2.4 | 1.0 px |  |
| `root-rest/rest/slot:5/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-rest/rest/slot:5/tile left` | 656.0 | 658.2 | -2.2 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-5/cap 0 left` | 688.0 | 690.0 | -2.0 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-rest/rest/keys:quick-slot-5/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-5/cap 1 left` | 723.0 | 725.0 | -2.0 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-rest/rest/keys:quick-slot-5/cap 1 width` | 15.0 | 17.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/slot:5/title ink center` | 676.5 | 679.2 | -2.7 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-rest/rest/keys:quick-slots/cap 0 left` | 667.0 | 669.96 | -2.96 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-rest/rest/keys:quick-slots/cap 0 width` | 41.0 | 36.88 | 4.12 | 1.0 px |  |
| `root-rest/rest/keys:quick-slots/cap 1 left` | 707.0 | 709.84 | -2.84 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-rest/rest/keys:quick-slots/cap 1 width` | 29.0 | 30.16 | -1.16 | 1.0 px |  |
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
| `root-selected/rest/keys:ctrl-shift-v/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/rest/keys:win-alt-left/cap 0 left` | 555.5 | 554.4 | 1.1 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 0 top` | 353.0 | 346.0 | 7.0 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 0 width` | 18.5 | 29.8 | -11.3 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 1 left` | 585.0 | 587.2 | -2.2 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 1 width` | 34.0 | 29.8 | 4.2 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/rest/row:Search Files/alias chip width` | 11.0 | 18.6 | -7.6 | 1.0 px |  |
| `root-selected/rest/row:Search Files/alias chip gap to what follows` | 16.0 | 12.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/slot:1/box left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/slot:1/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/rest/slot:1/tile left` | 63.0 | 59.8 | 3.2 | 1.0 px |  |
| `root-selected/rest/slot:1/tile width` | 40.0 | 42.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-1/cap 0 left` | 94.0 | 91.6 | 2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/rest/keys:quick-slot-1/cap 1 left` | 124.0 | 126.6 | -2.6 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/rest/keys:quick-slot-1/cap 1 width` | 21.0 | 17.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/slot:1/title ink center` | 83.5 | 80.8 | 2.7 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-selected/rest/slot:2/box left` | 162.0 | 159.6 | 2.4 | 1.0 px |  |
| `root-selected/rest/slot:2/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/rest/slot:2/tile left` | 211.0 | 209.4 | 1.6 | 1.0 px |  |
| `root-selected/rest/slot:3/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/rest/slot:4/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/rest/slot:4/tile left` | 507.0 | 508.6 | -1.6 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-4/cap 0 left` | 538.0 | 540.4 | -2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/rest/keys:quick-slot-4/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-4/cap 1 left` | 573.0 | 575.4 | -2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/rest/slot:4/title ink center` | 528.0 | 529.6 | -1.6 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-selected/rest/slot:5/box left` | 606.0 | 608.4 | -2.4 | 1.0 px |  |
| `root-selected/rest/slot:5/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/rest/slot:5/tile left` | 656.0 | 658.2 | -2.2 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-5/cap 0 left` | 688.0 | 690.0 | -2.0 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/rest/keys:quick-slot-5/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-5/cap 1 left` | 723.0 | 725.0 | -2.0 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/rest/keys:quick-slot-5/cap 1 width` | 15.0 | 17.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/slot:5/title ink center` | 676.5 | 679.2 | -2.7 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-selected/rest/keys:quick-slots/cap 0 left` | 667.0 | 669.96 | -2.96 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/rest/keys:quick-slots/cap 0 width` | 41.0 | 36.88 | 4.12 | 1.0 px |  |
| `root-selected/rest/keys:quick-slots/cap 1 left` | 707.0 | 709.84 | -2.84 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/rest/keys:quick-slots/cap 1 width` | 29.0 | 30.16 | -1.16 | 1.0 px |  |
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
| `root-selected/down-1/keys:ctrl-shift-v/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/down-1/keys:win-alt-left/cap 0 left` | 555.5 | 554.4 | 1.1 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 0 top` | 353.0 | 346.0 | 7.0 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 0 width` | 18.5 | 29.8 | -11.3 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 1 left` | 585.0 | 587.2 | -2.2 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 1 width` | 34.0 | 29.8 | 4.2 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/down-1/row:Search Files/alias chip width` | 11.0 | 18.6 | -7.6 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/alias chip gap to what follows` | 16.0 | 12.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/slot:1/box left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/slot:1/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/down-1/slot:1/tile left` | 63.0 | 59.8 | 3.2 | 1.0 px |  |
| `root-selected/down-1/slot:1/tile width` | 40.0 | 42.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-1/cap 0 left` | 94.0 | 91.6 | 2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/down-1/keys:quick-slot-1/cap 1 left` | 124.0 | 126.6 | -2.6 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/down-1/keys:quick-slot-1/cap 1 width` | 21.0 | 17.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/slot:1/title ink center` | 83.5 | 80.8 | 2.7 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-selected/down-1/slot:2/box left` | 162.0 | 159.6 | 2.4 | 1.0 px |  |
| `root-selected/down-1/slot:2/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/down-1/slot:2/tile left` | 211.0 | 209.4 | 1.6 | 1.0 px |  |
| `root-selected/down-1/slot:3/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/down-1/slot:4/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/down-1/slot:4/tile left` | 507.0 | 508.6 | -1.6 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-4/cap 0 left` | 538.0 | 540.4 | -2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/down-1/keys:quick-slot-4/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-4/cap 1 left` | 573.0 | 575.4 | -2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/down-1/slot:4/title ink center` | 528.0 | 529.6 | -1.6 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-selected/down-1/slot:5/box left` | 606.0 | 608.4 | -2.4 | 1.0 px |  |
| `root-selected/down-1/slot:5/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/down-1/slot:5/tile left` | 656.0 | 658.2 | -2.2 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-5/cap 0 left` | 688.0 | 690.0 | -2.0 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/down-1/keys:quick-slot-5/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-5/cap 1 left` | 723.0 | 725.0 | -2.0 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/down-1/keys:quick-slot-5/cap 1 width` | 15.0 | 17.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/slot:5/title ink center` | 676.5 | 679.2 | -2.7 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-selected/down-1/keys:quick-slots/cap 0 left` | 667.0 | 669.96 | -2.96 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/down-1/keys:quick-slots/cap 0 width` | 41.0 | 36.88 | 4.12 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slots/cap 1 left` | 707.0 | 709.84 | -2.84 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/down-1/keys:quick-slots/cap 1 width` | 29.0 | 30.16 | -1.16 | 1.0 px |  |
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
| `root-selected/selected/keys:ctrl-shift-v/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/selected/keys:win-alt-left/cap 0 left` | 555.5 | 554.4 | 1.1 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 0 top` | 353.0 | 346.0 | 7.0 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 0 width` | 18.5 | 29.8 | -11.3 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 1 left` | 585.0 | 587.2 | -2.2 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 1 width` | 34.0 | 29.8 | 4.2 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 2 left` | 618.0 | 620.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/kind ink right` | 735.0 | 740.0 | -5.0 | 1.5 px | right-aligned at the row's padding; a glyph's side bearing allowed |
| `root-selected/selected/row:Search Files/alias chip width` | 11.0 | 18.6 | -7.6 | 1.0 px |  |
| `root-selected/selected/row:Search Files/alias chip gap to what follows` | 16.0 | 12.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/slot:1/box left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/slot:1/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/selected/slot:1/tile left` | 63.0 | 59.8 | 3.2 | 1.0 px |  |
| `root-selected/selected/slot:1/tile width` | 40.0 | 42.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-1/cap 0 left` | 94.0 | 91.6 | 2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/selected/keys:quick-slot-1/cap 1 left` | 124.0 | 126.6 | -2.6 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/selected/keys:quick-slot-1/cap 1 width` | 21.0 | 17.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/slot:1/title ink center` | 83.5 | 80.8 | 2.7 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-selected/selected/slot:2/box left` | 162.0 | 159.6 | 2.4 | 1.0 px |  |
| `root-selected/selected/slot:2/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/selected/slot:2/tile left` | 211.0 | 209.4 | 1.6 | 1.0 px |  |
| `root-selected/selected/slot:3/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/selected/slot:4/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/selected/slot:4/tile left` | 507.0 | 508.6 | -1.6 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-4/cap 0 left` | 538.0 | 540.4 | -2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/selected/keys:quick-slot-4/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-4/cap 1 left` | 573.0 | 575.4 | -2.4 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/selected/slot:4/title ink center` | 528.0 | 529.6 | -1.6 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-selected/selected/slot:5/box left` | 606.0 | 608.4 | -2.4 | 1.0 px |  |
| `root-selected/selected/slot:5/box width` | 140.0 | 141.6 | -1.6 | 1.0 px |  |
| `root-selected/selected/slot:5/tile left` | 656.0 | 658.2 | -2.2 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-5/cap 0 left` | 688.0 | 690.0 | -2.0 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/selected/keys:quick-slot-5/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-5/cap 1 left` | 723.0 | 725.0 | -2.0 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/selected/keys:quick-slot-5/cap 1 width` | 15.0 | 17.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/slot:5/title ink center` | 676.5 | 679.2 | -2.7 | 1.5 px | centered across the slot; a glyph's side bearing allowed |
| `root-selected/selected/keys:quick-slots/cap 0 left` | 667.0 | 669.96 | -2.96 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/selected/keys:quick-slots/cap 0 width` | 41.0 | 36.88 | 4.12 | 1.0 px |  |
| `root-selected/selected/keys:quick-slots/cap 1 left` | 707.0 | 709.84 | -2.84 | 1.5 px | declared through a chain of shaped widths, which layout rounds part by part |
| `root-selected/selected/keys:quick-slots/cap 1 width` | 29.0 | 30.16 | -1.16 | 1.0 px |  |

## harness-reference: 0 failed


## parity: 266 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `root-rest/rest/row:Figma/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Figma/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/title ink left in row` | 55.0 | 50.0 | 5.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Clipboard History/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
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
| `root-rest/rest/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/tile/glyph ink left` | 11.0 | 7.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/group width` | 80.5 | 85.62 | -5.12 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 0 width` | 18.5 | 30.0 | -11.5 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 0 label ink left` | 0.5 | 6.0 | -5.5 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 1 width` | 34.0 | 30.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/alias chip left in row` | 615.0 | 611.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/alias chip width` | 11.0 | 19.0 | -8.0 | 1.0 px |  |
| `root-rest/rest/section:Pinned/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |
| `root-rest/rest/section:Pinned/note ink right in label` | 720.0 | 725 | -5.0 | 1.5 px | right-aligned; a glyph's side bearing allowed |
| `root-rest/rest/section:Commands/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |
| `root-rest/rest/keys:quick-slots/group width` | 69.0 | 70.06 | -1.06 | 1.0 px |  |
| `root-rest/rest/keys:quick-slots/cap 0 width` | 41.0 | 37.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/slot:1/box left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/slot:1/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/slot:1/title ink center in slot` | 74.0 | 70.5 | 3.5 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-rest/rest/slot:1/tile/left in slot` | 53.0 | 50.0 | 3.0 | 1.0 px |  |
| `root-rest/rest/slot:1/tile/width` | 40.0 | 42.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-1/cap 1 width` | 21.0 | 17.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-1/cap 1 label ink left` | 11.0 | 5.0 | 6.0 | 1.0 px |  |
| `root-rest/rest/slot:2/box left` | 162.0 | 160.0 | 2.0 | 1.0 px |  |
| `root-rest/rest/slot:2/tile/left in slot` | 51.4 | 49.41 | 1.99 | 1.0 px |  |
| `root-rest/rest/slot:3/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/slot:4/title ink center in slot` | 68.7 | 70.7 | -2.0 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-rest/rest/slot:4/tile/left in slot` | 48.2 | 50.2 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-4/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/slot:5/box left` | 606.0 | 608.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/slot:5/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/slot:5/title ink center in slot` | 68.1 | 71.11 | -3.01 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-rest/rest/slot:5/tile/left in slot` | 47.6 | 49.61 | -2.01 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-5/group width` | 50.0 | 52 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-5/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-5/cap 1 width` | 15.0 | 17.0 | -2.0 | 1.0 px |  |
| `root-rest/rest/keys:quick-slot-5/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Figma/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/title ink left in row` | 55.0 | 50.0 | 5.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Clipboard History/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
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
| `root-selected/rest/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/tile/glyph ink left` | 11.0 | 7.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/group width` | 80.5 | 85.62 | -5.12 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 0 width` | 18.5 | 30.0 | -11.5 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 0 label ink left` | 0.5 | 6.0 | -5.5 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 1 width` | 34.0 | 30.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/alias chip left in row` | 615.0 | 611.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/alias chip width` | 11.0 | 19.0 | -8.0 | 1.0 px |  |
| `root-selected/rest/section:Pinned/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |
| `root-selected/rest/section:Pinned/note ink right in label` | 720.0 | 725 | -5.0 | 1.5 px | right-aligned; a glyph's side bearing allowed |
| `root-selected/rest/section:Commands/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |
| `root-selected/rest/keys:quick-slots/group width` | 69.0 | 70.06 | -1.06 | 1.0 px |  |
| `root-selected/rest/keys:quick-slots/cap 0 width` | 41.0 | 37.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/slot:1/box left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/slot:1/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/slot:1/title ink center in slot` | 74.0 | 70.5 | 3.5 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-selected/rest/slot:1/tile/left in slot` | 53.0 | 50.0 | 3.0 | 1.0 px |  |
| `root-selected/rest/slot:1/tile/width` | 40.0 | 42.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-1/cap 1 width` | 21.0 | 17.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-1/cap 1 label ink left` | 11.0 | 5.0 | 6.0 | 1.0 px |  |
| `root-selected/rest/slot:2/box left` | 162.0 | 160.0 | 2.0 | 1.0 px |  |
| `root-selected/rest/slot:2/tile/left in slot` | 51.4 | 49.41 | 1.99 | 1.0 px |  |
| `root-selected/rest/slot:3/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/slot:4/title ink center in slot` | 68.7 | 70.7 | -2.0 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-selected/rest/slot:4/tile/left in slot` | 48.2 | 50.2 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-4/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/slot:5/box left` | 606.0 | 608.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/slot:5/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/slot:5/title ink center in slot` | 68.1 | 71.11 | -3.01 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-selected/rest/slot:5/tile/left in slot` | 47.6 | 49.61 | -2.01 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-5/group width` | 50.0 | 52 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-5/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-5/cap 1 width` | 15.0 | 17.0 | -2.0 | 1.0 px |  |
| `root-selected/rest/keys:quick-slot-5/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Figma/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/wash left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/wash width` | 732.0 | 740.0 | -8.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/tile/fill alpha` | 34.25 | 20.05 | 14.2 | 2.0 levels |  |
| `root-selected/down-1/row:Clipboard History/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/alias chip left in row` | 489.0 | 487.0 | 2.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/alias chip top in row` | 20.0 | 13.0 | 7.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/alias chip width` | 12.0 | 26.0 | -14.0 | 1.0 px |  |
| `root-selected/down-1/row:Clipboard History/alias chip height` | 6.0 | 18.0 | -12.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/group width` | 104.0 | 105.41 | -1.41 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 0 width` | 41.0 | 36.0 | 5.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 1 width` | 47.0 | 43.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:ctrl-shift-v/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/tile/glyph ink left` | 11.0 | 7.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/group width` | 80.5 | 85.62 | -5.12 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 0 width` | 18.5 | 30.0 | -11.5 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 0 label ink left` | 0.5 | 6.0 | -5.5 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 1 width` | 34.0 | 30.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/alias chip left in row` | 615.0 | 611.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/alias chip width` | 11.0 | 19.0 | -8.0 | 1.0 px |  |
| `root-selected/down-1/section:Pinned/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |
| `root-selected/down-1/section:Pinned/note ink right in label` | 720.0 | 725 | -5.0 | 1.5 px | right-aligned; a glyph's side bearing allowed |
| `root-selected/down-1/section:Commands/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slots/group width` | 69.0 | 70.06 | -1.06 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slots/cap 0 width` | 41.0 | 37.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/slot:1/box left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/slot:1/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/slot:1/title ink center in slot` | 74.0 | 70.5 | 3.5 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-selected/down-1/slot:1/tile/left in slot` | 53.0 | 50.0 | 3.0 | 1.0 px |  |
| `root-selected/down-1/slot:1/tile/width` | 40.0 | 42.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-1/cap 1 width` | 21.0 | 17.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-1/cap 1 label ink left` | 11.0 | 5.0 | 6.0 | 1.0 px |  |
| `root-selected/down-1/slot:2/box left` | 162.0 | 160.0 | 2.0 | 1.0 px |  |
| `root-selected/down-1/slot:2/tile/left in slot` | 51.4 | 49.41 | 1.99 | 1.0 px |  |
| `root-selected/down-1/slot:3/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/slot:4/title ink center in slot` | 68.7 | 70.7 | -2.0 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-selected/down-1/slot:4/tile/left in slot` | 48.2 | 50.2 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-4/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/slot:5/box left` | 606.0 | 608.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/slot:5/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/slot:5/title ink center in slot` | 68.1 | 71.11 | -3.01 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-selected/down-1/slot:5/tile/left in slot` | 47.6 | 49.61 | -2.01 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-5/group width` | 50.0 | 52 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-5/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-5/cap 1 width` | 15.0 | 17.0 | -2.0 | 1.0 px |  |
| `root-selected/down-1/keys:quick-slot-5/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Figma/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/title ink left in row` | 55.0 | 50.0 | 5.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Clipboard History/tile/glyph ink left` | 12.0 | 8.0 | 4.0 | 1.0 px |  |
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
| `root-selected/selected/row:Left Half/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/tile/fill alpha` | 34.25 | 20.05 | 14.2 | 2.0 levels |  |
| `root-selected/selected/row:Left Half/tile/glyph ink left` | 11.0 | 7.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/group width` | 80.5 | 85.62 | -5.12 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 0 width` | 18.5 | 30.0 | -11.5 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 0 label ink left` | 0.5 | 6.0 | -5.5 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 1 width` | 34.0 | 30.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:win-alt-left/cap 2 width` | 18.0 | 20.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/title ink left in row` | 55.0 | 51.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/tile/left in row` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/tile/width` | 26.0 | 28.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/tile/glyph ink left` | 13.0 | 9.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/kind ink right in row` | 725.0 | 729.0 | -4.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/alias chip left in row` | 615.0 | 611.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/alias chip width` | 11.0 | 19.0 | -8.0 | 1.0 px |  |
| `root-selected/selected/section:Pinned/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |
| `root-selected/selected/section:Pinned/note ink right in label` | 720.0 | 725 | -5.0 | 1.5 px | right-aligned; a glyph's side bearing allowed |
| `root-selected/selected/section:Commands/title ink left in label` | 15.0 | 11 | 4.0 | 1.0 px |  |
| `root-selected/selected/keys:quick-slots/group width` | 69.0 | 70.06 | -1.06 | 1.0 px |  |
| `root-selected/selected/keys:quick-slots/cap 0 width` | 41.0 | 37.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/slot:1/box left` | 14.0 | 10.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/slot:1/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/slot:1/title ink center in slot` | 74.0 | 70.5 | 3.5 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-selected/selected/slot:1/tile/left in slot` | 53.0 | 50.0 | 3.0 | 1.0 px |  |
| `root-selected/selected/slot:1/tile/width` | 40.0 | 42.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-1/cap 1 width` | 21.0 | 17.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-1/cap 1 label ink left` | 11.0 | 5.0 | 6.0 | 1.0 px |  |
| `root-selected/selected/slot:2/box left` | 162.0 | 160.0 | 2.0 | 1.0 px |  |
| `root-selected/selected/slot:2/tile/left in slot` | 51.4 | 49.41 | 1.99 | 1.0 px |  |
| `root-selected/selected/slot:3/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/slot:4/title ink center in slot` | 68.7 | 70.7 | -2.0 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-selected/selected/slot:4/tile/left in slot` | 48.2 | 50.2 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-4/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/slot:5/box left` | 606.0 | 608.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/slot:5/box width` | 140.0 | 142.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/slot:5/title ink center in slot` | 68.1 | 71.11 | -3.01 | 1.0 px | the extent's center by coverage (a pixel's worth of ink per column) |
| `root-selected/selected/slot:5/tile/left in slot` | 47.6 | 49.61 | -2.01 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-5/group width` | 50.0 | 52 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-5/cap 0 width` | 36.0 | 32.0 | 4.0 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-5/cap 1 width` | 15.0 | 17.0 | -2.0 | 1.0 px |  |
| `root-selected/selected/keys:quick-slot-5/cap 1 label ink left` | 4.0 | 6.0 | -2.0 | 1.0 px |  |

## Accepted discrepancies: 20 (failing, with a recorded disposition)

| Check | Native/measured | Reference/expected | Delta | Limit | Disposition |
|---|---|---|---|---|---|
| `root-rest/rest/keys:win-alt-left/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-rest/rest/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-rest/rest/sections/labels` | Pinned | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; both sides show the pinned strip's "Pinned" label (#101) |
| `root-rest/rest/slot:2/tile/glyph core pixels (stroke weight)` | 40.0 | 0 | 40.0 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-rest/rest/slot:3/tile/glyph core pixels (stroke weight)` | 25.35 | 0 | 25.35 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/rest/keys:win-alt-left/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/rest/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/rest/sections/labels` | Pinned | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; both sides show the pinned strip's "Pinned" label (#101) |
| `root-selected/rest/slot:2/tile/glyph core pixels (stroke weight)` | 40.0 | 0 | 40.0 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/rest/slot:3/tile/glyph core pixels (stroke weight)` | 25.35 | 0 | 25.35 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/down-1/keys:win-alt-left/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/down-1/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/down-1/sections/labels` | Pinned | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; both sides show the pinned strip's "Pinned" label (#101) |
| `root-selected/down-1/slot:2/tile/glyph core pixels (stroke weight)` | 40.0 | 0 | 40.0 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/down-1/slot:3/tile/glyph core pixels (stroke weight)` | 25.35 | 0 | 25.35 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/selected/keys:win-alt-left/cap 2 label ink left` | 4.0 | 7.0 | -3.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/selected/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `root-selected/selected/sections/labels` | Pinned | Commands | Pinned | Suggested | Commands | None | 0  | accepted (#94, #100): Pane labels a blank query's rows "Commands" and claims no recent use, so the reference's "Suggested · From your recent use" is not shown; both sides show the pinned strip's "Pinned" label (#101) |
| `root-selected/selected/slot:2/tile/glyph core pixels (stroke weight)` | 40.0 | 0 | 40.0 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
| `root-selected/selected/slot:3/tile/glyph core pixels (stroke weight)` | 25.35 | 0 | 25.35 | 25 % | accepted (#93, #101): the same 2px stroke at the same place; GPUI draws an SVG icon through its text's contrast and gamma correction, which lifts the stroke's anti-aliased edge pixels past the core threshold (e.g. Firefox's globe, one column: 232/208 native against 211/194 reference at the edges, 255 at the center on both), so the native glyph counts more core pixels. Only a heavier native glyph is accepted; a lighter one (a 1.6 stroke) still fails |
