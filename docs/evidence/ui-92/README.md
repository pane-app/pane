# #92 evidence: the launcher frame and Windows material treatment

Selected from one full run of `./scripts/visual-workbench.ps1` (with sensitivity, without `-RealAppSmoke`) on 2026-10-04; see [docs/visual-workbench.md](../../visual-workbench.md).

**Machine:** Windows 11 build 26200 (25H2), 96 DPI (100%). 125% and 150% were not run. Chrome 154.0.8037.93. Opaque material, dark theme unless a scenario names its own.

**Revision:** `79b313a` plus this ticket's uncommitted changes (`workingTreeDirty: true`); this ticket's commit is the result. `pane-visual-fixture.exe` SHA-256 `78A06BFB…A47E` (full hash in `workbench-run.json`).

**Reference:** `docs/evidence/ui-prototype/reference/launcher.html`, SHA-256 `F7E81E03…0BB4`.

## What changed

- The launcher window opens at the reference's **760×518** client (64 search header + 404 list + 50 footer), from one constant (`ui::shell::LAUNCHER_CLIENT`) the fixture shares.
- The panel's inner edge is an **inset ring**, as the reference's `box-shadow` is, not a 1px border. The header, list and footer now span all 760px: the row wash is 740px wide (it was 738), and nothing drifts by the border.
- The popover's edge got the same layout-free treatment.
- The footer pads **16 left / 8 right**, the reference's values.
- The list's paddings (4 / 10 / 10, gap 2) are their own tokens. The launcher and the fixture lay rows out through one shared `ui::shell::result_list`.
- Root search's placeholder is now "Search apps and commands…". This is a content adaptation of "Search apps, commands, plugins…": Pane has extensions, not plugins, and their results arrive as commands.

## Results (`compare/summary.md`)

| Section | Passed | Failed | Accepted discrepancies |
|---|---|---|---|
| harness-native | 373 | 0 | 0 |
| harness-reference | 125 | 0 | 0 |
| parity | 399 | 125 | 1 |

In `launcher-frame` (the frame at rest), every frame check passes except the corner:

| Check | Native | Reference |
|---|---|---|
| panel size | 760×518 | 760×518 |
| search header top / height / width | 0 / 64 / 760 | 0 / 64 / 760 |
| list top / height / width | 64 / 404 / 760 | 64 / 404 / 760 |
| footer top / height / width | 468 / 50 / 760 | 468 / 50 / 760 |
| left / right ring alpha (levels) | 18.7 / 18.7 | 19.4 / 19.2 |
| top edge (ring + highlight) | 43.3 | 42.7 |
| bottom edge (ring under the footer wash) | 15.6 | 16.0 |
| footer content right edge (8px padding) | 752 | 752 |
| header rule, footer rule | 63, 468 | 63, 468 |
| top-left corner diagonal inset | 0 (square) | 5 (≈18px radius) — **accepted discrepancy** |

The 125 remaining parity failures belong to later tickets: the keycaps (#93); row metadata, title gap, hover selection and match highlight (#94); the footer button (#95); and the pinned strip and section labels, which offset every row's top (#101).

**Sensitivity** (`compare-<perturbation>/summary.md`):

- Row inset +4px moves the wash's left edge 10 → 14. It now flips parity as well as harness, because the wash width matches the reference.
- The wrong selected fill reads 22 → 64 levels.
- The wrong hover fill reads 9 → 51 levels.

## The Windows material: what was measured and what was not

- **Corners — accepted discrepancy; the on-screen 8px is documented, not measured.** The pinned renderer (GPUI CE, `gpui_windows`) can't produce an 18px corner over a blurred backdrop:
  - Glass is `SetWindowCompositionAttribute` accent 4 (acrylic), which covers the whole window rectangle. A panel painted at 18px would show that acrylic as a plate behind the curve, which is why the panel paints no radius on Windows.
  - What rounds the window is the DWM corner preference (`DWMWCP_ROUND`, documented by Microsoft as 8px).
  - A transparent window with a painted 18px curve would still sit inside DWM's own frame shadow and rim, which don't follow a painted curve.
  - `PrintWindow` reads the client before DWM's clip, so the captures show the panel's own square corner (`crops/frame-corner-native-top-reference-bottom.png`: native top, reference bottom). The comparison records the difference and carries this disposition. The measured 0 is the panel's corner, not the 8px corner the screen shows, which only an on-screen capture could measure.
- **Outside shadow and the .5px black edge — not measured, no disposition yet.** The reference paints `0 0 0 .5px` black 75% and two drop shadows; on Windows the window's shadow is DWM's, and GPUI paints a drop shadow under its own translucent fill, so the panel cannot carry them. Measuring the difference needs an on-screen capture of the composed desktop.
- **Glass (blur 44, saturate 160%, active/inactive) — not run, no disposition yet.** The acrylic blur is applied while Windows composes the desktop, so it exists only in an on-screen capture of the window over a known backdrop. That means showing a window on the operator's screen. The machine was in use, so it was not run. Strict colors stay in the opaque material. Transparency-off and high-contrast still fall back to opaque (`glass_fallback_reason`, unchanged).

**Open on this ticket:** acceptance boxes 3 (glass evidence) and 4 (a measured disposition for shadow, blur and saturation, and an on-screen corner measurement) need that on-screen capture. The popover's edge was made layout-free in the same change. That is the same rule ("paint inset borders without adding unexpected layout inset"), and #95 restyles that surface.

## Other states

- **`crops/frame-light-native.png`** — the derived light palette at the same 760×518 dimensions. It is native-only, with its own harness checks: black washes and rules measured as darkening overlays.
- **`crops/narrow-rest-native.png`, `crops/narrow-last-selected-native.png`** — a 480×360 launcher. After Down ×7 the last row is scrolled into view, flush above the footer, and the footer stays visible. The fixture declares the scrolled positions by GPUI's own scroll rule, and the harness measures them.
- **Long status, resizing, scrolling, dragging and hiding.** These are unchanged code paths, covered by the window suite: `a_long_error_wraps_grows_and_scrolls_inside_the_footer`, `a_long_status_replaces_the_idle_strip_and_stays_readable`, `the_list_scrolls_to_keep_the_selected_row_visible`, `the_production_scenario_edits_searches_selects_opens_and_back_navigates`, and others.
- **New window test.** `the_launcher_divides_its_reference_client_edge_to_edge` asserts the real launcher's 0/64/468 boundaries at 760×518.

## Checks run (local, Windows)

```text
cargo fmt -p pane --check                      ok (after cargo fmt)
cargo check -p pane --tests --locked -j 1      ok
cargo build -p pane --locked -j 1              ok
cargo test -p pane --test window --locked -j 1 69 passed
cargo test -p pane --lib --locked -j 1         19 passed
python -m unittest (scripts/visual-workbench/test_compare.py)  15 passed
```

**Not run:** the real-app smoke (`-RealAppSmoke` opens `pane.exe` centered and active, and the operator was using the machine), the on-screen glass and shadow captures, 125% and 150%, and other OSes.
