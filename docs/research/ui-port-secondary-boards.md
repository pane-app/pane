# Secondary reference boards: exact measurements and behavior limits

Research only, 2026-10-04. Windows UI scope. Source: `.scratch/ui-reference/launcher.html`, SHA-256 `F7E81E030E2216FE61509B9AFD98A68147D1998C73F0A168BAB02D0BF00F0BB4`. The checked-in [reference extracts](ui-port/reference/) preserve authored CSS, logic and captured markup. `data-dc-tpl="N"` below means the element with that attribute in the named HTML extract, not a production selector. The `{board}.html` files are default rendered markup; the `{board}-template.html` files preserve the full authored template, including conditional branches, reproducibly extracted by `ui-port/extract-reference.py`. Conditional values below come from those templates, not inferred screenshots. These are fixed 1440 × 900 CSS-pixel boards. They do not establish a responsive layout or Windows physical-pixel sizing policy.

## Board bounds and shared controls

Measured default rectangles, in `(left, top, width, height)` CSS pixels:

| Board | Main panel | Structure |
|---|---|---|
| Calculator | `(340,150,760,518)` | Search 64; content 404; footer 50 |
| No results | `(340,150,760,518)` | Search 64; content 404; footer 50 |
| Clipboard history | `(250,130,940,600)` | Search 64; tabs 46; body 438; footer 52 |
| Snap HUD | `(350,460,740,396)` | Bottom is 44px above board bottom; content determines height |
| Store | `(140,80,1160,740)` | Search 64; body 624; footer 52 |
| Settings | `(160,90,1120,720)` | Title bar 48; body 672; no launcher footer |

Bounds are both inline declarations and browser measurements, except the HUD's 396px height, which is computed. Sources: each board's `.html` `section.glass`, and associated `.css`. The desktop wallpaper, fake applications behind the HUD, and 1440 × 900 stage are presentation context, not production window children.

The calculator/empty/clipboard row uses 44px height, 10px horizontal padding, 12px gap, 10px radius; 28px square tile with 7px radius. Labels are 30px high with `8px 10px 0` padding, 12px/500 text, `#8E8F94`. Row hover is white at **3.5%**, selection white at **8.5%** with 1px inset white at 5%. Because `.row.sel` follows `.row:hover` at equal specificity, a selected row stays selected when hovered. Footer button hover is white 6%, height 34px, padding `0 8px`, gap 6, radius 8, text 12.5px/500. Keycaps: minimum width 20px, height 20px, horizontal padding 5px, radius 5, white 7% background, white 8% inset border, black 35% bottom inset, Geist Mono 11px/500; key groups gap 3px. These distinct alpha values must remain distinct tokens. Sources: [calculator.css](ui-port/reference/calculator.css), [empty.css](ui-port/reference/empty.css), [clipboard.css](ui-port/reference/clipboard.css), selectors `.row`, `.row:hover`, `.row.sel`, `.label`, `.fbtn`, `.kbd`, `.keys`.

## Calculator

[Markup](ui-port/reference/calculator.html), nodes 12–16: header padding `0 20px`, gap 14, 20px search glyph; body padding `4px 10px 10px`, gap 2. Search field is 19px/400, `#F3F3F5`, caret accent. Node 20 answer card measures **740 × 160**, at `(350,252)`; margin `2px 0 4px`, padding `20px 20px 16px`, column gap 14, radius 14, white 6% fill, 1px accent inset border. Node 21 uses `1fr 40px 1fr` columns with 16px gaps. Both values use Geist Mono 34px/500, tracking −0.03em; source value `#D9DADD`, answer white. Their unit labels use 12.5px `#8E8F94`. Center arrow sits in 40px circle with white 7% fill. Secondary unit line has 14px top padding and white 7% top separator. Chips are 30px high, padding `0 10px`, gap 6, radius 8, white 6% fill → white 10% hover.

Recent calculations are 44px rows; expressions and answers 13.5px Mono; kinds reserve 88px and align right. Footer is 50px with padding `0 8px 0 16px`, white 6% top rule, black 14% fill. Primary action says Copy Answer and Enter; the auxiliary paste hint uses Windows `Ctrl+Enter` when `platform='Windows'`.

**Actual demo behavior:** [calculator.js](ui-port/reference/calculator.js) changes only `state.query`. The displayed 72 inches → 182.88 centimeters, extra units, history and footer are fixed. It performs no calculation, copy, paste, history selection or action-panel opening. A UI ticket may reuse real computed-result data; it must not infer unit conversion, calculation history or paste-to-previous-application support from these fixtures. In particular, changing the input does not make this board an evaluator.

## No results

[Markup](ui-port/reference/empty.html), node 17: 84px high notice, padding `8px 12px 4px`, gap 16, 44px circular icon; notice title 15px/500 and explanation 13px. Three 44px fallback rows follow a 30px section label. Store suggestions use **54px** `.prow`, 10px horizontal padding, 12px gap, radius 10; tile overrides to 32px/radius8; title 14px/500; metadata 12.5px; Install pill 30px high, padding `0 12px`, radius8, white8% fill → white13% hover. The footer and search match calculator. Source [empty.css](ui-port/reference/empty.css), `.prow`, `.pill`.

**Actual demo behavior:** [empty.js](ui-port/reference/empty.js) updates only the input; the notice and all “kubectx” labels remain static, as do selection and suggestions. Web/file/script fallbacks, store suggestions, installs, See all and actions are nonfunctional illustrations.

**Product conflict:** the first fallback is styled selected and footer suggests Enter invokes Search Web. The root glossary/current decisions explicitly say fallbacks are never automatically selected and an otherwise empty root search does nothing on Enter until the user selects one. Recommendation: preserve that existing behavior; reproduce the selected visual only after deliberate selection. Do not silently change fallback invocation to match this screenshot. No-results wording must interpolate the real query, and suggestions require actual available results; no fake Kubernetes packages in production.

## Clipboard history

[Markup](ui-port/reference/clipboard.html), nodes 12–35: header padding `0 12px 0 14px`, gap12; back button 32px/radius8; command breadcrumb 30px high with 20px tile. Tabs strip 46px high, padding `0 14px`, gap4; tab 30px high, padding `0 12px`, radius8. Tab text `#9A9BA0`, hover white4%/`#EDEDEF`, selected white10%/white text with white6% inset border. Selected tab remains selected on hover because `.tab.on` follows `.tab:hover`.

Body grid is **360px + remaining 580px**. Left list padding `2px 8px 10px`, gap2, 1px right divider; actual row width **343px** (360 minus horizontal padding and border). Row title 13.5px/500, ellipsis; timestamp 11.5px Mono. Right pane padding12; default inset preview **556 × 414**, radius12, black24% fill and white7% inset border. Default code preview padding `26px 28px`, 14.5px Mono, line-height1.8, line-number width12 and gap18. List scrolls vertically with hidden scrollbar. The last default row extends below the visible list and must be reachable through scrolling, not clipped away permanently. Sources: [clipboard.css](ui-port/reference/clipboard.css), `.list`, `.tab`, `.row`; markup nodes 34–56 and 105.

[Logic](ui-port/reference/clipboard.js) defines eight fixture clips; filters All/Text/Links/Images/Colors; groups Pinned/Today/Yesterday. Search is case-insensitive substring over title, body and source application. The default selection is `code`. A filter excluding it selects the first visible item for display; zero items yields no selection. Clicking a row selects it, not copies it. Up/Down stay within bounds and scroll selected items into view with an 8px cushion. Escape clears query. Pause toggles only the icon, Resume/Pause label, accent tint and pressed state; it does not pause host observation. Copy/Paste, Actions and Back do not have operational implementations in this demo. The default captured markup contains the code branch; the complete [clipboard template](ui-port/reference/clipboard-template.html), `sc-if` branches `is.text`, `is.color`, `is.link`, `is.image`, `isEmpty` and `hasCur`, supplies the other variants below.

Literal conditional preview geometry from [clipboard-template.html](ui-port/reference/clipboard-template.html):

| Branch | Geometry and typography | Content / behavior limit |
|---|---|---|
| Plain text | Preview child padding `28px 30px`; 20px text, line-height 1.5, tracking −.005em, `#EDEDEF`; `white-space:pre-wrap`, `text-wrap:pretty`. | Renders `cur.body`; outer preview retains 556×414 geometry, radius 12 and clipping. There is no inner text scrollbar. |
| Color | Absolute inset 0, full-preview selected hex fill; vertical stack anchored to bottom; gap 6, padding `24px 26px`. Hex 32px/500 Mono with −.02em tracking and black 80% text; RGB/HSL 13px Mono, black 66%. | Renders fixture hex/RGB/HSL. This is color display, not evidence of a production color parser. |
| Link | Absolute inset 0; centered horizontal/vertical column; gap 10, padding 20. Tile 44×44/radius 11, `#4A2E17` fill/`#FFC285` foreground, 22px icon. Domain 22px/500 `#EDEDEF`; full URL 13px Mono `#A3A4A9`. | Domain and URL are text spans, not clickable links. Long URL wrapping/overflow is not specially handled. |
| Image | Absolute inset 0 centered column, gap 8; hatch uses 135° alternating 10px white 5% and 10px transparent over `#15161A`. Image icon 28×28; label 13px `#A3A4A9`; dimensions 12px Mono `#8E8F94`. | Literally displays `[Screenshot preview]` and fixture dimensions; no image element or screenshot decoding exists. |
| No matching clips | Left list message padding `40px 16px`, centered 13px `#8E8F94`: “No clips match. Try another filter.” | `hasCur=false` removes the entire right preview card, leaving its padded pane empty. Footer metadata becomes “Nothing selected”; Paste to Obsidian, Copy and Actions remain visually present and have no disabled binding or handler. |

The top privacy caption “Password managers are never recorded” is fixed reference copy. Production wording must reflect actual sensitive-marker and exclusion behavior rather than promise universal detection. The footer's “Paste to Obsidian” is also fixed fixture copy; the template does not resolve the previous application.

**Production boundary:** Pane's glossary and [clipboard ADR](../adr/0020-host-keeps-clipboard-history-for-an-extension.md) describe host-managed **text** history with opt-in, retention, exclusions and deletion semantics. Image capture, pinning, rich type detection, clipboard pasting, and source-application/time formatting need explicit capability mapping. A visual port cannot replace retained real records with fixtures or remove opt-in/deletion controls because the reference omits them. Reference-only preview types can be documented visual variants without claiming the host supports them.

## Settings: Appearance is the only specified page

[Markup](ui-port/reference/settings.html) nodes 11–60: title bar48; Close button32 at right10/top8. Sidebar **232px**, padding `12px 10px`, 2px gap, black10% fill, 1px right separator. Search34px/radius8, black24% fill, white6% inset border; navigation rows36px/radius8, padding `0 10px`, gap10, text13px/500. Nav rest text `#B3B4B9`, hover white5%/`#EDEDEF`, active white9%/white. Content padding `26px 32px 24px`, gap36: actual controls width388 and fixed preview width400. Heading22px/600 with −.01em tracking; subtitle13px. Field groups gap18, internal gap8; field label13.5px/500, description12.5px with line-height1.45.

| Field | Literal reference contract | Demo limit |
|---|---|---|
| Material | Glass/Frost/Solid; default Glass. Segmented group36px high, padding3, gap2, radius10, black24%. Button30px/radius7; selected white12% with white8% top inset. | Changes mini preview only. |
| Accent | Lime `#C9EE6A`, Ice `#8FD3FF`, Amber `#FFC46B`, Lilac `#C3B2FF`; 30px circles, 10px gap; selected ring 2px `#1A1B1E` then 4px accent. | Fifth Custom color button has no handler. |
| Blur | Integer 0–60, default44; native HTML range,20px high; accent-colored. | Solid sets both Blur and Tint inputs `disabled="{{isSolid}}"` and their containing groups to40% opacity; values remain in state. |
| Tint | Integer55–95, default70; native HTML range,20px high. | Preview alpha = tint/100 for Glass; tint/100 × .78 for Frost;1 for Solid. |
| Density | Compact/Default/Roomy; default Default; same segment styling. | Mini rows32/38/46px, not actual launcher density definitions. |
| Show pinned slots | Default true; switch40×24/radius12; knob18×18 top3; left19 on/3 off; white knob. | Hides only mini-preview slots. |
| Show tips in footer | Default true; identical switch. | Changes mini-preview hint text only. |

Source [settings.js](ui-port/reference/settings.js), constructor and `renderVals`; [settings.css](ui-port/reference/settings.css), `.segwrap`, `.seg`, `.sw`, `.range`, `.mrow`; markup nodes 61–99 and [complete settings template](ui-port/reference/settings-template.html), `input#blur` and `input#tint` disabled bindings. Switching back from Solid restores enabled controls with their retained numeric values. Switch track is accent on / white16% off; background and knob-left transition .2s with default CSS ease. Mini row height transitions .2s. No other interaction animation is declared here.

The mini preview stage is400×520/radius16; launcher miniature340 wide/radius14, top inset56, header46, footer38; selected row white9%. Solid uses no blur, base `22,23,26` alpha1. Frost uses base `74,76,84` with alpha multiplier. The outer settings shell remains its fixed material regardless of mini-preview settings. No persistence or application-wide theme change exists. General, Hotkeys & Aliases, Plugins, Window Manager, Clipboard, Privacy, About and Search settings are visual navigation only; no content or handlers are supplied. “Changes apply instantly,” OS reduce-transparency following and theme-store integration are copy, not implemented behavior. Recommendation: retain real existing settings pages, style their controls consistently, and specify persistence/app-wide propagation separately if accepted; do not invent entire missing pages from their sidebar labels.

## Store (catalogued UI slice; backend is outside this port)

[Markup](ui-port/reference/store.html), nodes 26–78: three columns **208 / 568 / 384px**. Sidebar padding10/gap2, same36px nav as settings. List padding `4px 10px 10px`, gap2; row548×64, padding `0 12px`, gap12, radius12, hover3.5% white, selected8.5% plus5% border. Install/Open/Update pill min-width70, height30, padding `0 12px`, radius8. Here pills are spans inside row buttons: **there is no `.pill:hover` rule**, unlike empty-board install buttons. Open pill background transparent, text `#A3A4A9`. Detail pane padding `18px 20px 16px`, gap16; icon52/radius13; command rows34px, gap10; alias chip11px Mono/padding `2px 6px`/radius5/white14% border. Permission rows padding `11px 14px`, gap12, separated by white6% rules. Footer52. Source [store.css](ui-port/reference/store.css).

[store.js](ui-port/reference/store.js) returns seven fixed packages and selects Git Pulls statically. Search, categories, selection, install, update, source links and actions do not have backend handlers. “Reviewed by the Pane team,” download sizes, authors, version, command aliases, permissions, source link and counts are fictional fixture metadata.

**Product conflicts:** “Asks for” permissions, “Never sees,” “Every plugin runs sandboxed,” “No background process” and system-keychain promises cannot be adopted from this board. Pane's trusted-code model, ungated network use, optional helpers and scheduled/continuing extension behavior have different contracts. Replace or omit unsupported claims while preserving visual placement for actual metadata. “Plugin” is reference wording; repository vocabulary uses “extension.” UI tickets must explicitly record any text change from the reference rather than calling it pixel-identical text.

## Window-manager HUD (catalogued UI slice; native snapping outside this port)

[Markup](ui-port/reference/window-manager.html), nodes 75–124 and [CSS](ui-port/reference/window-manager.css): panel740 wide; header padding `14px 14px 12px 16px`; app tile32/radius8. Display selector buttons28px/radius7 with padding `0 10px`; selector wrapper padding2/gap2. Grid is five columns, two rows, gap8, horizontal padding14. Each layout card **136 × 88**, radius12, vertical gap9, miniature monitor66×42/radius6. Monitor's inner region uses percent geometry plus3px position/minus6px size. Label12px/500. Selected card white10%/accent1px inset, selected diagram accent, selected label white; others white4%/white5% border and `#A3A4A9` label. `.thumb:hover` declares white8%, but inline background on every generated card takes precedence, so the **effective hover background remains the inline value**. Keyboard focus has a2px white55% outline with1px offset. Do not infer an effective hover from the CSS rule alone.

Selection summary52px high/padding `0 16px`; layout shortcuts strip padding `10px 14px 12px 16px`, gap8; chip32px high/radius8. Footer42px, black14% fill and white6% top border. [Logic](ui-port/reference/window-manager.js) supports ten selections: halves, maximize, three thirds, left two-thirds and center. Default index8 is Left Two-Thirds. Arrows move ±1 horizontally and ±5 vertically, clamped across the flattened ten-element list (not geometric wrapping). Clicking a card updates preview and selection only. Reported size is derived from hardcoded1440×900, not Windows monitor/work-area detection. Windows modifier labels are Win+Alt. Display selection, Edit layouts, saved layouts, confirm/cancel, window moving and global hotkeys are illustrations. Fake editor/browser windows and full-screen snap overlay are reference scenery, not evidence of native behavior.

## Design-language disagreements that tickets must resolve explicitly

[Design-language board](ui-port/reference/design-language.txt) gives useful named layers and tokens, but does not override every literal board:

1. L1 says70% tint. Actual `.glass`: calculator/empty/clipboard70%, HUD/store72%, settings78%. Recommendation: named material variants, not one global alpha substituted everywhere.
2. L3 says “Toast & HUD,”88% tint/radius12/no blur. Actual snap HUD uses L1-like blur44/saturate160/radius18/tint72%. Recommendation: use the actual snap board for snap layout; use L3 only for its illustrated toast.
3. Subtitle/meta/footer summary says12.5px/400, but root/empty subtitles are13px and footer buttons12.5px/500. Clipboard titles13.5px, store rows64px, empty store recommendations54px, normal rows44px. Preserve semantic size variants.
4. Settings mini density Default38px differs from real launcher44px. Mini preview is scaled/condensed illustration, not authority for replacing production default row height.
5. Frost at minimum tint computes .55×.78=.429, contradicting blanket “tint never drops below55%.” Claimed4.5:1 on any wallpaper is not established by code or measurements. Need actual contrast/native-material validation if making that claim.
6. Design-language swatch's rendered alpha may serialize .086 although label says8.5%; CSS literal `.row.sel` is .085. Keep source literal, do not round-trip computed color into token drift.
7. Reference defaults to macOS. Apply Windows props/key labels before capture comparisons; do not reproduce Command glyphs on Windows. This is a Windows-only verification requirement, not a cross-OS testing ticket.

Recommended future acceptance matrix: default+hover+selected+selected-hover+keyboard-focus for each control class; selected visibility while scrolling; empty/filter/no-selection states; actual Windows modifier widths; all settings values and disabled states; three wallpaper backdrops when validating glass; screenshot evidence at known scale. These are proposed implementation acceptance requirements, not checks run by this secondary audit. Reference-source extracts and browser-derived geometry were read; no application implementation or CI was run here.
