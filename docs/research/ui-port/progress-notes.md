# UI port (#90) progress — branch impl/ui-91 (local only, never pushed)

Handoff: %TEMP%/pane-ui-90-handoff.md. One signed-off commit per ticket (`git commit -s`), ticket number in subject.
Never stage .scratch/, docs/research/ui-port/, docs/research/ui-port-native-audit.md, docs/research/ui-port-secondary-boards.md.

| Ticket | Status | Commit | Open findings |
|---|---|---|---|
| #91 workbench | done locally (79b313a), issue open | 79b313a | ci.md not updated for pane-visual-fixture bin (CI out of scope) |
| #92 frame + material | committed, comment posted, OPEN (glass/shadow on-screen capture not run; real-app smoke not run) | a084715 | evidence docs/evidence/ui-92 |
| #93 type/tiles/keycaps | committed, comment posted, OPEN (42/18 tile consumers in #101/#95) | 554c4ef | evidence docs/evidence/ui-93; tracking not applicable (GPUI) |
| #94 rows + selection | committed, comment posted, OPEN (unpushed) | d5e22d9 | evidence docs/evidence/ui-94 (run-94d: harness 1647/0, 155/0; parity 2415/86/30). Question for user: command/command-search rows keep their pointer fade? |
| #95 footer + Actions | committed, comment posted, OPEN (unpushed) | ffc86ae | evidence docs/evidence/ui-95 (run-95h: harness 2314/0, 183/0; parity 3580/67/39). Residual: Actions label 2px (layout rounding) |
| #96 empty + calculator | committed, comment posted, OPEN (unpushed) | b3474a3 (+8785794 green) | evidence docs/evidence/ui-96 (run-w1d: harness-native 310/0, harness-ref 59/0, parity 455/0/22 acc). Open box: return-to-normal-row focus/scroll not shown; no window test drives the notice. Comment https://github.com/hoangvu12/pane/issues/96#issuecomment-5986800668 |
| #97 Settings shell | committed, comment posted, OPEN (unpushed) | f69ac68 (+150ce57 green) | evidence docs/evidence/ui-97 (harness-native 181/0, harness-ref 58/0, parity 295/0/6 acc; nav-selected-fill flips 6). Open boxes: nav keyboard-focus state (user Q); real min/max/drag/resize not run. Page body empty in capture until #98. Comment https://github.com/hoangvu12/pane/issues/97#issuecomment-5986800921 |
| #98 Appearance | committed, comment posted, OPEN (unpushed) | 510b7b8 (+8cfad9e green, fc97ff7 evidence) | evidence docs/evidence/ui-98 (harness-native 781/0, harness-ref 126/0, parity 691/0/6 acc = #97 shell dispositions; segment-on-fill flips 8). Open boxes: preview wallpaper + sample content (user Q1/Q2), real pane.exe restart not run. Residuals: narrow preview below fold (scroll not captured); mini file-tile tint and slot glyphs not compared. Comment https://github.com/hoangvu12/pane/issues/98#issuecomment-5987461239 |
| #99 shared controls | committed, comment posted, OPEN (unpushed) | 40e1b5d (+35dae93 green, 500efe6 evidence) | evidence docs/evidence/ui-99 (8 native-only scenarios: harness-native 656/0; run-w3 whole: harness-ref 534/0, harness-native 7197/0, parity 10459/0/131 acc; new well-fill perturbation flips 8). Residuals: no Shortcuts/custom-view-frame scenario, no frame-ring test; one-cap recorder wells look empty on the left. User Qs 1-4 (recorder well-only clicks, no fades/press washes, confirm danger tone, "Open documentation"). Comment https://github.com/hoangvu12/pane/issues/99#issuecomment-5988328502 |
| #101 pins | committed, comment posted, OPEN (unpushed) | 774245b (+8785794 green) | evidence docs/evidence/ui-101 (pinned + 7 root scenarios: harness-native 3882/0, harness-ref 200/0, parity 7618/0/77 acc; row tops all pass now). Open box: real pane.exe restart with pins not run (core tests only). Comment https://github.com/hoangvu12/pane/issues/101#issuecomment-5986801197 |
| #102 clipboard | committed, comment posted, OPEN (unpushed) | a3cc4c7 (+55882ba green) | evidence docs/evidence/ui-102 (harness-native 520/0, harness-ref 71/0, parity 832/0/11 acc). Unmeasured: fixture colour-preview text ~10-14px high vs reference (fixture-only). Long-text scroll not captured. Comment https://github.com/hoangvu12/pane/issues/102#issuecomment-5986801482 |
| #103 acceptance | evidence committed, comment posted, OPEN (unpushed) | 6102aa6 guards + 582b73a evidence | evidence docs/evidence/ui-103 (run-103: harness-native 7197/0, harness-ref 836/0, parity 10504/0/131; ledger 73 rows: 17 pass/20 acc/25 native-only/11 not covered). Boxes open: coverage (glass, real-window states), real restart, #92 material dispositions. Pending consent: glass/shadow capture, real-app smoke (+ unscripted real checks), 125/150% DPI. Comment https://github.com/hoangvu12/pane/issues/103#issuecomment-5988835048 |

## Decisions so far
- #92: Windows corners stay DWM-rounded (DWMWCP_ROUND, documented 8px) because acrylic (accent 4) blurs the
  whole window rect, so a painted 18px radius shows a frost plate; a transparent window + painted radius would
  leave DWM's rectangular shadow/rim around rounded corners. Glass/shadow on-screen evidence needs a visible
  capture => not run without operator consent (non-interactive session); ticket stays open on those boxes.

## Questions to put to the user at the end
- Consent for a short visible (non-activating) glass/shadow capture for #92's matched-backdrop evidence.
- How unpushed local commits count for closing tickets.
- #101: full-slots replacement is a text list ("Replace Slot n: title") in the Actions panel, not a slot grid - OK?
- #101: dashed "Empty" outline and half-opacity unavailable tile with reason in warning colour are Pane's own design - keep?
- #101: pinned apps show the generic command tile - OK?
- #101: re-pinning while a query is typed clears the query to focus the slot - OK?
- #101: a Keyboard-page rebind to Ctrl+1-5 wins over the slot chords - refuse such rebinds?
- #102: should the launcher grow to 940x600 while the clipboard view shows?
- #102: Ctrl+K -> Manage and Ctrl+D -> Delete right keys? (Ctrl+K opens Actions on root.)
- #102: is "Manage" the right label where the reference says "Actions"?
- #96: every copying computed answer becomes a card, not only the calculator's - restrict?
- #96: accent ring shows only while the card is selected - OK?
- #96: production card is 80px tall, not 160 - OK?
- #96: notice's "install an extension" line OK given the ban on advertising a store?
- #97: should the sections list show a keyboard-focus state? Reference has none.
- #97: keep the Extensions count in the sidebar?
- #97: add the palette/shield/info glyphs or keep Pane's?

## Tips learned
- Run the workbench from the PowerShell tool with absolute paths (bash `powershell -File` leaves $PSScriptRoot empty).
- Python edit scripts: use %TEMP%/ed.py (File(path).rep/save) - preserves line endings; plain open('w') writes CRLF.
- Latest workbench run dir: .scratch/visual-workbench/run-93d (harness 618/0, 125/0; parity 1042/109/2 accepted).
- editable text element (gpui_elements) hardcodes letter_spacing None -> query tracking -.005em not applicable (fork).

## #95 plan (2026-10-05)
Core (TDD):
1. KeyboardAction::OpenActions ("open-actions"), default ctrl-k (cmd-k on macOS: ctrl-k protected there).
2. launcher/actions.rs: Launcher::result_actions() -> Option<ResultActions{target id, title, items}> for root search's selected row:
   primary (selected_action label/available) + "Pane" group: Assign/Change Hotkey… (catalog hotkey_editable), Add/Change Alias… (catalog editable).
   No pin/quit/new-window/hide (pin = #101). Launcher::open_result_action(target, Hotkey|Alias) verifies target still selected+eligible, opens show_hotkey/show_alias_form.
   Matching filter (case-insensitive label contains) in core.
UI:
3. ui footer composition shared w/ fixture: fbtn 34/r8/pad8/gap6/12.5 500, transparent rest, hover 6%; primary w/ accent caps; divider 1x16 10%; Actions button Ctrl K, open = bg 10% + white.
   Left: app-menu button (Pane mark) = Settings menu (Windows/Pane adaptation, documented) + hint "↵ opens instantly · Ctrl K for more" / open: "Type to filter actions · Esc goes back"; status replaces hint (bounded wrap/scroll kept).
4. features/actions_panel.rs: popup 320, right10 bottom58, r14, .pop material; header 30 (18/r5 tile); body pad6 gap1; arow 36/r8/gap10/pad8/13 450 #e4e4e7 icon #a3a4a9; hover 6% sel 11%; alabel 26; sep 1 m4x6 7%; search 44 border-top 7%; empty "No actions match"; dimmer rgba(6,7,8,.34) top64..bottom50.
   Keys: arrows/Enter/Escape(close only)/Ctrl K toggle; typing filters; outside click dismisses (consumed); restore query focus; target frozen; stale target can't invoke.
5. Command/command-search rows drop pointer fade (#100: share visuals).
6. Workbench: actions board (static fig) + root interactive menu states (open over Clipboard History, filter, empty); compare popup/footer.

## Wave plan (2026-10-05, user: parallel subagents in worktrees, no Rust runs until a wave is done)
- Wave 1: #96, #97, #101, #102 in parallel worktrees from ffc86ae; agents write code+tests, no cargo/workbench, commit -s in worktree.
- Integration: cherry-pick each onto impl/ui-91 one by one, then cargo fmt/check/tests + workbench serially in the main checkout; evidence + issue comments after.
- Wave 1 commits: #97 7d20c0f -> cherry-picked 1ecd8c1 (check clean; settings 54/54 incl. old frame-loop fix; launcher_settings 2 fails being fixed by #97 agent in main checkout),
  #96 c85d100 (wt agent-a9fdbfc...), #102 abcb533 (wt agent-a5bf703...), #101 pending (wt agent-a8c4ab6...).
  NOTE: Agent worktrees are created at main b7f1b88; agents reset to ffc86ae. Background review agents of subagents report to main session -> tell agents to run them foreground.
- Integrated: #97 = f69ac68 (launcher_settings test fixes amended: settle arrival; pointer move before outside click; both failed at ffc86ae too per agent). settings 54/54, launcher_settings 27/27, settings_search 8/8, shortcuts/open_pane/keyboard ok.
  #96 = b3474a3 (conflicts resolved via %TEMP%/resolve96.py; window test 'Copy answer' button assertion dropped: footer hides primary while status shows). core search/calculator, lib, window 85, aliases, command_search ok.
  #102 = a3cc4c7 (resolver agent; dup Glyph::Clock removed). clipboard_view 11/11, lib 47, window 91/91 (wheel test flaked once under load).
  #101 rebased by its agent -> 863e4fc -> cherry-picked 9b41035; uncommitted: cargo fmt + borrow fix in visual_fixture.rs (shift_group) -> amend into 9b41035 after full suite.
  Workbench run-w1a (at a3cc4c7, subset, no sensitivity): harness-native 77 fail, harness-ref 3, parity 122. Key: root-actions 'closed' capture still shows panel open (regression since #95);
  calculator-card ref card rect off 2-4px; clipboard row wash 8px wider; settings search fill alpha 47 vs 61; empty-state invoke keycap missing. Fix serially in main checkout after #101 lands.
  Gotcha: a worktree agent building with CARGO_TARGET_DIR=main target baked worktree paths into test bins -> `cargo clean -p pane -p pane-core -p pane-target` fixed.
- Wave 2: #98. Wave 3: #99. Wave 4: #103.

## Full-suite run after wave 1 (9b41035 + fmt/borrow fix)
- #101-caused: install.rs x3 (row rendered @284), settings.rs footer menu Tab -> 'Quick slot 1', window.rs gone-slot remove -> fix agent working in main checkout, amends 9b41035.
- Pre-existing at ffc86ae (verified in .scratch/wt-head): pane-core helpers.rs 6-7 process tests ('the helper did not start'). Flaky under load: window wheel test, core command_search reloading_the_package_stops_its_search.

## Known pre-existing failures
- settings.rs moving_up_the_sidebar_arrives_from_above + rapid_section_switches_retarget_the_arrival_from_where_it_is: 'the window never stopped asking for animation frames'; fail at 554c4ef (#93), d5e22d9 (#94) and with #95. Not caused by #95. Settings shell = #97 territory; report to user.

## #95 status (2026-10-05, late)
- Core: KeyboardAction::OpenActions (ctrl-k/cmd-k, alternate ctrl-shift-k); parse fixed (record first, then free defaults; also fixes moved-binding records).
  launcher/actions.rs: result_actions / result_action_ready / open_result_action; flows return to the search (epoch-guarded snapshot).
- UI: ui/footer.rs (mark button, hint, fbtn, divider, row), features/actions_panel.rs (compose/PanelView), app menu = Pane mark ("Pane menu").
- Kerning: Typography.features = kern (GPUI Windows omits kern; Geist 2% wider). Applied at window roots + editable fields + fixture shaping.
- GPUI orders shadows by unblurred box -> footer_row has 0.1% black fill to sort above popup drop shadow.
- Workbench: Step::Click, scenarios root-actions + actions-panel (static board), reference focuses .pop input after click (board runtime lacks refs).
- Residual: footer-actions label ink left 2px in open captures (native Ctrl cap 37 vs Chrome 36.4 -> button 1.4px wider). Row tops (#101) known.
- run-95f subset: harness 487/0, parity 863/15(12 row tops + 3 label)/11. Full run-95g in progress.
- Pre-existing failing settings tests (sidebar transitions) unchanged.

## Root-family workbench green (8785794, 2026-10-05)
- run-w1c (full, sensitivity ok): harness-ref 407/0, harness-native 5716/44, parity 9701/49/118 accepted. All remaining failures clipboard-* (harness 36, parity 40) and settings-* (harness 4, parity 12).
- Fixes: compare crop crash (off-image crops), app tile ring 1px@14%, unavailable slot fits (unavailable_gap 2), Pinned label caps tracking, static-board caret at end; many compare.py measurement fixes (local edges, per-column key alpha, coverage glyph/text). New dispositions: SPRITE_CONTRAST (GPUI mono-sprite contrast), ACTIONS_LABEL_COLOR (boards disagree on open label color).
- Sensitivity: row inset 482, selected 8, hover 2, nav 6 (selected/hover lower than #95 also with HEAD compare.py on same captures: #101 layout).

## Wave 1 done (2026-10-05)
- Commits on impl/ui-91 (local, unpushed): f69ac68 #97, b3474a3 #96, a3cc4c7 #102, 774245b #101, 8785794 root green, 150ce57 settings green, 55882ba clipboard green, bfc86e2 evidence (docs/evidence/ui-96, ui-97, ui-101, ui-102).
- Workbench run-w1d at 55882ba (full, sensitivity, clean tree): harness-ref 408/0, harness-native 5760/0, parity 9768/0/125 acc. Sensitivity: row inset 8 wash-left (482 checks total), selected 8, hover 2, nav 6. Fixture SHA 8921F47A...D0BE.
- Tests after fixes: window 100, settings 54, launcher_settings 27, settings_search 8, install 12, pane lib 51, core quick_slots 14, result_actions 9, clipboard_view 11, search 22, calculator 14, shortcuts 23, open_pane 12, keyboard 19; fmt clean.
  Known non-wave: core helpers.rs 6-7 process tests (also at ffc86ae); flakes under load: window mouse-wheel test, core command_search reloading test.
- Evidence folders carry report.json.gz FILTERED to each ticket's scenarios (runTotals = whole run).
- Results comments posted on #96/#97/#101/#102; issues left OPEN, labels/assignees untouched.
- Not verified anywhere in wave 1: 125%/150% DPI, real pane.exe smoke, glass/shadow on-screen capture (needs consent), real restart for pins.
- Next: wave 2 = #98 (Appearance; pending scenario appearance-page).

## Wave 2: #98 green (2026-10-05)
- 8cfad9e: compare.py measurement fixes only (68 first-run failures all measurement): tracks via local_edges(3 levels); segment wash vs track fill just left of each segment, white overlay in light; overlay_limit = 2 channel levels in alpha units; is_caret (light accent_text); mini_row_wash; mini pins via local_edges; descriptions first 160px; ringed_box no grow in light.
- run-w2 at 8cfad9e (full, sensitivity, clean tree): harness-ref 534/0, harness-native 6541/0, parity 10459/0/131 acc. Sensitivity: row inset 8, selected 8, hover 2, nav 6, segment-on-fill 8. Fixture SHA 0CFDFB2F...36BA2. Non-appearance scenario counts identical to run-w1d.
- fc97ff7 evidence docs/evidence/ui-98. Questions on #98: preview wallpaper illustration? sample preview content OK? override dims labels/segments to 40% but not descriptions OK?
- Next: wave 3 = #99.

## Wave 3: #99 green (2026-10-05)
- Port 40e1b5d (amended: dead result_row()/Glyph::Download/Copy + svgs removed; ui/shell.rs From<&pane_core::Section> moved to app::section_label -> ui/ has no pane_core;
  3 #99 tests fixed: form test settles view transition; popup-exit test clicks popup∩segment; settings click_row scrolls row into view).
- 35dae93 green: 6 harness-native fails were measurement (heading LSB K/L/E/D 1.76px via first_glyph_bearing; sidebar fill under open select popover shadow -> sidebar_rows bottom 100 when selectOpen);
  fixture form footer status now production ui::footer::status_message (extracted from app.rs); new perturbation well-fill (settings-keyboard).
- run-w3 at 35dae93 (full, sensitivity, clean tree): harness-ref 534/0, harness-native 7197/0, parity 10459/0/131 acc. Sensitivity: row inset 8, selected 8, hover 2, nav 6, segment-on-fill 8, well-fill 8. Fixture SHA 40413E16...1A1F. Non-#99 scenario counts identical to run-w2.
- 500efe6 evidence docs/evidence/ui-99. Tests at 35dae93: lib 58, settings 62, launcher_settings 27, shortcuts 24, keyboard 19, window 101, settings_search 8, open_pane 12, tray 9, install 12; python 56.
- Next: wave 4 = #103.

## Wave 4: #103 (2026-10-05)
- Full `cargo test -p pane-core -p pane --locked -j 1 --no-fail-fast` at 500efe6: 68 targets, 1453 passed, 0 failed (helpers.rs 44/44 this time; no flake hit). Log .scratch/test-103.log.
- 6102aa6 (scripts only): guards for #103 box 2 - freshness.py (fixture hash + sources digest sidecar target/debug/pane-visual-fixture.inputs.json; -SkipBuild verifies), reference fonts per board (check_reference_fonts), reference pointer parked + recorded (check_reference_pointer), parity client/device scale. test_freshness.py 8, ReferenceGuards 8; python 72.
- run-103 at 6102aa6 (full, sensitivity, clean): harness-native 7197/0, harness-ref 836/0 (534 + 257 fonts + 45 pointer), parity 10504/0/131 (+45 device scale). Fixture F8C509A7...994F; pane.exe A5A9317B...989A (cargo test relinks pane.exe with test features: rebuild before hashing).
- run-103-light (-Theme light -SkipSensitivity -SkipBuild): harness-native 6740/59 (dark-tuned measurement families; unclassified), parity meaningless.
- 582b73a evidence docs/evidence/ui-103 (README, ledger.md generated by %TEMP%/ledger103.py, atlas 29 images reviewed).
- Found in review: narrow 480px footer hint clipped mid-keycap ("Ctr") - ui/footer.rs hint overflow_hidden (#95); not fixed, asked user.
- Pending consent commands are in docs/evidence/ui-103/README.md.
- Coordinator ran (user OK "no games rn"): window-crop glass/opaque captures (.scratch/ui-captures/103-*) and real-app smoke (run-103-smoke, all 3 asserts pass). Recorded in 10e4d85 (docs/evidence/ui-103/real-app, R1: blur present, see-through ~0.25 vs CSS 0.30, DWM corner ~8px, acrylic holds inactive; saturation + outer shadow unmeasured). Ledger 74 rows 18/21/25/10.
  Follow-ups: #103 https://github.com/hoangvu12/pane/issues/103#issuecomment-5991530268 ; #92 https://github.com/hoangvu12/pane/issues/92#issuecomment-5991530767
  Still pending: 125/150% DPI, -FullScreen shadow (+ coloured backdrop for saturation: script work), unscripted real checks (Settings window ops, restart persistence, calculator copy/fallback, clipboard).
- Coordinator ran (user "just run"): -FullScreen shadow (103-shadow-*) and real pane.exe at 125/150% (103-dpi-*; scale restored to 100%). Recorded in 4505545: R2 open deviation = NO outer shadow (1px mid-grey DWM rim only; follow-up for #92 + user question); R3 = 125/150 proportional (x1.25/x1.50) and legible, reviewed by eye, no parity. Fixture workbench at 125% fails (parked off-monitor window keeps 96-DPI sizing) -> follow-up (cloak/on-monitor or per-monitor DPI + scaled reference). capture-pane-windows.ps1 at scale places backdrop from logical windowRect -> rim shows desktop; committed DPI crops trimmed 6px. Full-screen PNGs NOT committed (show user apps).
  Ledger 76 rows: 18 pass/21 acc/25 native-only/10 not covered/1 open (R2)/1 reviewed (R3). Comments: #103 https://github.com/hoangvu12/pane/issues/103#issuecomment-5991866498 ; #92 https://github.com/hoangvu12/pane/issues/92#issuecomment-5991867420
  Remaining: saturation (needs coloured backdrop: script work), unscripted real checks (Settings window ops, restart persistence, calculator copy/fallback, clipboard), workbench at scale, user decision on shadow.

## Pressed washes everywhere (2026-10-06)
- Adaptation, at the user's direction (overrides README "pressed where authored"): every pressable control (buttons, icon buttons, list items, menu/select rows, group headers, sidebar items, footer buttons, pins/slots, Actions/menu entries, clipboard tabs/rows, caption buttons, result rows) shows a pressed wash on pointer-down. The reference authors none of these (the footer menu button keeps its own open wash while pressed).
- Derived, no new tokens: `ui::theme::pressed(hover)` = the control's hover wash at double alpha (a selected item doubles its selected wash; the answer card its own fill). Instant: no transition. Settings caption buttons lost their 150ms pointer fade (GPUI fades a property the same way in every state), so `motion::pointer_fade`/`POINTER_FADE`/`ease_css` are gone.
