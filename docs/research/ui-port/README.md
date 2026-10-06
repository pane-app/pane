# Reference-faithful Pane UI on Windows

Research completed 2026-10-04; specification and tickets published after user approval. **No application implementation.**

Pane's UI differs because the previous work ported a restricted styling slice, retained several incompatible component recipes, and verified functionality rather than reference fidelity. Another palette adjustment will not fix it. The port needs exact control families, input-state behavior, complete screen composition, and a repeatable native/reference comparison.

Start with [the published specification and self-contained tickets](tickets.md). For implementation evidence, use the [native source audit](../ui-port-native-audit.md) and [all secondary boards](../ui-port-secondary-boards.md). The user approved the breakdown and delegated the remaining choices. [Specification #90](https://github.com/hoangvu12/pane/issues/90) and its native sub-issues are now authoritative; [the decision record](https://github.com/hoangvu12/pane/issues/100) records the settled contracts.

## Authority and research record

- Requested source: `.scratch/ui-reference/launcher.html`. Durable, byte-identical source: [retained launcher.html](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/evidence/ui-prototype/reference/launcher.html).
- Both files: **1,615,907 bytes**, SHA-256 **F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4**.
- Source audit checkout: **b7f1b881cc19b15e0c3c0febaf21565a79bf8271**. Product files were clean at research start; `.scratch/` already contained user/reference material.
- Read the root glossary, domain/tracker/triage/CI instructions, handoff/current decisions, renderer ADR 0003, prior UI interview, appearance/presentation/validation records, and specifications #61 and #70. The current user's Windows-only UI scope supersedes their historical all-OS execution gates for this milestone. No CI, guest build, release matrix or other-OS check ran.
- Opened the real reference in isolated Chrome with `use-browser`, through a loopback-only local server. Extracted all nine rendered boards, CSS, logic, default DOM geometry and screenshots. Then independently decompressed the source bundle to recover **all conditional templates**, rather than trusting only the visible DOM.
- [extract-reference.py](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/extract-reference.py) reproduces authored CSS, full templates and logic from the retained source and rejects a changed hash. [capture-reference.py](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/capture-reference.py) reproduces screenshots from an opened reference tab. Both are research tools, not product code.
- Launched the existing Windows executable once using the repository's isolated capture helper. [Native baseline](native-baseline/run-20261004-133320-1a2ec69f/00-initial-window.png) and [metadata](native-baseline/run-20261004-133320-1a2ec69f/pane-run.json) record binary hash, 96 DPI, dark/opaque appearance, foreground confirmation and cleanup. **The pre-existing executable's source revision is unverified.** Its screenshot is illustrative; current-code findings come from the pinned source audit. No fresh application build is claimed.
- Browser captures show the reference's default macOS labels. They are authored visual evidence, **not Windows-key-label goldens**. Future parity fixtures must use Windows labels and real effective Pane bindings. Measured board coordinates are CSS/logical pixels; native metadata distinguishes window/client/frame physical bounds.
- A [live interaction probe](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/interaction-probe.json) used browser pointer/keyboard events: hover selected Clipboard History, Down selected Left Half with the pointer stationary, one pixel of real pointer movement selected Clipboard again, Ctrl+K opened Actions, movement over Figma left the underlying target unchanged, and Escape closed the menu. Computed `.086` alpha is browser serialization of the authored `.085`; use the literal token. Temporary capture caches/data and raw bundled browser DOM were moved into `.scratch/ui-port-research/`; the saved capture metadata retains their original run paths.

## What was missed before

The [old reference summary](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/evidence/ui-prototype/reference/REFERENCE.md) explicitly left several boards unmined and reported panel height as unknown. The source actually specifies 64px search + 404px list + 50px footer = **518px**. The [earlier interview](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/evidence/ui-prototype/reference/ui-rework-interview.md) explicitly excluded pinned slots, Actions, Settings, Store and the snap HUD from its first slice. The [previous review](../../launcher-ui-review.md) assessed that smaller scope. Its success is not evidence of this requested port.

The native screenshot checker checks colors/text/presence, not corresponding reference components. A 460px panel, incorrect keycap font and wrong hover behavior can all pass. The current tests even assert the introduced pointer fade. New acceptance must compare the same data in the same states, rather than treating those old tests as a design oracle.

## Board-by-board coverage

| Board / screenshot | Literal target | Current production mapping | Port disposition |
|---|---|---|---|
| [Root](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/root-board.png) | 760×518; pinned grid; Suggested/Commands; rich rows; two-sided footer | Working root search, flat rows, 760×460, ellipsis/footer action | Direct style/state port; pinned and recent-use data need a small explicit contract |
| [Actions](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/actions-board.png) | Same root; 320px popup; selected-result header; action groups; bottom search | App-wide Settings popup only | New contextual UI backed by supported actions; do not relabel app menu as Actions |
| [Calculator](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/calculator-board.png) | 740×160 answer card; mono values; unit chips; recent rows | Existing computed result/copy action flattened to row | Use real supported calculation data; unit/history fixtures do not authorize new engines |
| [No results](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/empty-board.png) | 84px notice; fallback rows; optional extension suggestions | Plain message plus real fallback behavior | Port composition; preserve deliberate fallback selection; suggestions require real data |
| [Clipboard](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/clipboard-board.png) | 940×600; 360px list; preview; tabs; pause | Text-history extension with opt-in/retention/exclusions | Port supported text flow; new image/color/pin capabilities remain explicit decisions |
| [Snap HUD](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/window-manager-board.png) | 740×396; ten layouts; display selector; summary/chips | No snap feature | Catalogued complete source/UI recipe; separate future capability/UI milestone |
| [Store](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/store-board.png) | 1160×740; 208/568/384 columns | npm/Git/local installation, no store catalog | Catalogued complete source/UI recipe; no invented catalog or trust claims |
| [Settings Appearance](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/settings-board.png) | 1120×720; 48 titlebar; 232 sidebar; 388 controls/400 preview | Seven real pages in 740×530; different choices/preview | Port shell/control families; retain real pages and persistence; advanced controls need capability decision |
| [Design language](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/design-language-board.png) | Component/material/type examples | Partial theme/UI modules | Developer reference/workbench, not a new user-facing screen |

The user subsequently approved existing screens plus complete launcher UI and delegated remaining choices. Pins and supported text clipboard are included under the recorded contracts; store/HUD backends and advanced persistent Appearance controls are deferred. All nine boards remain in the inventory. There is **no claim that the proposed current-feature milestone alone completes every fictional feature shown on all nine boards**.

## Exact recipes the port must preserve

Full values and inline layout come from each `reference/<board>.css` and `<board>-template.html`, not a screenshot estimate. Read both: a stylesheet alone misses state callbacks, inline overrides, grid layout and conditional content.

| Family | Geometry / type | Surface / state |
|---|---|---|
| Root search | 64 high; horizontal pad20; icon20; gap14; input19/400, tracking−.005em | Input #F3F3F5; placeholder #86878C; accent caret #C9EE6A; separator white6% |
| Root body | 404 high; pad4/10/10; gap2; hidden scrollbar | Real scrolling and selected-item visibility retained |
| Root row | 44 high; radius10; horizontal pad10; gap12; icon28/r7; title14/500, subtitle13 | Rest transparent; CSS hover white3.5%; selected white8.5% plus inset white5%; **mouse movement normally selects the row** |
| Metadata | Kind12.5 with right-aligned min-width88; alias Mono11, pad2/6/r5; key group gap3 | Kind #8E8F94; alias #B9BABE with white14% inset edge; matched title spans accent |
| Section label | 30 high; pad8/10/0; gap8; font12/500, tracking .01em | #8E8F94; left title/right note |
| Pinned slot | 100 high; radius12; pad14/8/10; gap9; 42 icon/r11; label12.5/500; five equal columns/gap8 | Rest white3.5%/inset white5%; hover white7%; compact17px caps |
| Keycap | min-width20, height20, pad0/5, r5; Geist Mono11/500/line-height1 | White7% background, white8% inset edge, black35% **bottom** inset; #C9CACE; separate compact and lime Enter variants |
| Footer | 50 high; pad left16/right8; black14%; rule white6% | Left content; primary action; divider; contextual Actions; persistent errors must remain reachable |
| Footer button | 34 high; r8; pad0/8; gap6; 12.5/500 | Transparent → white6% hover; #D9DADD; Actions-open inline white10% |
| Contextual action | 36 high; r8; pad0/8; gap10; 13px | #E4E4E7; hover white6%; selected white11%; danger #FF9A92 |
| Sidebar item | 36 high; r8; pad0/10; gap10; 13/500 | #B3B4B9; hover white5%/#EDEDEF; selected white9%/white |
| Segmented choice | Track36 high/pad3/gap2/r10; segment30 high/r7,12.5/500 | Track black24%/edge white6%; active white12% + top white8% |
| Toggle | 40×24/r12; knob18×18/top3; left3 off/19 on | Track white16% off/accent on; white knob with short shadow; .2s background/left transition |

**Material recipes:** root/calculator/empty/clipboard tint `rgba(22,23,26,.70)`; Store/HUD .72; Settings .78. L1 sheen white5%→transparent at36%, blur44/saturate160%, radius18; inset border white7.5%, top highlight white10%, black75% outer .5px edge; external shadows `(0,50,120,-30,black72%)` and `(0,16,40,-16,black50%)`. L2 tint `rgba(38,39,43,.82)`, sheen6%→0 at40%, blur30/saturate160%, radius14, edge9%/top10%, outer black80% .5px, shadow `(0,28,70,-14,black75%)`. These are CSS targets, **not a proven GPUI/Windows compositor recipe**.

The decorative graphite/dusk/daylight wallpaper and SVG grain are outside the production app. Use them as external comparison backdrops. Painting that wallpaper inside Pane would fake translucency. (A picture the user chooses as the launcher's background image is a different thing: on the Solid material it is drawn on the opaque canvas; on Glass the canvas takes the glass tint's alpha and the picture is drawn at 84%, so the window's own frost shows through it. See [ADR 0028](../../adr/0028-the-launcher-draws-a-background-image-the-user-chooses.md).)

## Interaction fidelity, including hover

The root's `hover` callback selects on mouse movement while the contextual menu is closed. CSS selected style then dominates hover style. The same selected identity drives the footer and Enter. Keyboard navigation must not be undone by a stationary pointer; real movement changes selection again. Opening Actions freezes the underlying target until the menu closes. An unselected click first selects; a selected click invokes. The reference often selects before click because the mouse moved over the row. These distinctions need controlled event-sequence checks, not only screenshots.

| Current native behavior | Reference target / explicit handling |
|---|---|
| Root/button washes fade for150ms | Root row/footer CSS washes change immediately; preserve reference-specific timing |
| Root pointer hover paints .035 but leaves selection elsewhere | Actual movement selects row, .085 + edge; footer and Enter follow it |
| Root click always selects and invokes | Adopt reference select-then-invoke for click without prior movement; preserve single-click activation after pointer movement |
| Generic popup rows use root44/r10 geometry | Actions36/r8; separate sidebar36/r8; generic data rows44/r10 |
| All menu motion inherits prior Roboco-inspired policy | Source root hide uses opacity/scale .14s, target scale .98; Actions has no authored entrance tween; Settings toggle .2s; other screen transitions need an explicit adaptation decision |

This table follows the animation-review skill. Its general timing advice does not override the user's literal reference. Reduced-motion support should remain: endpoints and input behavior stay the same, decorative interpolation can be disabled. Existing native focus, accessibility, IME, cancellation, error and extension behavior must survive component extraction.

## Fonts and icons: verify actual assets

The earlier font identities were unresolved. They are now parsed from the original embedded WOFF2 payloads in [font metadata](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/font-metadata.json): two distinct payload hashes repeated under18 per-board asset IDs. Geist is variable100–900, version1.800; Geist Mono variable100–900, version1.701. The current native TTFs report Geist1.800 and Geist Mono1.700 in [native font metadata](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/native-font-metadata.json). This is a **provenance discrepancy to investigate**, not proof of a visible metric difference or a reason to replace all fonts. Verify family resolution,500/600 weight, glyph widths/baselines and mono coverage; correct keycaps currently inheriting14px Geist first.

Reference icons are stroke SVG with round caps/joins, width1.6; application glyphs width2. Row tiles28/r7, pinned42/r11, selected-menu mini18/r5, toast34/r9. App tiles have vertical gradients and different highlights from neutral command tiles. Map icons by stable identities/metadata, never localized titles. The full palette/path inventory is in [root logic](https://github.com/hoangvu12/pane/blob/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference/root.js). Unknown items need a deliberate neutral treatment, not a fabricated application identity.

## Component boundaries

Keep GPUI CE and the accepted `features/`, `extension_views/`, `ui/` organization. No toolkit migration or new crate is justified by this research.

1. `ui` owns visual recipes: semantic colors **per control family**, typography, spacing, geometry, material/elevation, icon tiles, keycap/key sequence, section header, result row, action row, sidebar item, footer button, fields/segments/toggles, preview surface and empty-state framing. Components accept display values and callbacks; they do not decide which command runs.
2. Feature adapters own stable IDs, sections, effective shortcut labels, current selection, supported action availability and navigation. Keep one selection identity for rows/footer/Actions; do not add a second component-local selection source.
3. Retain the existing editable text/IME entity and searchable select behavior. Restyle their chrome and expose the few missing slots. Avoid a second editor/dropdown state machine.
4. A narrow, read-only projection may expose result kind, alias, effective shortcut, match spans and computed-result display data. Keep GPUI types out of core and preserve result ranking and activation IDs. Existing `Row` is insufficient for full rich-row composition.
5. Existing extension-owned custom drawing stays extension-owned. Theme only host framing. Host forms, installation/confirmation screens, errors and all Settings pages must migrate through actual shared consumers; adding unused tokens is not extraction.

Detailed current entry points and line references are in the [native audit](../ui-port-native-audit.md). The current no-core-import claim is already violated by keycap's `pane_core::Binding`; move binding formatting into an adapter when separating keycap presentation.

## Decisions and limitations that cannot be hidden in a styling ticket

| Question | Evidence | Proposed disposition |
|---|---|---|
| Can Windows reproduce exact CSS blur/saturation/corners/shadows? | Native currently asks for Blurred and DWM round preference; CSS asks explicit filters/radius | Bounded Windows rendering proof before claiming full material parity; no silent approximation |
| Pinned slots and recent suggestions | Root reference fixtures exist; production lacks their data contract | Decide host-owned stable pins and suggestion source before production home ticket; no fake installed apps/recent-use claim |
| Contextual operations | Reference simulates new-window, quit, pin, hide, alias/hotkey | Offer supported commands only; decide additional capabilities separately; preserve app Settings entry |
| Fallback selection | Empty mock preselects; Pane requires deliberate selection | Preserve existing contract; capture selected visual after user selects |
| Advanced Appearance | Reference-only Frost/accent/blur/tint/density/toggles vs real theme/material settings | Port existing real controls first; decide each missing persistent setting; sliders must not suggest OS blur control that is unimplemented |
| Preview density | Mini32/38/46 vs root44 | Root board governs root; miniature is a distinct preview recipe |
| Clipboard content | Reference includes image/color/pins, production text history | Style supported text flow; capability decisions gate richer types |
| Store trust copy | Mock says sandbox/permission gating/no background processes | Do not ship those false claims; accepted trusted-extension contracts govern copy |
| HUD material | Design-language says no blur/r12; HUD actually blur44/r18 | Concrete HUD board governs that future screen |
| Light and narrow layouts | Reference is dark, fixed desktop stage; Pane already supports light/resizing | Derived adaptations, explicitly labeled; no invented claim of literal reference equivalence |
| Windows words and keys | Default mock macOS/“plugins”; Pane glossary says extensions | Adapt to Windows/effective bindings and domain vocabulary; record text-width differences |

Microsoft documents DWM rounding as a preference, with application/window constraints, not an arbitrary18px radius API. Its Acrylic material recipe combines blur/tint and other effects. These support investigating the native difference; they do **not** establish that substituting a WinUI brush into this GPUI app will reproduce CSS. Sources: [DWM rounded corners](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-rounded-corners), [Acrylic material](https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic), accessed2026-10-04. Local pinned material/renderer code remains the integration authority.

## Windows-only acceptance contract

Each implementation ticket carries its own local checks and visual evidence. Use **production components in a deterministic Windows fixture window** with reference text/data, plus a real feature-adapter scenario. Fixture-only matching is insufficient; arbitrary current user data cannot support pixel comparison.

- Match logical client size, DPI, font load, content, selection, scroll offset, keyboard focus, pointer position, appearance and backdrop. Record revision, binary hash, reference hash, OS/build, DPI and effective material. Compare content/client crops separately from outer frame/shadow. Never resize screenshots to hide geometry errors.
- Reference dark/100% is the canonical measured baseline. At each control family capture rest, hover, selected, selected+hover, pressed where authored, keyboard focus, disabled/unavailable, long text and scroll boundary. Include menus with filtering/zero actions, query empty/nonempty, errors and no results. Disabled/pressed cases absent from reference must be marked adaptations. (Superseded for pressed states by [446efd5](https://github.com/hoangvu12/pane/commit/446efd59a55edfe64f74fc53d3045131af1d8d00), at the user's direction: every pressable control now shows a pressed wash on pointer-down, its hover wash at double alpha with no fade, whether or not the reference authors one. That is a recorded adaptation; see [progress notes](progress-notes.md#pressed-washes-everywhere-2026-10-06). "Pressed where authored" above is the original contract, kept as history.)
- Proposed strict layout criterion: same declared dimensions; measured edges/baselines within **1 logical pixel**, without cumulative drift. Flat deterministic opaque colors within **2 channel levels** after matching compositing. These are proposed engineering thresholds, not reference-authored numbers. Text antialiasing may be separately masked only at glyph edges; control fill/edge/padding cannot be masked. Avoid a single global similarity percentage.
- Opaque comparisons establish geometry/type/state colors. Matched external graphite plus bright/dark edge-pattern backdrops establish glass appearance and blur. Reference opacity alone is not blur evidence. Keep screenshots at settled state and interaction samples immediately after events when checking timing.
- Use currently available display scales locally;100% measured here. Run125%/150% only if safely available without changing global settings; otherwise record not run. Narrow-screen adaptation has its own readable/keyboard-reachable acceptance, not a nonexistent narrow reference.
- Suggested scoped commands after implementation: `cargo fmt -p pane --check`; `cargo check -p pane --tests --locked -j 1`; `cargo test -p pane --test window --test command_search --locked -j 1`; add the affected Settings/control suite only. Build the local app when capturing changed source. Inventory existing test binaries before selecting them. These commands were **not** run during research.
- No macOS/Linux job, push-triggered CI, all-workspace release check or package publishing is an acceptance dependency. Existing unrelated platform tickets stay untouched. Product/functionality regressions on Windows remain required for the changed controls.

## Completion and handoff

Research supplies complete authored sources, all nine visual boards, source-level mismatch findings, a native baseline with honest provenance, proposed component contracts, explicit feature gaps, and reviewable ticket bodies. It does not certify a port, new feature behavior, exact Windows compositor capability or untested DPI configurations.

The approved breakdown is now [published on GitHub](tickets.md), with all 13 native sub-issues and 30 blocker links verified. The planning prerequisite is completed; 12 implementation tickets remain open. No prior parent or platform issue was closed or changed. The remaining decision-table proposals above are historical research context where superseded by [the approved contracts](approved-contracts.md); they are not unanswered approval requests.
