# #103 evidence: the integrated Windows UI, component by component

The integrated acceptance record for the UI port ([#90](https://github.com/hoangvu12/pane/issues/90)), on Windows only, from local checks. It is not release certification. Everything here is **local and unpushed** on `impl/ui-91`. **#90 is not complete:** the remaining operator-consent items and the open user questions below must resolve first.

**Real-window evidence, added later.** After the user's go-ahead ("no games rn", then "just run"), the coordinator ran the consented visible steps on the real `pane.exe`:

- the glass and opaque captures over matched backdrops (window crop);
- the real-app smoke;
- a full-screen shadow capture;
- captures at 125% and 150% display scale. The scale was restored to 100% on both monitors afterwards, as the coordinator verified.

They are recorded in [Real-window evidence](#real-window-evidence-glass-and-the-real-app-smoke) and [`real-app/`](real-app/). I measured and looked at their images; I ran nothing visible myself.

**Files:**

- [`ledger.md`](ledger.md): the component-by-component ledger. Every board, component and state, with its source target, its native evidence and its result, then the accepted deviations with their full dispositions.
- `compare/summary.md` and `compare/report.json.gz`: the full run's summary and every check, unfiltered.
- `compare-<perturbation>/summary.md`: the six sensitivity perturbations.
- `workbench-run.json`, `registry.json`, `reference-manifest.json.gz`: the run's record, its scenarios, and the reference capture's manifest, including each capture's font and pointer record.
- `light-variant.json`: the `-Theme light` run's totals and harness failures.
- `test-summary.txt`: the full test run, target by target.
- `atlas/`: 29 images copied from `run-103/compare/`. Each is described [below](#atlas-what-the-images-show).
- `real-app/`: the real window's glass and opaque captures, their backdrop patterns, their run records, `glass-measurements.json`, and the real-app smoke's captures and records.

## Identifiers

| What | Value |
|---|---|
| Source | `6102aa6` on `impl/ui-91`, clean tree (untracked `.scratch/`, `.claude/`, `docs/research/` only). `6102aa6` adds the workbench guards (scripts only) on top of `500efe6`, whose Rust sources it leaves unchanged. |
| Integrated tickets | #91 `79b313a` · #92 `a084715` · #93 `554c4ef` · #94 `d5e22d9` · #95 `ffc86ae` · #97 `f69ac68` + `150ce57` · #96 `b3474a3` · #102 `a3cc4c7` + `55882ba` · #101 `774245b` + `8785794` · wave-1 evidence `bfc86e2` · #98 `510b7b8` + `8cfad9e` + `fc97ff7` · #99 `40e1b5d` + `35dae93` + `500efe6` · #103 guards `6102aa6` |
| `target/debug/pane.exe` | SHA-256 `A5A9317B0216DC30B5CFBF24719A408FCC334E932340F992869A9AC2E178989A`, from `cargo build -p pane --locked -j 1` at `6102aa6`. Builds here are not bit-reproducible, and `cargo test` relinks the binary with test features (`8844E191…D945` after the test run), so this hash names this one build. |
| `target/debug/pane-visual-fixture.exe` | SHA-256 `F8C509A7575956E7ECDA6E0DFCADC2C0864F93E7B16951639ECC103D3241994F`. The workbench built and recorded it, with the sources' digest `CD93FD3E…88B8` (`workbench-run.json`, `fixture.freshness`). |
| Reference | `docs/evidence/ui-prototype/reference/launcher.html`, SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`. It is verified by the run, and `.scratch/ui-reference/launcher.html` has the same hash. |
| Toolchain | rustc 1.98.1 (`48a229cea` 2026-09-01), cargo 1.98.1, x86_64-pc-windows-msvc; Node 24.18.0; Python 3.12.10 with Pillow; Chrome 154.0.8037.93 (headless). |

## OS, DPI and material

- **OS:** Windows 11 Pro 25H2, build 26200.8737. The registry's `ProductName` reads "Windows 10 Pro", as it does on every Windows 11.
- **Scale:** 96 DPI (100%) on every native capture, and device scale factor 1 on the reference. The new parity check `client/device scale` compares the two: 45/45 pass. The fixture workbench cannot capture at 125% (see [below](#scaled-displays-125-and-150)). The real window was captured at 125% and 150% and reviewed by eye (R3), with no geometry parity at those scales.
- **Client sizes:** root 760×518, Settings 1120×720, clipboard 940×600, each checked on both sides. The narrow variants are 480×360 (launcher), 760×520 (Settings) and 760×518 (clipboard).
- **Material:** every workbench capture is in the opaque ("solid") material. Glass exists only in an on-screen composition: acrylic blur, the DWM corner and the window shadow. It was captured later on the real window over matched backdrops, window crop only, and is measured in R1 ([below](#real-window-evidence-glass-and-the-real-app-smoke)). Windows' transparency setting was on (`transparencySetting: 1`). A full-screen capture shows that the real window draws **no outer shadow** (R2, an open deviation).
- **Theme:** dark. The light palette is a derived adaptation with no light board. `launcher-frame-light` and `appearance-light` are measured in the main run, and a whole `-Theme light` run is recorded [below](#the-light-variant).

## Checks (local, Windows)

```text
cargo fmt -p pane -p pane-core --check                                 clean (at 500efe6)
cargo check -p pane --tests --locked -j 1                              ok
cargo build -p pane --locked -j 1                                      ok
cargo test -p pane-core -p pane --locked -j 1 --no-fail-fast           68 targets, 1453 passed, 0 failed (one run)
python -m unittest discover -s scripts/visual-workbench -p "test_*.py" 72 passed (56 + 8 new guard tests + 8 freshness tests)
./scripts/visual-workbench.ps1 -OutputDir .scratch/visual-workbench/run-103            exit 0, see below
./scripts/visual-workbench.ps1 -OutputDir .scratch/visual-workbench/run-103-light -Theme light -SkipSensitivity -SkipBuild   exit 0, see below
./scripts/visual-workbench.ps1 -OutputDir .scratch/visual-workbench/run-103-smoke -Scenario root-rest -SkipSensitivity -RealAppSmoke   exit 0 (run by the coordinator), see "Real-window evidence"
```

**The full test run had no failure.** These were all checked and none failed:

- pane-core `helpers.rs`, 44/44. It has failed 6–7 process-timing tests at `ffc86ae` and in earlier full runs, and passed in this one.
- `window`'s `the_mouse_wheel_scrolls_away_until_the_rows_reload`, a known flake under load.
- core `command_search`'s `reloading_the_package_stops_its_search`, a known flake under load.
- `settings.rs`'s former frame-loop pair, fixed under #97.

Nothing needed a fix, so no ticket received a fix commit. The run took 34 minutes, compile included, with nothing else building.

## The workbench: run-103

A full run with sensitivity at `6102aa6` (clean tree, dark theme, opaque material), into `.scratch/visual-workbench/run-103`:

| Section | Passed | Failed | Accepted |
|---|---|---|---|
| harness-native | 7197 | 0 | 0 |
| harness-reference | 836 | 0 | 0 |
| parity | 10504 | 0 | 131 |

These counts are run-w3's plus the guards' new checks: 302 harness-reference (257 font, 45 pointer) and 45 parity (`device scale`). Every other count is unchanged. **Every section is green, and every perturbation flips the checks it drives:**

| Perturbation | Flips (harness + parity) | Example |
|---|---|---|
| `row-padding-plus-4` | 8 "wash left" (482 checks in all) | wash left 10 → 14 |
| `selected-fill` | 8 | selected wash 22.05 → 63.57 levels |
| `hover-fill` | 2 | hover wash 8.82 → 51.07 |
| `nav-selected-fill` | 6 | Appearance's nav wash 23.0 → 63.9 |
| `segment-on-fill` | 8 | chosen segment 31.2 → 77.1 |
| `well-fill` | 8 harness | recorder well 61.1 → 129.4 |

### The guards the acceptance asks for

The second acceptance box asks that "no resized images, wrong-font fallback, uncontrolled cursor position or old binary can pass unnoticed". Four of those guards were missing before `6102aa6`:

- **binary freshness**;
- **the reference's fonts**: they were recorded but never checked;
- **the reference's pointer**: it was not parked between scenarios and not recorded;
- **an explicit same-scale check.**

`6102aa6` adds them, with tests. [docs/visual-workbench.md](../../visual-workbench.md#what-cannot-pass-unnoticed-103) lists every guard and where it lives:

| Guard | Where | In run-103 |
|---|---|---|
| Reference hash pinned | `visual-workbench.ps1` step `reference-hash`; `reference-capture.mjs` exits 3 on other bytes | verified |
| Same DPI | native: `native-capture.ps1` checks the client against its DPI. **New:** parity `client/device scale` (native DPI / 96 against the reference's factor; `compare.scale_parity`) | 45/45 |
| Same client bounds | `native-capture.ps1` throws on another size; harness-native `client/size` (75), parity `client/size` (45), harness-reference `boards/*/glass/size` (8) | all pass |
| No resized image | `PrintWindow` client and the CDP clip at scale 1; compared 1:1, so the size checks fail a rescaled image | all pass |
| No fallback face | native: `check_fonts` (315 checks). **New:** reference, `check_reference_fonts`. Each board's own FontFaceSet is awaited before the capture and recorded with it; the board's Geist/Geist Mono weights must have loaded and no face may have failed (257 checks). | all pass |
| Controlled pointer | native: the window is parked off every display (the run throws otherwise), input is posted to its HWND, and `PrintWindow` draws no cursor. **New:** reference, the pointer is parked off every board before each scenario and recorded at each capture. Harness-reference `pointer` requires it off the board or on the query field before the scenario's own pointer steps, and on the board after them (45 checks). | all pass |
| No old binary | **New:** `freshness.py`. A build records the fixture's hash and its sources' digest beside it; `-SkipBuild` verifies both, and refuses a hand-built binary or one older than the tree. | `record` in run-103, `verify` in the light run |

A demonstration on the tree: an untracked `crates/pane/src/zz_freshness_probe.rs` made `freshness.py verify` refuse ("the sources changed since the binary was built", exit 2). Deleting it made the verify pass again. The guard logic is unit-tested in `test_compare.py` (`ReferenceGuards`, 8) and `test_freshness.py` (8).

### The light variant

`-Theme light -SkipSensitivity -SkipBuild`, into `run-103-light` (`light-variant.json`). The fixture passed `verify`.

- **harness-native:** 6740 passed, 59 failed.
- **Parity and harness-reference:** not meaningful. The boards are dark only, and some reference measurements take the native manifest's palette. The 7 harness-reference failures are all the Appearance miniature's row-title ink left (760 against 764). The same pinned reference passed these checks in run-103, so I take them to be that palette assumption, but I did not confirm it.

The 59 harness-native failures fall in families whose measurement was only ever tuned for the dark palette:

- the clipboard preview card's ring (card top 116 against 123);
- the chosen segment, white on a light track (its alpha reads negative);
- the answer card's ring, darker green in light, where the check looks for lime;
- the open Actions footer's rule;
- the tile-stroke ratio.

I looked at three light captures (`root-actions/open`, `answer-plain`, `clipboard-production/rest`). They render coherently: the ring is visible in dark green, and the panel and popover read correctly. One thing I saw: the preview card shades from lighter at its top to darker below.

**I did not classify these failures one by one as product or measurement faults.** So the light acceptance claim stays limited to `launcher-frame-light` and `appearance-light`, which pass in run-103. Light in every other family is "not covered".

## Ledger summary

[`ledger.md`](ledger.md) has 76 rows:

| Result | Rows |
|---|---|
| pass | 18 |
| accepted deviation | 21 |
| native-only (harness pass, no board state to compare) | 25 |
| not covered | 10 |
| open deviation (R2: no outer shadow) | 1 |
| reviewed by eye (R3: 125%/150%) | 1 |

The 131 accepted checks carry 18 dispositions (D1–D18), each copied in full into the ledger. In brief:

- **Platform rendering (75 checks):**
  - GPUI's glyph contrast thickens icon strokes (D4, 43) and nav glyph boxes (D16, 6).
  - Chrome's glass composites the sidebar's black 10% about 0.3 level darker (D15, 6).
  - The reference's Geist Mono subset lacks ← and ↵ (D3, 20).
- **Windows frame (1):** the corner is DWM's, not a painted 18px (D1). On the real window it measures about 8px (R1).
- **Real-window glass (R1, measured outside the workbench):**
  - the blur is present;
  - it passes about 25% of the backdrop, against the CSS's 30%;
  - the acrylic holds when the window is inactive.

  Saturation is unmeasured.
- **Real-window shadow (R2, open):** no outer drop shadow. There is only a 1px mid-grey DWM rim, where the reference has a soft shadow.
- **Real window at 125% and 150% (R3):** proportional and legible, reviewed by eye.
- **Pane content and behaviour, per #100 (55):**
  - "Commands" rather than "Suggested · From your recent use" (D2, 19);
  - only real Actions entries (D6, 3);
  - fallbacks unselected, with their match highlighted and no reorder note (D9–D11, 10);
  - no store or paste advertisements (D12, D14, D8, 4);
  - the notice's wording (D13, 4);
  - the accent Enter cap and white open label against a board that disagrees with itself (D7, D5, 4);
  - GPUI scrolls the least that shows the clip, not an 8px cushion (D17, 10);
  - the no-match footer shows only Actions (D18, 1).

The 10 not-covered rows are:

- real Settings minimize, maximize, drag and resize (no script; pending);
- footer hover;
- long footer status;
- Actions entry hover and a target gone from behind the panel;
- slot focus;
- Settings sidebar keyboard focus (open question);
- the segment focus ring;
- the Shortcuts page;
- an extension's custom view in the host frame;
- clipboard tab and row hover.

Each names the window tests that cover its behaviour where they exist.

## Literal matches

The authored values, matched within the #91 limits (1 logical px for edges and baselines, 2 levels for flat fills, 4 for glyph cores) in reference-backed parity:

- **Launcher frame:** 760×518, header 64, list 404, footer 50, all 760 wide; the inset ring and top highlight; footer padding 16/8; the header and footer rules.
- **Search field:** 19/400 with its placeholder and caret position, at rest and focused with a typed query. Its copy differs; see the adaptations.
- **Result rows:** 44/r10 with a 28/r7 tile; title 14/500, subtitle 13 with a 12px gap; kind, alias chip and keys; the match highlight in the accent. Every state matches: rest, hover (pointer movement selects), selected, selected-hover, and the pointer/keyboard handover with its 3.5% hover-only wash.
- **Section labels:** "Pinned", "Results · N matches", "Fallbacks" (place and type).
- **Keycaps:** regular (Mono 11/500, 20 high, r5, white 7%, ring and bottom inset), compact (17), the lime Enter cap, and a 3px gap. Group widths agree within the 1px limit.
- **Pinned strip:** five 100/r12 slots, 42/r11 tiles, 12.5/500 titles, compact chords, and the 7% hover. A query hides the strip and clearing it restores it.
- **Footer:** the mark, the hint, `.fbtn` 34/r8 at rest, the 1×16 rule, Actions, and the open 10% wash.
- **Actions panel:** `.pop` 320/r14 placed right 10 and bottom 58; the header with its 18/r5 tile; 36/r8 entries with the 11% selected wash; the group label, the rule and the 44px search row; "No actions match"; Escape closing only the panel; the dimmer.
- **No-results notice:** its disc and lines. **Fallback rows:** 44px.
- **Answer card:** 740×160/r14 with its accent ring, Mono 34/500 values, captions, 40px arrow, chips and rule (board fixture).
- **Settings shell:** titlebar 48, the 232px sidebar and its 34px search well, `.nav` 36/r8 (rest, hover 5%, selected 9%, selected-hover), and the heading block.
- **Appearance:** segment tracks 36/r10 with 30/r7 segments (rest, hover, chosen); labels 13.5/500 and descriptions 12.5/1.45; the miniature's rows, slots, search line, caret and footer; Solid's 40% dimming.
- **Clipboard split view:** header 64, the tab strip, the 360px list with 44px rows and Mono timestamps, the 556×414 preview card (code, text, link, image), and the 52px footer. Every state matches: rest, click-to-select, keys, no-match, cleared, and the Text tab.

## Approved adaptations

Each is recorded with its ticket. Accepted parity checks carry their Dn.

- **Windows caption buttons** (minimize, maximize, close) replace the Settings board's lone close glyph (#97). They are not compared.
- **DWM window corners** (`DWMWCP_ROUND`, documented 8px) replace a painted 18px radius, because acrylic covers the whole window rect (#92, D1). On the real window the corner measures about 8px on glass and opaque alike (R1). The reference's outer shadows and .5px black edge have no counterpart: the real window draws no outer shadow, only a 1px mid-grey DWM rim. This is measured, and it is **an open deviation, not an approved adaptation** (R2).
- **Windows keycaps and effective bindings:** Ctrl, Alt and Win names; Win Ctrl Alt Shift key order; full chords such as Shift ↵, Ctrl ↵ and Ctrl Shift P. The boards are captured with `platform: 'Windows'` (#93).
- **Pane-specific copy:**
  - "Search apps and commands…" (#92);
  - "Commands", with no recent-use claim (D2);
  - "Nothing matches…" and "install an extension" (D13);
  - "Open documentation" (#99);
  - "Manage" for the clipboard's Actions (#102);
  - the clipboard caption reads the real retention (#102).
- **The Pane mark** opens the Pane menu (Settings), apart from the contextual Actions panel (#95, #100).
- **Root behaviour (#100):**
  - fallbacks stay unselected until a deliberate selection (D9), and their titles show the match (D10, D11);
  - footers keep Pane's hint where the boards advertise a store or paste (D8, D12, D14);
  - the accent Enter cap (D7), and the white open Actions label (D5);
  - the Actions panel lists only operations Pane can perform (D6).
- **Clipboard:**
  - scroll-least on keys (D17);
  - no primary action with nothing listed (D18);
  - the window grows to 940×600 while the view shows;
  - Delete is Ctrl+D and Manage is Ctrl+K (#102).
- **Settings (#97, #99):**
  - Pane's seven pages with Pane's glyphs, where the board has palette, shield and info;
  - the Extensions count;
  - work-area sizing (minimum 560×400) and a one-column collapse below the canonical width;
  - focus rings on segments;
  - the derived pages (General, Launcher, Keyboard, Extensions, About, Shortcuts, and the host form). These are described as reference-consistent, never as matching a board.
- **Computed answers (#96):** an 80px production card with no captions or chips without data behind them. The card shows for every copying answer, with its ring only while selected.
- **Pins (#101):**
  - the dashed Empty slot;
  - the dimmed unavailable slot with its reason;
  - the generic command tile for a pinned application;
  - a text list as the full-slots replacement choice.
- **The light palette** and the **narrow layouts** are derived; no board authors them.
- **Measured platform limits:**
  - GPUI's sprite contrast (D4, D16);
  - the glass compositing of the sidebar fill (D15);
  - the reference's own font subset (D3);
  - the query's −.005em tracking, which GPUI's editable text can't apply (about 0.1px a character, #93);
  - subpixel against grayscale anti-aliasing;
  - kerning switched on explicitly (#95);
  - a 1/255 footer fill that orders the footer above the panel's shadow (#95).

## Absent or deferred reference features (#100)

None of these is implemented, and no capture or text claims them. The reference variants that appear in the workbench are fixture content only.

- **The Plugin Store board** and its catalog, acquisition, install and update backends; "From the Plugin Store" suggestions in the empty state; "Find themes in the Plugin Store".
- **The snap HUD (window-manager) board** and its window-manipulation backend.
- **Advanced Appearance:**
  - the Frost material and accent swatches;
  - custom colour;
  - blur and tint sliders;
  - Density;
  - the pinned-visibility and footer-tip toggles;
  - the preview's wallpaper.

  The board's versions are drawn only by the fixture.
- **Calculator extras:** unit and time conversion, unit captions, "Also" chips, calculation history ("Recent calculations"), and pasting the answer into the previous app.
- **Clipboard extras:**
  - image, colour and link acquisition, and the Links, Images and Colors tabs;
  - the Pinned clips group;
  - code and syntax classification;
  - network previews;
  - "Paste to Obsidian" (paste-to-app);
  - "Password managers are never recorded" as a claim.
- **Recent-use suggestions:** "Suggested · From your recent use", frecency and telemetry.
- **Actions entries:** Open New Window, Show in File Manager, Quit, Hide from Results.
- **The 34px launch toast tile:** Pane has no launch toast (#93).
- **The board's Settings sections:** Hotkeys & Aliases, Plugins, Window Manager, Clipboard and Privacy as pages. Pane keeps its own seven: General, Launcher, Appearance, Shortcuts, Keyboard, Extensions and About.

## Found while reviewing, not measured

- **The narrow launcher's footer hint is cut mid-keycap.** At 480×360, "· Ctrl K for more" ends in a clipped "Ctr" behind Run Command (`atlas/launcher-frame-narrow--narrow-last-selected-native.png`). The hint is `overflow_hidden` in `ui/footer.rs` (#95). No check measures it. It is left unfixed here and is a user question. The real window shows the same thing at its 380×420 narrow size (`real-app/glass-light-narrow.png`, `real-app/opaque-light-narrow.png`): only a fragment of the ↵ cap shows before Open command. At 125% and 150% the narrow window clips it the same way (`real-app/dpi125-narrow-trimmed.png`, `dpi150-narrow-trimmed.png`).
- **The colour preview's hex/rgb/hsl lines sit about 14px higher** natively (`atlas/clipboard-previews--color-side-by-side.png`). The preview is fixture-only (#102), and the check covers only which branch shows.
- **Solid's dimmed sliders keep the accent at 40%,** where the board's disabled range is grey (#98; fixture-only family).
- **The Settings shell scenario** draws the shell only: its page body below the heading is empty, while the board shows the Appearance page. The Appearance scenarios draw the page.
- **Hover captures:** the reference's caret is absent in `root-hover/hover`, presumably mid-blink, while Pane draws no blink. Caret ink is excluded from header checks.

## Interaction and behaviour boxes

These are covered by the full test run, not by captures. The fixture attaches no behaviour, and its input is posted window messages, not the OS's hit-testing.

- **Hover selects; Enter invokes the visible target once:**
  - `root_rows_select_under_the_moving_pointer_at_once`
  - `a_resting_pointer_leaves_the_keys_selection_alone`
  - `the_footer_button_runs_the_selected_action_like_enter`
  - the double-dispatch test
  - `a_slot_runs_once_per_press_and_an_empty_one_ignores_its_keys`
- **Menu focus and dismissal:**
  - `the_open_binding_…_escape_closes_only_them`
  - `an_outside_click_closes_the_panel_without_invoking_what_it_covered`
  - `the_pointer_cannot_change_the_panels_target`
  - `a_target_gone_from_behind_the_panel_runs_nothing`
- **Binding labels:** `the_actions_binding_follows_the_keyboard_page`, and #93's rebinding test that checks the hint and the invoked action together.
- **Form, IME, navigation, Settings persistence, privacy and errors:** the `window` (101), `settings` (62), `launcher_settings` (27), `shortcuts` (24), `keyboard` (19), `settings_search` (8) and `install` (12) suites, and core's `clipboard` (66), `clipboard_view` (11), `quick_slots` (14) and `hotkeys` (21). All pass.
- **Extension drawing is not recoloured:** the custom-view window tests pass. No test asserts the host frame's ring, and no scenario captures it.
- **Not run against the real `pane.exe`:** restart persistence (theme, material, pins) is shown only by a fresh application in the test process.

## Real-window evidence: glass and the real-app smoke

Run by the coordinator, after the user's go-ahead, against `pane.exe` SHA-256 `A5A9317B…989A`. That is the same binary as above, rebuilt with a plain `cargo build` at `582b73a`. All three captures are at 96 DPI, and each run verified that its backdrop sat directly behind Pane in Z-order. I measured the captures (`real-app/glass-measurements.json`, from a script reading the PNGs only) and looked at every image copied here.

**Glass and opaque over matched backdrops** (`scripts/capture-pane-windows.ps1 -Theme dark -Material glass|opaque -Backdrop -BackdropPattern light|dark [-ExerciseWindow]`, window crop, **no `-FullScreen`**). The runs are `.scratch/ui-captures/103-glass-light/run-20261005-160732-4f92a4b5`, `103-glass-dark/run-20261005-160741-473a6182` and `103-opaque-light/run-20261005-160754-eb8f7845`. Each holds the initial capture plus active, inactive, moved, narrow and restored. Copied here: `glass-light-{initial,moved,narrow}.png`, `glass-dark-initial.png`, `opaque-light-{initial,narrow}.png`, both backdrop patterns, and each run's `pane-run.json`.

- **What the images show:**
  - Glass over the light backdrop (#ECECEC with black blocks and bands, `backdrop-light-pattern.png`): the pattern's black blocks show through as soft dark blobs behind a grey-tinted panel. No hard edge survives.
  - Glass over the dark backdrop (#161616 with white blocks): the white blocks show as soft light patches in a darker panel.
  - The opaque control over the light backdrop: a flat dark panel with nothing showing through.
  - All three show the real fresh-install home: five dashed "Empty" slots (the dashes are faint, especially in opaque), Commands with the sample rows (Rust, JavaScript, TypeScript sample, Install extension from folder and from npm), and the footer hint with its keycaps.
  - `glass-light-moved.png`: the blobs sit elsewhere, so the blur follows what is behind the window.
  - `glass-light-narrow.png` (366×413 crop): five narrow Empty slots, rows ellipsized, and the footer hint clipped to a fragment of its ↵ cap (see [Found while reviewing](#found-while-reviewing-not-measured)).
- **Measured (R1 in the ledger):**
  - **Blur present.** The backdrop steps 236 levels from one pixel to the next. Behind the panel's content-free areas no two neighbouring pixels differ by more than 2–3 levels, and a column profile in the right padding varies smoothly between 52 and 69 over about 100px.
  - **Tint.**
    - Comparing the two backdrops, the panel passes about 25% of the blurred backdrop: 0.24 in the header, 0.25 in the right padding, and 0.34 in a strip whose own backdrop mean departs from the pattern's.
    - The CSS `.glass` passes 30% under a 5% sheen, so Pane's glass reads about 5 points more opaque.
    - The base recovered from both backdrops is about 24 levels of luma, against the opaque control's 23 (#16171A-ish).
    - This method's spread between regions is as large as that difference, so I record it as a measured near-match, not an exact one.
  - **Corners are DWM-rounded, at about 8px,** on glass and opaque alike. The backdrop shows whole in the first 2 pixels along the diagonal and the first 4 along the edge, then anti-aliases into a 1px DWM rim. A radius of 8px predicts a 2.3px diagonal inset; the reference's 18px would predict 5.3. The crop is the DWM frame, 762×519 around the 760×518 client, and the rim's luma follows the backdrop (189 over light, 60 over dark at the top edge).
  - **Inactive.** With another window in the foreground (`foregroundIsPane: false`), the capture is byte-identical to the active and restored ones, in all three runs. **The acrylic holds when the window is inactive**, and nothing else in the crop changes.
- **Not measured:**
  - saturation: both backdrops are grey, so `saturate(160%)` has nothing to act on;
  - the outer drop shadows and the .5px black edge beyond the window rect, because `-FullScreen` was not run;
  - the blur radius against the reference's 44px;
  - the glass preview miniature over a real desktop (#98).

**Real-app smoke** (`./scripts/visual-workbench.ps1 -OutputDir .scratch/visual-workbench/run-103-smoke -Scenario root-rest -SkipSensitivity -RealAppSmoke`, revision `582b73a`, clean tree). Its `root-rest` comparison came out the same as run-103's: harness-native 199/0, harness-reference 53/0, parity 399/0/4. The real `pane.exe` passed all three assertions:

- `00`–`03` are distinct;
- `02-down` equals `04-escape`: Escape from the form returns to exactly the search it was opened from;
- `00-root` equals `05-escape-again`: the second Escape clears the query.

The hashes confirm both pairs (`F49430AD…`, `0FEBA31F…`). Copied here: `smoke-00-root.png` through `smoke-03-enter.png`, `smoke-pane-run.json` and `smoke-workbench-run.json`.

- `00`: the fresh-install home (five Empty slots, Rust sample selected, Open command).
- `01`: "install" typed gives "Results · 3 matches", with "Install" in the accent and folder selected.
- `02`: Down selects "Install extension from npm"; the footer reads "Install from npm".
- `03`: Enter opens the form "Install extension from npm", with its field focused, Show package, and Submit ↵.

The smoke drives the real window through posted window messages, as the fixture does. It exercises the real feature adapters, not the OS's own pointer hit-testing or focus arbitration.

### The outer shadow (full screen)

The coordinator ran `scripts/capture-pane-windows.ps1 -Theme dark -Material glass -Backdrop -BackdropPattern light|dark -FullScreen`, in runs `.scratch/ui-captures/103-shadow-light/run-20261005-162642-f09c37dc` and `103-shadow-dark/run-20261005-162646-f9a41555`.

- **What is committed.** The full-screen PNGs show the operator's other applications outside the backdrop, so they are not committed, and neither is the wider `shadow-crop.png`. Only `shadow-crop-backdrop-only.png` is committed (`real-app/shadow-{light,dark}-backdrop-only.png`): screen 563,233 to 1373,800, wholly inside the test backdrop. I looked at both: they show only the launcher and the black-and-white (or white-on-dark) pattern around it.
- **No outer drop shadow.**
  - The pattern's hard edges run right up to the window's edge on every side, with no darkening.
  - Measured (`real-app/shadow-and-scale-measurements.json`; the frame is 587,257 to 1349,776). Below the bottom edge, at x 700, 968 and 1200, y 772–775 read 66, 66, 78, 189 over the light backdrop, and every row from 776 on reads 236, the backdrop itself. Over the dark backdrop they read 22, 21, 37, 60, then 22.
  - Past the left and right edges at y 500, and above the top edge, the backdrop resumes one pixel past the rim.
  - The rim is a 1px mid-grey, semi-opaque DWM edge (189 over 236, 60 over 22). It is not the reference's `.5px` black 75% edge, which would darken both backdrops.
- **The reference** adds `0 50px 120px -30px rgba(0,0,0,.72), 0 16px 40px -16px rgba(0,0,0,.5)`. My estimate from that CSS (a Gaussian of sigma = blur/2, not a Chrome render) is that it darkens the backdrop by about 58% right under the bottom edge, 50% at 10px, 27% at 40px and 7% at 100px. Over the light backdrop that is about 99, 118, 172 and 220, where Pane leaves 236.
- **Disposition: open (R2).** This is a measured deviation, not an accepted one.
  - The cause hasn't been investigated in code. Likely DWM draws no shadow for this window's style or for the acrylic accent, and GPUI paints nothing outside the window rect.
  - Options for #92, already weighed in its decision record: a DWM shadow attribute or style, or a painted shadow inside a transparent margin (which brings back the rim and corner problems #92 recorded).
  - It is a follow-up for #92 and a user question.

### Scaled displays: 125% and 150%

- **The fixture workbench at 125% failed** (`.scratch/visual-workbench/run-103-125`, aborted): every native capture reported "client is 760x518 physical px; the scenario needs 950x648 (… at 120 DPI)".
  - The fixture is parked past the virtual screen's right edge, so it is probably on no monitor and keeps the default 96-DPI sizing.
  - The client-size guard caught it, as it should.
  - 150% was not attempted on the workbench.
  - **Workbench follow-up:** park the fixture on a real monitor but cloaked (`DWMWA_CLOAK`) or fully occluded, or give the window a per-monitor DPI at creation. Then a scaled reference capture, to make parity meaningful at scale.
- **The real `pane.exe` on screen** (`capture-pane-windows.ps1 -Theme dark -Material opaque -Backdrop -BackdropPattern dark -ExerciseWindow`): runs `.scratch/ui-captures/103-dpi-125/run-20261005-163042-adc04cd7` (DPI 120) and `103-dpi-150/run-20261005-163057-53020295` (DPI 144).
  - The frame grows to 952×649 and 1144×779, against 762×519 at 96: ×1.249/1.250 and ×1.501.
  - I looked at every PNG in both runs: initial, active, inactive, moved, narrow and restored. Active, inactive and restored are byte-identical within each run.
    - At both scales the layout is the 100% layout, scaled. The query line, the Pinned label with Ctrl 1–5, five Empty slots, Commands with the five sample rows, and the footer's hint, Open command ↵ and Actions Ctrl K are all present.
    - Text, glyphs, tiles and keycaps are crisp, with no clipping or overlap in the full-size window.
  - The narrow captures (459×517 and 552×621) ellipsize the descriptions, scroll the third row under the footer, and **clip the footer hint to a fragment of its ↵ cap**, as at 100%.
- **Committed copies are trimmed 6px a side** (`real-app/dpi{125,150}-{initial,narrow}-trimmed.png`). At scale, the capture script placed its backdrop from the logical `windowRect` (774×526), not the physical frame. So the DWM rim and up to 4px inside the raw crops showed slivers of the operator's desktop: coloured pixels and, at the top of the 125% crop, fragments of other windows. The trimmed copies have no such pixels within 3px of their edges.
  - This is a `capture-pane-windows.ps1` limitation at non-100% scale; I didn't fix it.
- **Claim:** the real window scales proportionally and renders legibly at 125% and 150%, reviewed by eye (R3). There is **no geometry parity** at those scales: the reference is pinned at 96 DPI.

## Still pending

None of these has run.

1. **Saturation.** `saturate(160%)` needs a coloured backdrop. `capture-pane-windows.ps1` has only grey patterns, so that is script work first, then a visible capture like the glass runs (about 10 s, Pane in the foreground).
2. **Real-process checks with no script:**
   - Settings' real minimize, maximize, drag and resize (#97);
   - a real restart that keeps the theme and material (#98) and the pins (#101);
   - a real arithmetic copy and a configured fallback (#96);
   - real clipboard copies (#102).

   Each needs the operator by hand or new scripted steps in `real-app-smoke.ps1`. Either way it shows `pane.exe`.
3. **Workbench at scale.** The follow-up above, before any parity at 125% or 150%.
4. **The outer shadow (R2).** It waits on the user's answer below.

## Open user questions

Collected from every ticket's results comment. None is answered yet.

- **All tickets:** how do local, unpushed commits count toward closing #91–#103? Until that is decided, every ticket stays open.
- **#92 and everything after it:** the glass, shadow, smoke and 125%/150% captures have run. The unscripted real-process checks remain; see [Still pending](#still-pending).
- **#92, the shadow (new):** the real window draws no outer shadow (R2). Pursue it (a DWM shadow attribute or style, or a painted shadow in a transparent margin), or accept its absence as a Windows adaptation?
- **#94:** should command and command-search rows lose their pointer fade? #95 has since given command rows root search's immediate washes, per #100. The question remains only for confirmation.
- **#96:**
  1. Restrict the answer card to the calculator?
  2. Is an accent ring only while the card is selected OK?
  3. Is the 80px production card OK, against the board's 160?
  4. Is the notice's "install an extension" line OK under the rule against advertising a store?
- **#97:**
  1. Should the sidebar show a keyboard-focus state?
  2. Keep the Extensions count?
  3. Add the palette, shield and info glyphs, or keep Pane's?
- **#98:**
  1. Should the preview draw the board's wallpaper as an illustration?
  2. Are sample rows, slots and a tip in the preview OK?
  3. Under an override, labels and segments dim to 40% but descriptions don't. Is that OK?
- **#99:**
  1. Recorders take clicks on their well only. OK?
  2. Pointer fades and press washes are removed from the Settings controls. OK?
  3. Should Extensions' destructive and Cancel answers get a danger tone or a button look?
  4. Is "Open documentation" OK?
- **#101:**
  1. Is the full-slots replacement as a text list OK?
  2. Keep the dashed Empty slot and the dimmed unavailable slot?
  3. Is the generic command tile for pinned applications OK?
  4. Re-pinning while a query is typed clears the query. OK?
  5. Should a Keyboard-page rebind to Ctrl+1–5 be refused?
- **#102:**
  1. Should the launcher grow to 940×600 for the clipboard view?
  2. Are Ctrl+K for Manage and Ctrl+D for Delete right?
  3. Is "Manage" the right label?
- **#103 (new):**
  1. The narrow launcher clips its footer hint mid-keycap. Should the hint drop its parts whole below some width?
  2. Should the light palette's measurement be extended to every family? Today it is validated only on the launcher frame and the Appearance page.
  3. Should the workbench gain scaled-display support (a cloaked or on-monitor fixture, plus a scaled reference)?

## Atlas: what the images show

I looked at every image listed here. In each side-by-side, Pane's capture is left of the magenta bar and the reference's right.

**Root launcher (side by side):**

- `root-rest--rest`:
  - The search header, the Pinned label with Ctrl 1–5, five slots with tiles, titles and per-slot caps, and the rows (Figma selected, then Clipboard History with `cb` and Ctrl Shift V, Left Half with Win Alt ←, Search Files) line up. The footer does too: the mark, "↵ opens instantly · Ctrl K for more", Open Application with a lime ↵, the rule and Actions.
  - Differences:
    - Pane says "Commands" where the reference says "Suggested · From your recent use" (D2);
    - the placeholder copy differs;
    - Pane lists a fifth row (Plugin Store, `store`) where the reference starts its "Commands" label.
- `root-hover--hover`: the pointer on Clipboard History selects it on both sides, and the footer reads Run Command. The reference's caret is not visible in this capture; Pane's is.
- `root-pointer-keys--down-under-pointer`: after Down with the pointer resting, Left Half carries the selected wash and Clipboard History keeps the faint hover-only wash, on both sides.
- `root-focus--focus-typed`: "clip" typed gives "Results · 1 match" with Clipboard History selected and "Clip" in the accent, the same on both sides. Pane's glass-free panel is flat where the reference shows its blurred backdrop band.
- `root-actions--open`:
  - The Actions panel opens over the list, with the dimmer, and the footer's Actions button is pressed on both sides.
  - Pane lists Run Command, then "Pane" with Pin to Quick Slot, Change Hotkey… and Change Alias…. The reference lists Assign Hotkey… and Add Alias… with key chords, and Hide from Results (D6).
  - Pane's panel is shorter, with fewer entries, and its entries show no chords except the primary's ↵.
- `actions-panel--open`:
  - On the static board (`fig`, Figma), Pane lists Open Application and Pin to Quick Slot only. The reference adds Open New Window, Show in File Manager, chords for every entry, Quit Figma in the danger colour, and Assign Hotkey and Add Alias (D6).
  - The reference's footer shows its tip and a plain ↵ (D7, D8).
- `keycap-windows--keycaps`: Pane's capture is the cap groups alone on the panel. Ctrl K, Win Alt ←, Ctrl Shift V, the lime ↵, the compact Ctrl 1, then the native-only Shift ↵ and Ctrl ↵ (lime) and Ctrl Shift P. The reference side is the root board, whose groups are compared in place.
- `launcher-frame--frame-crop-parity-frame-corner` (native above, reference below): a tiny crop of the top-left corner around the magnifier. At this size I can barely make out Pane's square corner against the reference's curve. D1 records the measured difference, 0 against 5px diagonal inset.
- `pinned-strip--slot-hover`: the pointer over Visual Studio Code lightens its slot on both sides by about the same amount.
- `empty-state--notice`:
  - The notice's disc and two lines, three fallbacks and the store block line up.
  - Pane:
    - says "Nothing matches" and "install an extension" (D13);
    - selects no fallback (D9);
    - highlights "kubectx" in the fallback titles (D10);
    - has no "Reorder in Settings" (D11);
    - keeps its hint and no primary (D12).
  - The store block is fixture content.
- `calculator-card--card`:
  - The card's box, lime ring, "72 in" and "182.88 cm" in Mono, the captions, the arrow disc, the rule and the "Also" chips match.
  - The recent calculations below are fixture content.
  - The footer hint differs (D14).

**Root launcher (native-only):**

- `root-unavailable--unavailable-selected-native`: the selected Clipboard History row grows to two lines, with "The extension that provides this command is disabled; turn it back on in Settings to run it here" in the warning colour, beside its alias and caps.
- `root-long-content--long-content-native`: a long title ellipsizes ("…own fixtu…") before the `long` alias chip and the caps, with no overlap.
- `launcher-frame-light--frame-light-native`: the same layout in the derived light palette: light panel, dark text, grey washes, the lime Enter cap.
- `launcher-frame-narrow--narrow-last-selected-native`: at 480×360, Settings (Ctrl ,) is selected and scrolled flush above the footer. **The footer hint is cut to "· Ctr" behind Run Command.**
- `pinned-partial--partial-native`: Terminal in slot 1; Empty dashed slots 2, 4 and 5; Obsidian dimmed in slot 3, with "Notes is disabled" in the warning colour. The dashes are faint at this size.
- `answer-long--long-answer-native`: "123456789 * 1000 + 98765 - 1" and "123456887764" step down to a smaller Mono size and fit their columns inside the ringed card. The footer reads Copy answer.

**Settings:**

- `settings-shell--hover`:
  - The titlebar, sidebar and search well line up, and General carries the hover wash on both sides.
  - Pane draws minimize, maximize and close; the reference has one close glyph.
  - Pane's page body is empty below the heading, because this scenario draws the shell only. The reference shows its Appearance page.
  - The Appearance, Privacy and About glyphs differ in shape.
- `appearance-page--rest`:
  - Material (Glass chosen), the accent swatches, the Blur and Tint sliders, Density, both toggles and the miniature launcher match in place.
  - Pane's preview stage has no wallpaper; the reference's has a grey gradient.
  - Pane's slider tracks are lime like the board's.
- `appearance-solid--solid`: Solid chosen; the note changes; Blur, Tint and their note dim on both sides. **Pane's dimmed sliders stay greenish where the board's are grey.** The miniature turns solid on both sides.
- `appearance-override--override-native`: the override notice in the warning colour ("PANE_THEME=dark and PANE_MATERIAL=opaque override…"), with both fields' labels and segments dimmed and their descriptions at full strength. The live preview sits to the right.
- `settings-general-recording--recording-native`: the Open Pane recorder listening ("Press the keys…" in the accent under a light ring), the long refusal in the danger colour over two lines, the launch-at-login switch on, and the tray row dimmed with its reason in the warning colour.
- `settings-launcher--open-native`: the Display select open. Its search well is focused; Primary display is highlighted with the accent dot; Pointer's display follows; Active window's display is dimmed with its reason. The popover covers the Reopening field.
- `form-validation--rejected-native`: "Greet someone", the Name well focused with "Enter a name" in the danger colour, the Greeting segments with Good morning chosen, Greet, and the footer status "Name: Enter a name".

**Clipboard:**

- `clipboard-rest--rest`: the header (back, chip, search, Pause), tabs, privacy caption, groups, rows with Mono times, the code preview with line numbers, and the footer (Paste to Obsidian, Copy, Actions) line up. The reference's content is fixture-only.
- `clipboard-previews--color`: the lime clip is selected and its swatch fills the card on both sides. **Pane's hex, rgb and hsl lines sit about 14px higher.** That is unmeasured and fixture-only.
- `clipboard-filter--no-match`: "No clips match. Try another filter." on both sides, no preview, "Nothing selected". Pane's footer shows only Actions (D18).
- `clipboard-production--rest-native`: production's text records under Today, Yesterday and Older; the caption "Text is kept for 7 days · copies marked private are skipped"; a plain-text preview; and the footer with Copy, Delete (Ctrl D) and Manage (Ctrl K).
- `clipboard-off--off-native`: history off. The header offers Turn on, the caption says nothing is kept, the list explains, there is no preview, and only Manage is in the footer.
