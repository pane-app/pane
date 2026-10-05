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

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-reference | 125 | 0 | 0 |
| harness-native | 373 | 0 | 0 |
| parity | 399 | 125 | 1 |

## harness-native: 0 failed


## harness-reference: 0 failed


## parity: 125 failed

| Check | Native/measured | Reference/expected | Delta | Limit | Note |
|---|---|---|---|---|---|
| `keycap-windows/keycaps/keycap:ctrl-k/labels` | Ctrl+K | Ctrl | K | None | 0  | production shows one cap with the chord's text; the reference one cap per key |
| `keycap-windows/keycaps/keycap:ctrl-k/group width` | 53.0 | 59.41 | -6.41 | 1.0 px | label widths are controlled: same effective binding on both sides |
| `keycap-windows/keycaps/keycap:ctrl-k/label font` | 14px Geist 500 | 11px Geist Mono 500 | None | 0  | the reference's keycaps set Geist Mono 11/500 |
| `keycap-windows/keycaps/keycap:win-alt-left/labels` | Alt+Win+Left | Win | Alt | ← | None | 0  | production shows one cap with the chord's text; the reference one cap per key |
| `keycap-windows/keycaps/keycap:win-alt-left/group width` | 99.0 | 85.62 | 13.38 | 1.0 px | label widths are controlled: same effective binding on both sides |
| `keycap-windows/keycaps/keycap:win-alt-left/cap fill alpha` | 20.19 | 18.11 | 2.08 | 2.0 levels |  |
| `keycap-windows/keycaps/keycap:win-alt-left/label font` | 14px Geist 500 | 11px Geist Mono 500 | None | 0  | the reference's keycaps set Geist Mono 11/500 |
| `keycap-windows/keycaps/keycap:ctrl-shift-v/labels` | Ctrl+Shift+V | Ctrl | Shift | V | None | 0  | production shows one cap with the chord's text; the reference one cap per key |
| `keycap-windows/keycaps/keycap:ctrl-shift-v/group width` | 94.0 | 105.41 | -11.41 | 1.0 px | label widths are controlled: same effective binding on both sides |
| `keycap-windows/keycaps/keycap:ctrl-shift-v/cap fill alpha` | 20.02 | 17.67 | 2.34 | 2.0 levels |  |
| `keycap-windows/keycaps/keycap:ctrl-shift-v/label font` | 14px Geist 500 | 11px Geist Mono 500 | None | 0  | the reference's keycaps set Geist Mono 11/500 |
| `keycap-windows/keycaps/keycap:enter/labels` | Enter | ↵ | None | 0  | production shows one cap with the chord's text; the reference one cap per key |
| `keycap-windows/keycaps/keycap:enter/group width` | 22.0 | 20 | 2.0 | 1.0 px | label widths are controlled: same effective binding on both sides |
| `keycap-windows/keycaps/keycap:enter/cap fill alpha` | 20.1 | 174.81 | -154.71 | 2.0 levels | the reference's Enter cap is the lime accent |
| `keycap-windows/keycaps/keycap:enter/label font` | 14px Geist 500 | 11px Geist Mono 500 | None | 0  | the reference's keycaps set Geist Mono 11/500 |
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
| `root-focus/focus-typed/search-header/query/placeholder ink left` | 55.0 | 57.0 | -2.0 | 1.0 px |  |
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

## Accepted discrepancies: 1 (failing, with a recorded disposition)

| Check | Native/measured | Reference/expected | Delta | Limit | Disposition |
|---|---|---|---|---|---|
| `launcher-frame/frame/frame/top-left corner diagonal inset` | 0.0 | 5.0 | -5.0 | 1.0 px | accepted (#92): the panel paints no radius on Windows - the acrylic backdrop covers the whole window rectangle, so a painted 18px curve would show it as a plate - and the DWM's corner preference (DWMWCP_ROUND, Microsoft's documented 8px) rounds the window instead. This capture is PrintWindow's, taken before the DWM clip, so it shows the panel's square corner, not the 8px the screen shows; the on-screen corner is documented, not measured here |

## Native-only captures (no reference counterpart; not compared)

- `launcher-frame-light`: launcher-frame-light/frame-light.png
- `launcher-frame-narrow`: launcher-frame-narrow/narrow-rest.png, launcher-frame-narrow/narrow-last-selected.png
- `root-long-content`: root-long-content/long-content.png
- `root-unavailable`: root-unavailable/rest.png, root-unavailable/unavailable-selected.png

## Pending scenarios (reference saved; native fixture not registered yet)

- `actions-panel` (actions board, 760x518): https://github.com/hoangvu12/pane/issues/95
- `calculator-card` (calculator board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `empty-state` (empty board, 760x518): https://github.com/hoangvu12/pane/issues/96
- `settings-shell` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/97
- `appearance-page` (settings board, 1120x720): https://github.com/hoangvu12/pane/issues/98
- `pinned-strip` (root board, 760x518): https://github.com/hoangvu12/pane/issues/101
- `clipboard-split` (clipboard board, 940x600): https://github.com/hoangvu12/pane/issues/102
