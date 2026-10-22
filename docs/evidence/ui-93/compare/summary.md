# Visual workbench comparison: baseline

Reference SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`, Chrome/154.0.8037.93.
Limits: {'edge_px': 1.0, 'flat_fill_levels': 2.0, 'glyph_core_levels': 4.0}.

- `keycap-windows`: 96 DPI, client [760, 518], opaque, perturbation None
- `launcher-frame`: 96 DPI, client [760, 518], opaque, perturbation None
- `launcher-frame-light`: 96 DPI, client [760, 518], opaque, perturbation None
- `launcher-frame-narrow`: 96 DPI, client [480, 360], opaque, perturbation None
- `root-focus`: 96 DPI, client [760, 518], opaque, perturbation None
- `root-hover`: 96 DPI, client [760, 518], opaque, perturbation None
- `root-long-content`: 96 DPI, client [760, 518], opaque, perturbation None
- `root-rest`: 96 DPI, client [760, 518], opaque, perturbation None
- `root-selected`: 96 DPI, client [760, 518], opaque, perturbation None
- `root-selected-hover`: 96 DPI, client [760, 518], opaque, perturbation None
- `root-unavailable`: 96 DPI, client [760, 518], opaque, perturbation None
- `tile-sizes`: 96 DPI, client [760, 518], opaque, perturbation None

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 125 | 0 | 0 |
| harness-native | 618 | 0 | 0 |
| parity | 1042 | 109 | 2 |

## harness-native: 0 failed


## harness-reference: 0 failed


## parity: 109 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `launcher-frame/frame/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `launcher-frame/frame/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `launcher-frame/frame/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `launcher-frame/frame/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `launcher-frame/frame/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `launcher-frame/frame/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `launcher-frame/frame/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `launcher-frame/frame/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `launcher-frame/frame/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-focus/rest/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-focus/rest/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-focus/rest/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-focus/rest/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-focus/rest/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-focus/rest/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-focus/rest/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-focus/rest/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-focus/rest/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-focus/focus-typed/row:Clipboard History/top in client` | 68.0 | 100 | -32.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-focus/focus-typed/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-focus/focus-typed/row:Clipboard History/title match highlighted` | no | yes | None | 0  | the reference paints the matched part of a title in the accent |
| `root-focus/focus-typed/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-focus/focus-typed/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-focus/back-to-rest/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-focus/back-to-rest/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-focus/back-to-rest/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-focus/back-to-rest/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-focus/back-to-rest/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-focus/back-to-rest/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-focus/back-to-rest/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-focus/back-to-rest/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-focus/back-to-rest/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-hover/rest/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/rest/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/rest/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-hover/rest/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/rest/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-hover/rest/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/rest/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-hover/rest/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-hover/rest/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-hover/hover/selection/selected rows` | Figma | Clipboard History | None | 0  | the reference selects the row the pointer moves over; production only washes it |
| `root-hover/hover/selection/hover-only rows` | Clipboard History |  | None | 0  |  |
| `root-hover/hover/row:Figma/wash alpha` | 21.65 | 0.0 | 21.65 | 2.0 levels |  |
| `root-hover/hover/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/hover/row:Clipboard History/wash alpha` | 8.98 | 21.96 | -12.98 | 2.0 levels |  |
| `root-hover/hover/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/hover/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-hover/hover/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/hover/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-hover/hover/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-hover/hover/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-hover/hover/footer-primary/label` | Open Application | Run Command | None | 0  |  |
| `root-hover/hover/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-hover/hover/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-rest/rest/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-rest/rest/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-rest/rest/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-rest/rest/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-rest/rest/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-rest/rest/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-selected/rest/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/rest/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/rest/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/rest/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-selected/rest/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-selected/rest/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-selected/down-1/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/down-1/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/down-1/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/down-1/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-selected/down-1/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-selected/down-1/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-selected/selected/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/selected/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected/selected/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected/selected/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-selected/selected/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-selected/selected/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-selected-hover/selected/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected-hover/selected/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected-hover/selected/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected-hover/selected/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected-hover/selected/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected-hover/selected/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected-hover/selected/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-selected-hover/selected/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-selected-hover/selected/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |
| `root-selected-hover/selected-hover/row:Figma/top in client` | 68.0 | 242 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected-hover/selected-hover/row:Clipboard History/top in client` | 114.0 | 288 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected-hover/selected-hover/row:Clipboard History/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected-hover/selected-hover/row:Left Half/top in client` | 160.0 | 334 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected-hover/selected-hover/row:Left Half/title-to-subtitle ink gap` | 8.0 | 13.0 | -5.0 | 1.0 px |  |
| `root-selected-hover/selected-hover/row:Search Files/top in client` | 206.0 | 380 | -174.0 | 1.0 px | the reference lists the pinned strip and section labels above the rows (#101) |
| `root-selected-hover/selected-hover/row:Search Files/title-to-subtitle ink gap` | 8.0 | 14.0 | -6.0 | 1.0 px |  |
| `root-selected-hover/selected-hover/footer-primary/button height` | 28.0 | 34 | -6.0 | 1.0 px |  |
| `root-selected-hover/selected-hover/footer-primary/button fill alpha` | 21.73 | -0.37 | 22.1 | 2.0 levels | the reference's footer buttons are transparent at rest |

## Accepted discrepancies: 2 (failing, with a recorded disposition)

| Check | Native/measured | Reference/expected | Delta | Limit | Disposition |
|---|---|---|---|---|---|
| `keycap-windows/keycaps/keys:win-alt-left/cap 2 label ink top` | 6.0 | 8.0 | -2.0 | 1.0 px | accepted (#93): the reference's embedded Geist Mono is a 225-glyph subset without this symbol, so the browser draws it in a system fallback face; Pane draws Geist Mono's own |
| `launcher-frame/frame/frame/top-left corner diagonal inset` | 0.0 | 5.0 | -5.0 | 1.0 px | accepted (#92): the panel paints no radius on Windows - the acrylic backdrop covers the whole window rectangle, so a painted 18px curve would show it as a plate - and the DWM's corner preference (DWMWCP_ROUND, Microsoft's documented 8px) rounds the window instead. This capture is PrintWindow's, taken before the DWM clip, so it shows the panel's square corner, not the 8px the screen shows; the on-screen corner is documented, not measured here |

## Native-only captures (no reference counterpart; not compared)

- `launcher-frame-light`: launcher-frame-light/frame-light.png
- `launcher-frame-narrow`: launcher-frame-narrow/narrow-rest.png, launcher-frame-narrow/narrow-last-selected.png
- `root-long-content`: root-long-content/long-content.png
- `root-unavailable`: root-unavailable/rest.png, root-unavailable/unavailable-selected.png
- `tile-sizes`: tile-sizes/tiles.png

## Pending scenarios (reference saved; native fixture not registered yet)

- `actions-panel` (actions board, 760x518): https://github.com/hoangvu12/pane/issues/95
- `calculator-card` (calculator board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `empty-state` (empty board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `settings-shell` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/97
- `appearance-page` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/98
- `pinned-strip` (root board, 760x518): https://github.com/hoangvu12/pane/issues/101
- `clipboard-split` (clipboard board, 940x600): https://github.com/hoangvu12/pane/issues/102
