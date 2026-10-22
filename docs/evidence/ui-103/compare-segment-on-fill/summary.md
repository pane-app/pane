# Visual workbench comparison: segment-on-fill

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `appearance-page`: 96 DPI, client [1120, 720], opaque, perturbation segment-on-fill

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 135 | 0 | 0 |
| harness-native | 259 | 4 | 0 |
| parity | 458 | 4 | 4 |

## Sensitivity: passed in the baseline, failing now

| Check | Baseline | Now | Expected | Delta | Limit |
|---|---|---|---|---|---|
| `harness-native/appearance-page/rest/segment:Glass/segment on wash alpha (chosen)` | 31.2 | 77.11 | 31.0 | 46.11 | 2.1448090993736315 levels |
| `harness-native/appearance-page/rest/segment:Default/segment on wash alpha (chosen)` | 31.2 | 77.11 | 31.0 | 46.11 | 2.1448090993736315 levels |
| `parity/appearance-page/rest/segment:Glass/segment wash alpha` | 31.2 | 77.11 | 30.82 | 46.29 | 2.1633572249980593 levels |
| `parity/appearance-page/rest/segment:Default/segment wash alpha` | 31.2 | 77.11 | 31.02 | 46.09 | 2.2175404259069014 levels |
| `harness-native/appearance-page/segment-hover/segment:Glass/segment on wash alpha (chosen)` | 31.2 | 77.11 | 31.0 | 46.11 | 2.1448090993736315 levels |
| `harness-native/appearance-page/segment-hover/segment:Default/segment on wash alpha (chosen)` | 31.2 | 77.11 | 31.0 | 46.11 | 2.1448090993736315 levels |
| `parity/appearance-page/segment-hover/segment:Glass/segment wash alpha` | 31.2 | 77.11 | 30.82 | 46.29 | 2.1633572249980593 levels |
| `parity/appearance-page/segment-hover/segment:Default/segment wash alpha` | 31.2 | 77.11 | 31.02 | 46.09 | 2.2175404259069014 levels |

## harness-native: 4 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `appearance-page/rest/segment:Glass/segment on wash alpha (chosen)` | 77.11 | 31.0 | 46.11 | 2.1448090993736315 levels | over the track's fill; 2 channel levels over a background of 17: 2.1 alpha levels |
| `appearance-page/rest/segment:Default/segment on wash alpha (chosen)` | 77.11 | 31.0 | 46.11 | 2.1448090993736315 levels | over the track's fill; 2 channel levels over a background of 17: 2.1 alpha levels |
| `appearance-page/segment-hover/segment:Glass/segment on wash alpha (chosen)` | 77.11 | 31.0 | 46.11 | 2.1448090993736315 levels | over the track's fill; 2 channel levels over a background of 17: 2.1 alpha levels |
| `appearance-page/segment-hover/segment:Default/segment on wash alpha (chosen)` | 77.11 | 31.0 | 46.11 | 2.1448090993736315 levels | over the track's fill; 2 channel levels over a background of 17: 2.1 alpha levels |

## harness-reference: 0 failed


## parity: 4 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `appearance-page/rest/segment:Glass/segment wash alpha` | 77.11 | 30.82 | 46.29 | 2.1633572249980593 levels | 2 channel levels over a background of 19: 2.2 alpha levels |
| `appearance-page/rest/segment:Default/segment wash alpha` | 77.11 | 31.02 | 46.09 | 2.2175404259069014 levels | 2 channel levels over a background of 25: 2.2 alpha levels |
| `appearance-page/segment-hover/segment:Glass/segment wash alpha` | 77.11 | 30.82 | 46.29 | 2.1633572249980593 levels | 2 channel levels over a background of 19: 2.2 alpha levels |
| `appearance-page/segment-hover/segment:Default/segment wash alpha` | 77.11 | 31.02 | 46.09 | 2.2175404259069014 levels | 2 channel levels over a background of 25: 2.2 alpha levels |

## Accepted discrepancies: 4 (failing, with a recorded disposition)

| Check | Native/measured | Reference/expected | Delta | Limit | Disposition |
|---|---|---|---|---|---|
| `appearance-page/rest/sidebar/fill alpha` | 24.93 | 28.04 | -3.11 | 2.0 levels | accepted (#97): both draw the sidebar as black 10% (25.5 levels). Over the page's ~22 levels that darkens it by about 2 levels, so one level of rounding is about 11 alpha levels. Pane reads its 25.5 within the flat-fill limit; Chrome's glass composites the same overlay 0.2-0.3 of a level darker, and reads about 28. Accepted only while Pane matches its declaration and the two differ by less than one level of the page |
| `appearance-page/rest/nav:General/glyph ink height in item` | 10.0 | 8.0 | 2.0 | 1.0 px | accepted (#97): the same 16px glyph at the same center. Its 1.6 stroke is about 1.07px here, and a vertical stroke's round cap covers its end row only partly; GPUI's sprite contrast (see SPRITE_CONTRAST) lifts that row past a pixel's worth of coverage (0.64 a pixel against Chrome's 0.5 on General's sliders), so the native box reads an edge row longer at an end. Accepted only where the native box is larger, by at most 2px, around a center within 1px of the reference's |
| `appearance-page/segment-hover/sidebar/fill alpha` | 24.93 | 28.04 | -3.11 | 2.0 levels | accepted (#97): both draw the sidebar as black 10% (25.5 levels). Over the page's ~22 levels that darkens it by about 2 levels, so one level of rounding is about 11 alpha levels. Pane reads its 25.5 within the flat-fill limit; Chrome's glass composites the same overlay 0.2-0.3 of a level darker, and reads about 28. Accepted only while Pane matches its declaration and the two differ by less than one level of the page |
| `appearance-page/segment-hover/nav:General/glyph ink height in item` | 10.0 | 8.0 | 2.0 | 1.0 px | accepted (#97): the same 16px glyph at the same center. Its 1.6 stroke is about 1.07px here, and a vertical stroke's round cap covers its end row only partly; GPUI's sprite contrast (see SPRITE_CONTRAST) lifts that row past a pixel's worth of coverage (0.64 a pixel against Chrome's 0.5 on General's sliders), so the native box reads an edge row longer at an end. Accepted only where the native box is larger, by at most 2px, around a center within 1px of the reference's |
