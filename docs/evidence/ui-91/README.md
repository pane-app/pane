# #91 evidence: Windows reference/native comparison workbench

These files were selected from one full run of `./scripts/visual-workbench.ps1 -RealAppSmoke` on 2026-10-04 (see [docs/visual-workbench.md](../../visual-workbench.md)).

**Machine:** Windows 11 build 26200.8737 (registry ProductName "Windows 10 Pro", 25H2), 96 DPI (100%). 125% and 150% were not run. Chrome 154.0.8037.93.

**Revision:** `b7f1b881` plus this ticket's uncommitted changes (`workingTreeDirty: true`); this ticket's commit is the result. Binary hashes:

| Binary | SHA-256 |
|---|---|
| `pane-visual-fixture.exe` | `96D9743E…3064E` |
| `pane.exe` | `DCFDCB68…A41F` |

Full hashes are in `workbench-run.json` and `real-app-smoke/pane-run.json`.

**Reference:** `docs/evidence/ui-prototype/reference/launcher.html`, SHA-256 `F7E81E03…0BB4`. The requested `.scratch` copy is identical.

## Files

- **`workbench-run.json`** — the command, each step's outcome, binary hashes and the sensitivity summary.
- **`registry.json`** — the scenarios, their steps and the pending scenarios with their tickets.
- **`compare/summary.md`, `compare/report.json`** — the baseline comparison:

  | Section | Passed | Failed |
  |---|---|---|
  | harness-native | 272 | 0 |
  | harness-reference | 115 | 0 |
  | parity | 306 | 158 |

  Parity is expected to fail until #92–#99 land.
- **`compare-<perturbation>/summary.md`** — the sensitivity runs. Each lists the checks that passed in the baseline and fail under the fault:
  - row padding +4px: the wash's left edge goes 11 → 15 px, a 4px delta against the declared 11;
  - wrong selected fill: 22 → 64 levels;
  - wrong hover fill: 9 → 51 levels.
- **`crops/`** — 1:1 side-by-sides (native left, reference right) and row crop pairs (native top, reference bottom).
  - `perturbed-*` crops show the faults.
  - `*-native.png` are the native-only states: unavailable and long content.
- **`reference/boards/`** — every reference board's glass panel at its own size, with Windows labels. Root is 760×518, Settings 1120×720 and clipboard 940×600. `reference-manifest.json` holds the DOM state at each capture.
- **`native/root-selected/`** — one scenario's fixture manifest (the declared values) and run record (DPI 96, client 760×518, off-screen, never foreground, exit code, cleanup).
- **`real-app-smoke/`** — the real `pane.exe` search interaction: type `install`, Down, Enter (the npm form), Escape, Escape. The captures and `pane-run.json` record the three passed `check_screenshot.py` assertions.
