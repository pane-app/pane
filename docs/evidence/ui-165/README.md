# #165 evidence: virtualized lists on Windows

Measured and captured on 2026-10-08 from `main` at `ff9ad18d` on Windows 11 build 26200 (x86_64, 16 logical processors), at 96 DPI with opaque material and the dark theme.

## Benchmark

`virtual_lists::benchmark`, run optimized with debug assertions as its doc comment says. This branch fixes its row count: Pane's own "Settings…" row is listed after the commands.

| Measure (11,000 results: 1,000 applications and 10,000 files) | p50 | p95 | max | Target |
| --- | --- | --- | --- | --- |
| Keystroke to frame | 9.86 ms | 13.98 ms | 20.78 ms | p95 ≤ 16 ms: met |
| Scroll frame | 3.49 ms | 5.43 ms | 6.19 ms | 60 fps: met (184 fps at p95) |

## No visible change

`after-rest.png`, `after-hover.png` and `after-selected.png` show the current build: the pinned home, then rows 44 px high every 46 px. The hover and selection washes are the same rounded fill, inset 10 px either side. Compare them with the captures from before #165 in [`../ui-101/crops/root-selected-selected-side-by-side.png`](../ui-101/crops/root-selected-selected-side-by-side.png) (native on the left of the magenta bar), which have the same row pitch, inset and wash. The window tests for row geometry and selection (`window.rs`) pass unchanged.
