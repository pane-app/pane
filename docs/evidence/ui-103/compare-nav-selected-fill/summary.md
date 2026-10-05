# Visual workbench comparison: nav-selected-fill

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `settings-shell`: 96 DPI, client [1120, 720], opaque, perturbation nav-selected-fill

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 116 | 0 | 0 |
| harness-native | 131 | 3 | 0 |
| parity | 295 | 3 | 6 |

## Sensitivity: passed in the baseline, failing now

| Check | Baseline | Now | Expected | Delta | Limit |
|---|---|---|---|---|---|
| `harness-native/settings-shell/rest/nav:Appearance/nav wash alpha (selected)` | 23.02 | 63.93 | 23 | 40.93 | 2.0 levels |
| `parity/settings-shell/rest/nav:Appearance/nav wash alpha` | 23.02 | 63.93 | 23.06 | 40.87 | 2.0 levels |
| `harness-native/settings-shell/hover/nav:Appearance/nav wash alpha (selected)` | 23.02 | 63.93 | 23 | 40.93 | 2.0 levels |
| `parity/settings-shell/hover/nav:Appearance/nav wash alpha` | 23.02 | 63.93 | 23.06 | 40.87 | 2.0 levels |
| `harness-native/settings-shell/selected-hover/nav:Appearance/nav wash alpha (selected)` | 23.02 | 63.93 | 23 | 40.93 | 2.0 levels |
| `parity/settings-shell/selected-hover/nav:Appearance/nav wash alpha` | 23.02 | 63.93 | 23.06 | 40.87 | 2.0 levels |

## harness-native: 3 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `settings-shell/rest/nav:Appearance/nav wash alpha (selected)` | 63.93 | 23 | 40.93 | 2.0 levels |  |
| `settings-shell/hover/nav:Appearance/nav wash alpha (selected)` | 63.93 | 23 | 40.93 | 2.0 levels |  |
| `settings-shell/selected-hover/nav:Appearance/nav wash alpha (selected)` | 63.93 | 23 | 40.93 | 2.0 levels |  |

## harness-reference: 0 failed


## parity: 3 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `settings-shell/rest/nav:Appearance/nav wash alpha` | 63.93 | 23.06 | 40.87 | 2.0 levels |  |
| `settings-shell/hover/nav:Appearance/nav wash alpha` | 63.93 | 23.06 | 40.87 | 2.0 levels |  |
| `settings-shell/selected-hover/nav:Appearance/nav wash alpha` | 63.93 | 23.06 | 40.87 | 2.0 levels |  |

## Accepted discrepancies: 6 (failing, with a recorded disposition)

| Check | Native/measured | Reference/expected | Delta | Limit | Disposition |
|---|---|---|---|---|---|
| `settings-shell/rest/sidebar/fill alpha` | 24.93 | 28.04 | -3.11 | 2.0 levels | accepted (#97): both draw the sidebar as black 10% (25.5 levels). Over the page's ~22 levels that darkens it by about 2 levels, so one level of rounding is about 11 alpha levels. Pane reads its 25.5 within the flat-fill limit; Chrome's glass composites the same overlay 0.2-0.3 of a level darker, and reads about 28. Accepted only while Pane matches its declaration and the two differ by less than one level of the page |
| `settings-shell/rest/nav:General/glyph ink height in item` | 10.0 | 8.0 | 2.0 | 1.0 px | accepted (#97): the same 16px glyph at the same center. Its 1.6 stroke is about 1.07px here, and a vertical stroke's round cap covers its end row only partly; GPUI's sprite contrast (see SPRITE_CONTRAST) lifts that row past a pixel's worth of coverage (0.64 a pixel against Chrome's 0.5 on General's sliders), so the native box reads an edge row longer at an end. Accepted only where the native box is larger, by at most 2px, around a center within 1px of the reference's |
| `settings-shell/hover/sidebar/fill alpha` | 24.93 | 28.04 | -3.11 | 2.0 levels | accepted (#97): both draw the sidebar as black 10% (25.5 levels). Over the page's ~22 levels that darkens it by about 2 levels, so one level of rounding is about 11 alpha levels. Pane reads its 25.5 within the flat-fill limit; Chrome's glass composites the same overlay 0.2-0.3 of a level darker, and reads about 28. Accepted only while Pane matches its declaration and the two differ by less than one level of the page |
| `settings-shell/hover/nav:General/glyph ink height in item` | 10.0 | 8.0 | 2.0 | 1.0 px | accepted (#97): the same 16px glyph at the same center. Its 1.6 stroke is about 1.07px here, and a vertical stroke's round cap covers its end row only partly; GPUI's sprite contrast (see SPRITE_CONTRAST) lifts that row past a pixel's worth of coverage (0.64 a pixel against Chrome's 0.5 on General's sliders), so the native box reads an edge row longer at an end. Accepted only where the native box is larger, by at most 2px, around a center within 1px of the reference's |
| `settings-shell/selected-hover/sidebar/fill alpha` | 24.93 | 28.04 | -3.11 | 2.0 levels | accepted (#97): both draw the sidebar as black 10% (25.5 levels). Over the page's ~22 levels that darkens it by about 2 levels, so one level of rounding is about 11 alpha levels. Pane reads its 25.5 within the flat-fill limit; Chrome's glass composites the same overlay 0.2-0.3 of a level darker, and reads about 28. Accepted only while Pane matches its declaration and the two differ by less than one level of the page |
| `settings-shell/selected-hover/nav:General/glyph ink height in item` | 10.0 | 8.0 | 2.0 | 1.0 px | accepted (#97): the same 16px glyph at the same center. Its 1.6 stroke is about 1.07px here, and a vertical stroke's round cap covers its end row only partly; GPUI's sprite contrast (see SPRITE_CONTRAST) lifts that row past a pixel's worth of coverage (0.64 a pixel against Chrome's 0.5 on General's sliders), so the native box reads an edge row longer at an end. Accepted only where the native box is larger, by at most 2px, around a center within 1px of the reference's |
