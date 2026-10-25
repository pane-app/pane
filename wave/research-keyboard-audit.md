# Keyboard audit: Pane vs Raycast (2026-10-24)

Read-only audit of `pane-app/pane` at `main` (069bfab0), Windows PC. Nothing was built or edited. Code references are `crates/pane/src/...` unless stated. Raycast facts come from a web-research pass, with URLs. **UNCONFIRMED** marks anything no source states.

## Summary

**Appendix A** lists every mouse handler, tooltip and native menu item. There are 163 mouse actions:
- 82 have a direct keyboard route;
- 16 have only a partial or indirect one;
- 59 have none: 48 in Settings, 8 tooltips, 3 in designed views;
- 6 are canvas events whose keyboard route depends on the extension.

The launcher window itself has no action without a route.

- **The launcher is keyboard-first already.** Key contexts are bound throughout:
  - root search
  - Actions panel and submenus
  - confirmations (Enter/Escape/Space; Tab stays in the dialog)
  - toasts (Ctrl+T details, focusable close button, Escape)
  - footer menu
  - designed forms and controls (focus ring, Enter/Space)
  - Grid (2D arrows)
  - split views
  - pins (Ctrl+Shift+F, Ctrl+Alt+arrows, Ctrl+1..5)

  The launcher gaps are small.
- **Settings is half keyboard, half mouse.** These work by keyboard:
  - the sidebar (Up/Down through every page and every extension's page)
  - the search (Ctrl+F, arrows, Enter)
  - text fields and selects
  - the hotkey recorders and alias cells
  - Appearance's segments

  These are **not focusable at all** and are mouse-only:
  - every switch
  - every plain button
  - every list row
  - every popover menu
  - every segment outside Appearance
  - every flow screen (the uninstall confirmation, install preview, update results)

  The controls in `ui/controls.rs` are "presentation only" and leave focus to the caller (`ui/controls.rs:7-11`). Most callers attach only `on_click`.
- **The user's case: there is no keyboard path to uninstall an extension.**
  - In Settings, "Actions…" is a click-only ghost button (`features/settings/extensions.rs:2396-2404`), and its menu has no key handling (`:2405-2505`). The "Uninstall X?" confirmation it opens is drawn as click-only rows (`:975-998`).
  - Escape on that confirmation closes the whole Settings window, because no control there takes Escape (`features/settings/mod.rs:140-143`, `:491-498`).
  - Root search's Actions panel has no Uninstall, Disable or always-present Configure Extension entry (`crates/pane-core/src/launcher/actions.rs:245-331`).
  - Settings search finds "Uninstall X", but Enter only opens the page (`extensions.rs:412-427`).
- **Raycast is not a high bar for Settings.** Its manual documents Settings with the mouse ("click the Uninstall button, or right-click"). Its keyboard route is the Action Panel in root search: Uninstall Extension, Disable Command, Configure Command/Extension (⇧⌘, / Ctrl+Shift+,), and Set Hotkey/Alias submenus. Pane should copy that route and also make its own Settings fully operable. ADR 0035 already says "a control that takes the keyboard, such as a Settings control or a field, keeps its focus ring".

---

## Part 1: Raycast's keyboard model

Raycast v2 is one product on macOS and Windows; the manual's Settings page covers both ([settings](https://manual.raycast.com/settings)). Windows is at v2.9 ([Windows changelog](https://www.raycast.com/changelog/windows)). I found no Reddit or GitHub reports about keyboard use in Settings, so the evidence is the official docs and changelogs.

### Settings window

| Topic | What Raycast does | Source |
|---|---|---|
| Open | ⌘, / Ctrl+, opens Settings. ⇧⌘, / Ctrl+Shift+, opens the selected root item's settings. | [keyboard-shortcuts](https://manual.raycast.com/keyboard-shortcuts), [settings](https://manual.raycast.com/settings) |
| Moving between tabs | No documented Ctrl+Tab, ⌘1..9 or ⌘[ / ]. **UNCONFIRMED / no evidence.** The documented fast path is Settings search: ⌘F / Ctrl+F, "select a result to jump straight to the right place". v2 tabs are Account, General, Launcher, Shortcuts, Keyboard, Advanced, Organizations, About, AI, Applications and Extensions, and each installed extension has its own sidebar entry. | [macOS beta 0.59](https://www.raycast.com/changelog/macos-beta/0-59), [Windows 0.59](https://www.raycast.com/changelog/windows/3), [settings.md](https://manual.raycast.com/settings.md) |
| Extensions list and arrows | Arrows, Left/Right expand and Enter in the extension/command tree are not documented. **UNCONFIRMED.** v2.7 "fixed command rows expanding and highlighting again after you'd already navigated to them" shows rows expand, not by which key. Windows 0.37 added Emacs/Vim navigation "in supported areas (lists, grids)". | [macOS changelog](https://www.raycast.com/changelog), [Windows p5](https://www.raycast.com/changelog/windows/5) |
| Action Panel in Settings | Not documented. **UNCONFIRMED** that ⌘K works in Settings. The documented controls are mouse ones: Uninstall button, right-click → Uninstall, per-command enable toggle, Record Hotkey button, alias field. Windows 0.52 added "Edit, Duplicate, Disable, Delete actions in settings" for Quicklinks, without saying how they are reached. | [settings](https://manual.raycast.com/settings), [Windows p3](https://www.raycast.com/changelog/windows/3) |
| Recording a hotkey | Click Record Hotkey; a popover records. It auto-saves after about 1.5 s, or ↵ saves at once. **Backspace while recording clears** (Windows 0.49). A conflicting chord turns the recorder red and names the owner. Alias: "Press ↵ or click away to save". Starting a recording with Enter/Space on a focused cell is **UNCONFIRMED** (the docs say "click"). Escape inside the recorder is **UNCONFIRMED**. | [aliases & hotkeys](https://manual.raycast.com/command-aliases-and-hotkeys.md), [Windows p4](https://www.raycast.com/changelog/windows/4) |
| Escape | Settings → Keyboard → "Escape Key Closes Window", on by default (Windows 0.53). Two fixes: "Fixed Escape closing the window instead of dismissing an opened alert" (Windows 0.64), and "Fixed Escape inside a Settings dialog closing the whole Settings window instead of the dialog" (Windows 2.3). **The innermost thing closes first.** Whether Escape clears the Settings search first is **UNCONFIRMED**. | [settings.md](https://manual.raycast.com/settings.md), [Windows p2](https://www.raycast.com/changelog/windows/2), [Windows changelog](https://www.raycast.com/changelog/windows) |
| Focus rings, Tab order, toggles, dropdowns | Not documented. **UNCONFIRMED.** Indirect evidence: "Fixed focus management in BYOK dialog" (Windows 0.67), and "Enter wouldn't create new line in text areas inside dialogs" (Windows 0.55). | [Windows p2](https://www.raycast.com/changelog/windows/2), [Windows p3](https://www.raycast.com/changelog/windows/3) |
| Managing extensions from root search | See the bullets below this table. | see below |

Root-search Action Panel actions for managing extensions:

- **Uninstall Extension** is an Action Panel action on any of the extension's commands ([v1 store](https://manual.raycast.com/v1/store)).
- **Configure Command** is ⇧⌘, / Ctrl+Shift+, and **Configure Extension** is ⌥⌘, / Ctrl+Alt+, ([action-panel](https://manual.raycast.com/action-panel)). The pages disagree slightly on these keys.
- The **Configure Command submenu** holds Set Hotkey, Set Alias, Delete Hotkey and Delete Alias ([aliases & hotkeys](https://manual.raycast.com/command-aliases-and-hotkeys.md)).
- **Disable Command** is ⇧⌘D / Ctrl+Shift+D, or ⌃⇧⌘D per another page; API 1.48 moved it to ⌘⌥⇧D ([developer changelog](https://developers.raycast.com/misc/changelog)).
- A "Manage Extensions" command: **UNCONFIRMED**, none found.

### Launcher

| Topic | What Raycast does | Source |
|---|---|---|
| Action Panel | ⌘K / Ctrl+K opens and closes it, and typing filters it. ↵ primary, ⌘↵ / Ctrl+↵ secondary, ⌥↵ / Ctrl+Shift+↵ tertiary. Esc closes it or leaves a submenu. Submenus have their own shortcut and filtering. Extensions can't claim ⌘K, ⌘W or ⌘Esc. | [keyboard-shortcuts](https://manual.raycast.com/keyboard-shortcuts), [action-panel API](https://developers.raycast.com/api-reference/user-interface/action-panel), [developer changelog](https://developers.raycast.com/misc/changelog) |
| Forms | ⌘↵ / Ctrl+↵ submits **from any field**, Tab / ⇧Tab move between fields, Esc cancels. Fields have `autoFocus`. Dropdown has a filter field. Opening a dropdown, Space on a checkbox and date-picker keys are **UNCONFIRMED**. | [keyboard-shortcuts](https://manual.raycast.com/keyboard-shortcuts), [form API](https://developers.raycast.com/api-reference/user-interface/form) |
| List/Grid dropdown | "Invoke it either by using the global shortcut ⌘P or by clicking on it". Ctrl+P on Windows is **UNCONFIRMED**. | [list API](https://developers.raycast.com/api-reference/user-interface/list), [grid API](https://developers.raycast.com/api-reference/user-interface/grid) |
| Lists, grid, detail, back | ↑↓ move; ⌥↑↓ / Alt+↑↓ page; ⌘↑↓ / Ctrl+↑↓ jump sections; Page Up/Down (v2.7). Esc goes back; ⌘Esc / Shift+Esc pops to root. Grid 2D arrows and keyboard scrolling of the detail are **UNCONFIRMED**. | [keyboard-shortcuts](https://manual.raycast.com/keyboard-shortcuts), [macOS changelog](https://www.raycast.com/changelog) |
| Toasts | `primaryAction` and `secondaryAction` take a `shortcut`. API 1.104.2 "Fixed platform-specific shortcuts on toast actions" suggests they work from the keyboard. A toast action panel exists (Windows 0.52). v2.9 made in-window toasts and HUDs dismissable. A generic toast key is **UNCONFIRMED**. | [toast API](https://developers.raycast.com/api-reference/feedback/toast), [developer changelog](https://developers.raycast.com/misc/changelog), [macOS changelog](https://www.raycast.com/changelog) |
| Dialogs and confirmations | `confirmAlert` has a primary and a dismiss action, a Destructive style, and "Do not show this message again". Enter confirms / Esc cancels is **UNCONFIRMED** in the docs; the Windows fixes above imply Escape dismisses. | [alert API](https://developers.raycast.com/api-reference/feedback/alert) |
| Favorites and pins | Add/remove ⌘F / ⇧⌘F. The move shortcut differs by source: ⇧⌘↑↓, ⌘↑↓, or ⌘⌥↑↓ (the API's `Common.MoveUp/MoveDown` is Ctrl+Alt+↑↓ on Windows). Windows 0.58 "Ensured move favorite / move pinned actions use the same shortcuts". | [keyboard-shortcuts](https://manual.raycast.com/keyboard-shortcuts), [action-panel](https://manual.raycast.com/action-panel), [keyboard API](https://developers.raycast.com/api-reference/keyboard), `docs/research/raycast-favorites.md` |
| Number keys | Ctrl+1..9 runs the nth item in all lists and menus (Windows 0.33), including compact mode (0.59) and the numpad (2.6). | [Windows p5](https://www.raycast.com/changelog/windows/5), [Windows p3](https://www.raycast.com/changelog/windows/3) |
| First run | Onboarding introduces the features and lets you choose the global hotkey (Alt+Space suggested). Whether it is fully keyboard-operable is **UNCONFIRMED**. | [manual windows/settings](https://manual.raycast.com/windows/settings), [quickstart](https://manual.raycast.com/quickstart.md) |

**What to copy.** Raycast's documented keyboard model for managing extensions is the **Action Panel on a root-search command**: Uninstall Extension, Disable Command, Configure Command/Extension with fixed chords, and Set Hotkey/Alias. In Settings the pattern is: Ctrl+F search, Escape that closes the innermost thing first, and Backspace that clears a recorder. Making every Settings control focusable goes beyond what Raycast documents, but the user asked for it and ADR 0035 already commits to focus rings on Settings controls.

---

## Part 2: Pane, screen by screen

### 2.1 Settings shell (`features/settings/mod.rs`, `search.rs`, `ui/settings_shell.rs`)

**Works by keyboard today:**
- Ctrl+, from the launcher opens Settings (`crates/pane-core/src/keyboard.rs:259`).
- The sidebar list holds focus on open (`mod.rs:221-222`). Up/Down walk every page and every installed extension's page (`mod.rs:117-120`, `:312-364`).
- Tab and Shift+Tab traverse tab stops (`:125-126`). Ctrl+W closes (`:131-139`). Escape closes when "Escape closes Settings" is on, which is the default (`:143`, `:491-498`; `pane-core/src/host_settings.rs:388`).
- Ctrl+F focuses the search. In the search, Up/Down/Enter work, and Escape clears the query, then leaves (`search.rs:94-114`).
- A control that loses its element hands focus back to the sidebar (`mod.rs:253`).

**Needs the mouse, or is weak:**
- **Enter on the sidebar does nothing** (`mod.rs:113-116`). Nothing moves the keyboard from the sidebar into the page except Tab, and Tab skips every non-focusable control.
- No key switches pages while focus is inside a page. Up/Down are handled only on the sections list (`mod.rs:592-593`), so a page control must Shift+Tab back to the sidebar first.
- The caption buttons are click-only (`mod.rs:803-878`). This is normal, since Ctrl+W and Alt+F4 cover them.
- **Escape is not layered.** A control that doesn't bind Escape lets it reach `EscapeSettings`, which closes the window. That includes the Actions… popover, the + menu and every flow screen. Raycast fixed the same bug twice (Windows 0.64 and 2.3).
- No visible focus state on the sidebar list itself. The selection wash stands in for it; acceptable, but there is no ring when focus returns there.

**Raycast:** Ctrl+F search and Escape-closes are the same. Raycast closes the innermost dialog first. Page-switching keys are UNCONFIRMED.

### 2.2 Settings › Extensions (`features/settings/extensions.rs`)

**Works by keyboard today:**
- **Selecting an extension:** sidebar Up/Down, or Ctrl+F, type the title, Enter (`extensions.rs:309-320`).
- **Preferences:**
  - Text, password and path wells are tab stops with a focus ring (`:2938-2951`, `:3005`).
  - Dropdown preferences are the shared Select: Enter/Space/Down opens it; then arrows, Home/End, Enter, Escape, Tab (`ui/select.rs:153-163`).
  - The Capabilities provider selects are the same control (`:1880-1941`).
- **A command's alias and hotkey:** the Shortcuts page's cells, reused (`:2662-2668`). They are tab stops; Enter/Space edits or records, Escape cancels, Clear is a tab stop (`shortcuts.rs:133-162`).
- **The npm/Git install field** takes focus when opened (`:3501-3516`).

**Needs the mouse (no focus handle; only `on_click`):**

| Control | Where |
|---|---|
| + menu button and its three items (Install from Folder/npm/Git…) | `:590-599`, `:605-619`; popover closes only on mouse-down outside, `:648-685` |
| Group page: Installed list rows (open an extension) | `:1497-1513` |
| Group page: runtime and kept-data rows | `:1567-1580` |
| Group page: "Update extensions automatically" switch | `:1593-1603` |
| Group page: Check for updates button | `:1626-1633` |
| Official Extensions → Install | `:1687-1699` |
| Install from Folder…/npm…/Git… buttons | `:1735-1742` |
| Show Package (and **Enter in the install field does not submit**: no binding; `submit_install` is called only from the click) | `:1783-1789`, `:3523` |
| Extension page: **Enabled** switch (disable/enable extension) | `:2026-2044` |
| Extension page: Update automatically switch | `:2049-2059` |
| Extension page: **Actions…** button | `:2396-2404` |
| Actions menu items: Check for Update, Reload, Clear Cache, Reset Confirmations, Retry/details, Show Source Folder, **Uninstall** | `:2409-2493` (popover `:2494-2505`) |
| Requirements "fix" buttons | `:2155-2161` |
| Per-command **enable switch** | `:2698-2711` |
| Per-command **Fallback** switch | `:2683-2692` |
| "Not active; click to forget it" rows | `:2577-2589` |
| Checkbox preferences (switch) | `:2843-2853` |
| File/folder/app preference "Choose…"/"Add…" | `:2884-2890` |
| **Flow screens drawn in place of the page**: the uninstall confirmation ("Uninstall X?" → keep / delete data / Cancel), disable-what-others-require, the install preview's Install row, details screens' Retry | rows `:975-998` |
| Flow screens' Back button | `:1000-1013` |
| Collection choice (#308): row preview, tick box, Install row, Back | `:1128-1149`, `:1243-1265`, `:1159-1178`, `:1180-1193` |
| Update results rows and Back | `:1316-1331`, `:1339-1349` |

The tests drive these by click: `crates/pane/tests/confirmations.rs:385-398` and `npm.rs:199-200` click `extension-menu`.

**The result for the user's case.** To uninstall without the mouse, he would need to:

1. reach the Actions… button (impossible),
2. reach Uninstall in its menu (impossible), and
3. answer "Uninstall X?" (impossible; Escape there closes Settings and leaves the launcher holding the confirmation screen).

The same is true for disabling an extension or a command, reload, clear cache and check for update.

**Raycast:** a sidebar entry per extension, with Uninstall reached by button or right-click (mouse), and **Uninstall Extension / Disable Command / Configure Extension in root search's Action Panel** (keyboard). Its in-Settings keyboard model is UNCONFIRMED.

### 2.3 Settings › Shortcuts (`features/settings/shortcuts.rs`)

**Works by keyboard today** (the best page):
- The filter, the group headers (Enter/Space toggle), the alias cells (Enter/Space edit; Enter commits, Escape cancels), the hotkey cells (Enter/Space record; Escape/Tab cancel) and Clear are all tab stops with focus rings (`:29-33`, `:133-162`, `:1128-1136`, `:1422-1427`, `:1593-1600`).
- While a recorder listens, Up/Down/Ctrl+W/Ctrl+F are captured as keys, not actions (`mod.rs:97-109`).

**Gaps:**
- **Backspace while recording does not clear.** The key goes to `Shortcut::new` and is refused or recorded as a chord (`:664-692`). Raycast: Backspace clears (Windows 0.49).
- No Enter-to-save-now. Pane saves on the first valid chord, which is fine.

### 2.4 Settings › General (`features/settings/general.rs`)

**Works:** the Open Pane recorder (focusable; Enter/Space record, Escape/Tab cancel; `:130-137`, `:471-495`). The Appearance section's theme and material segments (Enter/Space; focus ring, `appearance.rs:55-56`, `:434-441`, `:580-587`) are the model to copy.

**Needs the mouse:**
- the Reset button inside the recorder (`:496-509`)
- "Use the Windows key" (`:510-524`)
- the Launch at login, Tray and Taskbar switches (`:525-570`)

The code says so: "the reset and the switches take no focus of their own" (`:446-447`).

### 2.5 Settings › Launcher (`features/settings/launcher.rs`)

**Works:** the monitor, reopening and sensitivity selects (`:629-646`).

**Needs the mouse:**
- the Learn switch (`:649-677`)
- the Window mode and Pinned layout segments (`:689-702`, drawn `:817-839` without focus)
- the Compact pinned switch (`:703-711`)
- Reset ranking… → Reset / Cancel (`:754-807`)
- Reset search history… → Reset / Cancel (`:879-931`)

### 2.6 Settings › Keyboard (`features/settings/keyboard.rs`)

**Works:** each action's recorder (`:91-98`, `:512-552`), the navigation-bindings select, and the game-program field (`:869-884`).

**Needs the mouse:**
- each recorder's Reset (`:553-562`)
- the Escape behaviour segments (`:563-569`, `:608-630`)
- "Escape closes Settings" (`:570-578`)
- the Game mode switch (`:810-830`)
- each program's Remove (`:849-859`)
- Add (`:886-891`); **Enter in the program field does not add**

### 2.7 Settings › File Search (`features/settings/file_search.rs`) and About (`about.rs`)

- **File Search:** the pattern field is focusable (`:757`). Every button is click-only (`:473`, `:580`, `:598`, `:672`, `:708`, `:726`, `:772`, `:824`, `:917`).
- **About:** every control is click-only (`about.rs:262-330`): Check for updates, Install update, Documentation, Copy diagnostics, Log folder.

### 2.8 Launcher: root search, Actions panel, pins, footer

**Works:**
- Up/Down, five-row and section jumps, Page Up/Down, Enter / Ctrl+Enter / Ctrl+Shift+Enter, Ctrl+K, Escape back, Backspace back, Shift+Escape to root, Ctrl+, Settings, all rebindable (`keyboard.rs:67-163`, `lib.rs:76-87`).
- Ctrl+1..5 for quick slots, Ctrl+Shift+F pin/unpin, Ctrl+Alt+arrows move a pin (`keyboard.rs:215-247`), matching Raycast's `Common.MoveUp/Down`.
- The Actions panel filters as you type, with arrows, Enter, Escape stepping out of a submenu, and Tab closing (`features/actions_panel.rs:185-200`).
- The footer menu has arrows, Enter, Escape and Tab (`footer_menu.rs:72-83`).
- The Manage Extensions root row opens Settings at the group (`pane-core/src/launcher.rs:5164`).

**Gaps against Raycast:**
- **The Actions panel has no Uninstall Extension, Disable Command or Disable Extension.** Configure Command… and Configure Extension… appear only when the command or package declares preferences (`pane-core/src/launcher/actions.rs:298-315`), so an extension without preferences has no "open its page" action.
- No chord for Configure Command/Extension. Raycast uses Ctrl+Shift+, and Ctrl+Alt+,. Pane's Ctrl+, opens Settings at General, not at the selected command.
- Assign Hotkey… and Add Alias… have no chords (Raycast's have none by default either: **UNCONFIRMED**).

### 2.9 Confirmations, dialogs and toasts

**Works:**
- **The launcher's confirmation dialog:** Enter confirms, Escape dismisses, Space ticks "Don't ask again", and Tab stays in the dialog (`features/confirmation.rs:60-70`). This matches Raycast.
- **Toasts:**
  - Ctrl+T opens the details and actions popover for any footer toast (`toast.rs:113-132`, `:446-473`). Inside it, Up/Down/Enter choose an action and Escape closes.
  - The close (×) and details (^) controls are always tab stops with Enter/Space, and Escape dismisses while the toast has focus (`toast.rs:20-36`, `:778-782`).
  - An action's own shortcut runs while the toast shows.

**Gaps:**
- No direct "dismiss toast" key; it takes Tab to the toast first. Minor; Raycast's equivalent is UNCONFIRMED.
- Settings' own confirmations are not dialogs:
  - the extension flow screens (2.2)
  - the Launcher page's Reset ranking and Reset history "arm, then Reset / Cancel" rows (2.5)

  Both are click-only.

### 2.10 Designed views: List, Grid, Detail, Canvas, forms (`extension_views/designed/*`)

**Works:**
- Every interactive component is a tab stop with a focus ring: buttons, links, toggle, checkbox, segmented, slider, select, inputs, date, tags and paths (`designed/mod.rs:28-37`, `:145-190`, `components.rs:299-1424`).
- Enter submits from a single-line field, and Ctrl+Enter from a text area (`mod.rs:184-185`, `fields.rs:11-17`).
- **Grid:** Up/Down move by a row of cells, Left/Right by a cell (`app.rs:672-735`, `:1854-1861`).
- **Canvas:** arrows, Home/End and the semantic keys work (`mod.rs:165-183`).
- **List dropdown:** reachable by Tab, since it is the shared Select (`list.rs:497-528`).
- **Back:** Escape pops the stack, and Backspace in an empty field does too (`list.rs:15-17`).

**Gaps:**
- **Ctrl+Enter submits only from text fields and text areas.** It is bound in `DesignedInput` and `DesignedTextArea` only (`mod.rs:184-185`). From a focused checkbox, toggle, select, date, tag or path picker it is unbound there; whether the window's secondary-action chord submits instead is **unverified**. Raycast: ⌘↵ / Ctrl+↵ submits from any field.
- The submit chord is hard-coded `ctrl-enter` even on macOS (`mod.rs:184-185`). Raycast's is ⌘↵ there.
- **No shortcut opens a List or Grid dropdown.** Raycast: ⌘P.
- Grid Left/Right ignore key repeat (`app.rs:1860-1869`). Minor.
- The detail pane has no keyboard scrolling. **UNCONFIRMED** for Raycast too, so low priority.

### 2.11 Clipboard History and Search Files split views

**Works:**
- Up/Down, Page Up/Down, Enter paste, Ctrl+Enter copy, Ctrl+D delete, Ctrl+K actions, Escape clears then leaves (`clipboard_history.rs:17-45`, `:109-125`).
- Search Files reuses root search's keys (`search_files.rs:1-30`).
- The type dropdown is the shared Select, reachable by Tab.

**Gaps:**
- No Ctrl+P to open the type dropdown.
- The **"Open File Search Settings" link is click-only** (`search_files.rs:498-504`, `:549-555`), with no Actions-panel equivalent.
- Ctrl+D is `ctrl-d` on macOS too (`clipboard_history.rs:92`). Minor.

### 2.12 Top bar and back button, update item, install and preview flows, first setup

- **The back button** (`root_search/mod.rs:333-370`) is deliberately not focusable; Escape is the key, and its tooltip names it. This matches Raycast.
- **The update item** and launcher-side install previews, update results and details screens are launcher rows, so Enter/arrows/Escape work. Only their Settings-side copies are mouse-only (2.2).
- **First setup in the launcher** is the choice screen: Space ticks (`lib.rs:87`, `app.rs:737-745`), and Enter previews or installs.
- First setup does not offer to set the Open Pane hotkey. Raycast's onboarding does; whether it is keyboard-operable is UNCONFIRMED. Low priority.

### 2.13 Focus rings

- **Rings are drawn** on: Settings wells, recorders, the Select trigger, Shortcuts headers, cells and Clear, Appearance segments, designed controls, the toast controls, and pins (`focus_visible`). The shared ring is `controls::focus_ring` (`ui/controls.rs:239-244`).
- **No ring is possible** wherever there is no focus handle: all switches (`controls::toggle`, `:180-210`), `controls::button` / `ghost_button` / `icon_button` (`:380-428`, `:664-671`), `list_item` (`:680-714`), `menu_row` used as popover items (`:752-824`), and the Launcher and Keyboard pages' segments.

---

## Part 3: Fix plan, in priority order

Titles follow the repo's commit style: plain sentences. Each item names its files. "Quick win" means about a day or less, reusing an existing pattern.

### 1. Make the extension page's switches, Actions menu and install controls work from the keyboard

- **Problem:** the user can't disable, reload or uninstall an extension, turn a command off, or install from the group page without the mouse (2.2).
- **Raycast behaviour to copy:**
  - Every control is reachable.
  - The innermost popup takes Escape.
  - Recorders and aliases already match.
- **Files:**
  - `features/settings/extensions.rs`: the + menu, Actions…, menu rows, switches, list rows, buttons, Choose…, fix and forget rows
  - `ui/controls.rs`: a focusable switch and button helper with `focus_visible` ring, or reuse the pattern in `designed/components.rs:1061-1096`
  - `features/settings/mod.rs`: key bindings
- **Acceptance criteria:**
  - On an extension's page, Tab reaches Enabled, Update automatically, Actions…, every preference control, each command's alias, hotkey, Fallback and enable switch, and the Choose… and fix buttons.
  - Enter/Space toggle a switch or press a button, and each shows the focus ring.
  - Actions… opens with Enter/Space/Down and focuses the first enabled item. Up/Down/Home/End move, Enter runs, and Escape or Tab closes the menu and returns focus to Actions… without closing Settings. The + menu works the same way.
  - On the group page, the Installed rows, Install buttons and Show Package are tab stops, and **Enter in the npm/Git field shows the package**.
  - A window test drives "open the page → Actions… → Uninstall" with keys only.

### 2. Answer extension confirmations and previews in Settings with Enter, arrows and Escape

- **Problem:** "Uninstall X?", disable-what-others-require, the install preview, the collection choice and the update results are click-only rows in Settings. Escape there closes the whole window and leaves the launcher holding the screen (2.1, 2.2).
- **Raycast behaviour to copy:** Escape dismisses the dialog, not the window (Windows 0.64 and 2.3 fixes). A destructive answer needs an explicit choice.
- **Files:**
  - `features/settings/extensions.rs`: `flow_screen`, `choice_screen`, `update_results_screen`, `answer`
  - `features/settings/mod.rs`: `escape_settings` ordering
  - Possibly `pane-core` `Launcher::select` / `back` (already exist)
- **Acceptance criteria:**
  - When a flow screen shows, its list takes focus with the launcher's selected row highlighted.
  - Up/Down move the selection, which mirrors `Launcher::select`. Enter activates, as `answer` does. Space ticks a collection choice. Escape runs Cancel on a confirmation, or Back on a details, preview or results screen, and never closes Settings while a flow screen shows.
  - Back is a tab stop.
  - A test uninstalls an extension with "keep saved data" using only Up/Down/Enter, and a second test checks that Escape on the confirmation cancels and keeps Settings open.

### 3. Add Uninstall Extension, Disable Command and Configure Extension to the Actions panel (quick win)

- **Problem:** root search, the keyboard-first surface, has no way to manage the extension behind a command (2.8).
- **Raycast behaviour to copy:**
  - "Uninstall Extension", "Disable Command" and "Configure Command / Configure Extension" in the Action Panel.
  - Chords: Ctrl+Shift+, for Configure Command, and Ctrl+Alt+, for Configure Extension, per [action-panel](https://manual.raycast.com/action-panel).
  - Disable Command on Ctrl+Shift+D or Ctrl+Alt+Shift+D, since the sources disagree.
- **Files:**
  - `crates/pane-core/src/launcher/actions.rs` (`ResultAction`, `result_actions`)
  - the existing operations (`Launcher::extension_operations` / `run_extension_operation`) and the launcher's own uninstall confirmation (`pane-core/src/launcher/uninstall.rs`)
  - `features/actions_panel.rs`: keycaps
  - `keyboard.rs`: fixed bindings
  - `pane-core/src/keyboard.rs`
- **Acceptance criteria:**
  - For an installed command's row, the panel lists, under a "Pane" or "Extension" group:
    - Configure Command…, when the command has preferences
    - Configure Extension…, **always**, opening its page
    - Disable Command
    - Uninstall Extension…, destructive
  - Uninstall opens the launcher's existing "Uninstall X?" confirmation, so Enter/Escape work there.
  - Configure Extension shows and runs on its chord.
  - Default extensions follow the existing operation's availability (unavailable entries show the reason).
  - This is a core change within ADR 0043: same operations, new entry point. Note the ADR in the PR.

### 4. Give the General, Launcher, Keyboard, File Search and About pages' switches, segments and buttons keyboard focus

- **Problem:** about 40 Settings controls outside Extensions are mouse-only (2.4–2.7, 2.13).
- **Raycast behaviour to copy:** every control is reachable; toggles respond to Space (UNCONFIRMED in Raycast's docs, but it is the platform norm).
- **Files:**
  - `features/settings/general.rs` (`:496-570`)
  - `launcher.rs` (`:649-711`, `:754-931`, `segments` `:817-839`)
  - `keyboard.rs` (`:553-578`, `:608-630`, `:810-891`)
  - `file_search.rs`
  - `about.rs` (`:262-330`)
  - the shared helper from item 1
  - copy `appearance.rs:55-56`, `:434-441`, `:580-587` for segments
- **Acceptance criteria:**
  - Every element with role Button, Switch, RadioButton, Link or MenuItem on these pages is a tab stop with a visible focus ring.
  - Enter/Space activate it. Left/Right move a focused segment group, as the designed segmented control does.
  - Reset ranking and Reset history move focus to Reset when armed, and Escape cancels the armed state.
  - **Enter in the game-program field adds it.**
  - A new window test walks each Settings page with Tab and fails if any pressable role is not a tab stop. This guards against regressions.

### 5. Clear a recorded hotkey with Backspace, and reach the recorders' Reset from the keyboard (quick win)

- **Problem:** recorders can't be cleared or reset without the mouse, except on the Shortcuts page's Clear (2.3, 2.4, 2.6).
- **Raycast behaviour to copy:** Backspace while recording clears the hotkey (Windows 0.49; macOS 2.7 fix). Escape cancels.
- **Files:**
  - `features/settings/shortcuts.rs` (`shortcuts_hotkey_key_down` `:664-692`)
  - `general.rs` (`recorder_key_down`, Reset `:496-509`)
  - `keyboard.rs` (`keyboard_key_down`, Reset `:553-562`)
  - `mod.rs` (`captured_while_recording`)
- **Acceptance criteria:**
  - **On a command's hotkey cell:** Backspace or Delete with no modifier while recording clears the hotkey, as Clear does.
  - **On the Open Pane and Keyboard-page recorders:** Backspace resets to the default, or says why it cannot (the Open Pane hotkey can't be empty).
  - Reset buttons are tab stops.
  - Tests cover both recorders.

### 6. Move between Settings pages and into a page's controls from the keyboard (quick win)

- **Problem:** Enter on the sidebar does nothing, and a page's controls can't switch pages without Shift+Tab back (2.1).
- **Raycast behaviour to copy:** Settings search (already there). Tab-switching keys are UNCONFIRMED in Raycast, so this follows platform norms rather than copying.
- **Files:** `features/settings/mod.rs` (`bind_keys`, `next_section`/`previous_section` handlers moved to the root), and each page's `focus` function for "first control".
- **Acceptance criteria:**
  - Enter or Right on the sidebar focuses the page's first tab stop.
  - Ctrl+Tab / Ctrl+Shift+Tab (and Ctrl+PageDown / PageUp) select the next or previous sidebar entry from anywhere in the window, except while a recorder listens.
  - Escape from a page control returns focus to the sidebar before it closes the window.
  - The sidebar draws a focus ring around its selected entry while it has focus.

### 7. Submit forms with Ctrl+Enter from any field, open list dropdowns with Ctrl+P, and remove tags and paths from the keyboard

- **Problem:** designed-form submission, the dropdowns and the form pickers lag Raycast (2.10, 2.11). Appendix A adds these gaps:
  - tag chips and the many-paths picker's chips are removed only by a click (`designed/fields.rs:215`, `:343`);
  - the single path picker's "Choose…" button takes no focus (`fields.rs:426-430`). Typing a path is the only keyboard route.
- **Raycast behaviour to copy:** ⌘↵ / Ctrl+↵ submits from any form field, and ⌘P opens a List or Grid dropdown. Both are primary sources ([keyboard-shortcuts](https://manual.raycast.com/keyboard-shortcuts), [list API](https://developers.raycast.com/api-reference/user-interface/list)).
- **Files:**
  - `extension_views/designed/mod.rs` (`:184-185`: bind `SubmitForm` on the form's own context and use the platform modifier: `cmd-enter` on macOS)
  - `extension_views/designed/list.rs` (`render_designed_dropdown`)
  - `features/clipboard_history.rs` and `search_files.rs` (type select)
  - `search_files.rs:498-504`, `:549-555`: make "Open File Search Settings" a tab stop, or an Actions-panel entry
- **Acceptance criteria:**
  - With focus on a checkbox, toggle, select trigger, date, tag or path picker in a form, Ctrl+Enter (Cmd+Enter on macOS) submits.
  - Ctrl+P (Cmd+P on macOS) opens the open List's or Grid's dropdown and the Clipboard History and Search Files type selects, and is shown in a tooltip.
  - The File Search Settings link is reachable without the mouse.
  - Backspace in an empty tag field removes the last tag, and in a many-paths well the last path. Each chip is also a tab stop that Delete or Backspace removes.
  - The single path picker's Choose… is a tab stop, and Enter or Space opens the system dialog.

### 8. Reach detail links, long details and hover-only text from the keyboard

- **Problem** (found in Appendix A):
  - **Links in a designed view's Markdown, the Detail pane included, are click-only.** They have no focus handle, and the address shows only in a tooltip (`designed/markdown.rs:383-405`).
  - **Long text in these regions scrolls only with the wheel:**
    - the details of a confirmation, preview or error overlay (`app.rs:3553-3570`)
    - the toast details text (`toast.rs:101-102`)
    - the detail panes of designed Lists and the split views (`designed/list.rs:31-32`, `ui/split_view.rs:28-29`)

    A long stack trace or preview can't be read by keyboard. The error overlay's Copy row is the only workaround.
  - **Information shown only in tooltips:**
    - a row's title and subtitle tooltips and accessory tooltips (`ui/result_row.rs:182`, `:197`, `:296`)
    - icon labels (`ui/extension_icon.rs:216`)
    - pin details (`quick_slots.rs:679`, `compact_pins.rs:189`)
    - Markdown image alt text (`markdown.rs:356`)
- **Raycast behaviour to copy:**
  - Raycast's Detail Markdown links are clickable, and extensions add "Open in Browser" actions. Keyboard scrolling of the detail is **UNCONFIRMED**.
  - Accessory tooltips are hover-only in Raycast too. So the tooltip part is Pane going further, and optional.
- **Files:**
  - `extension_views/designed/markdown.rs`
  - `features/actions_panel.rs` and `pane-core`'s `item_actions`: link entries
  - `app.rs` (details region)
  - `features/toast.rs`
  - `ui/split_view.rs`
  - `extension_views/designed/list.rs`
- **Acceptance criteria:**
  - Each link in the shown Markdown is reachable. Either each link is a tab stop (Enter or Space opens it, focus ring), or the Actions panel lists "Open <link text>" entries for the selected item's detail. The URL shows in the footer or status while a link is focused.
  - When the details region or a detail pane overflows, Shift+Up/Down or Shift+Page Up/Down (or the platform convention chosen) scrolls it without moving the selection.
  - Optionally, the selected row's tooltip text is shown in the footer status while it is selected.

### 9. Add a Quit Pane command to root search (quick win)

- **Problem:** the tray menu is the only way to quit (`crates/pane-core/src/tray/windows.rs:754-756`, `tray/macos.rs:216-224`). By keyboard that means the system's tray navigation: Win+B, the arrows, then the menu key. No root command quits Pane. With the tray hidden by the General page's switch, there is **no visible way to quit**. Whether closing the launcher window quits is **UNCONFIRMED**.
- **Raycast behaviour to copy:** a "Quit Raycast" root command. **UNCONFIRMED** in the sources fetched for this audit, but long-standing in Raycast.
- **Files:**
  - `crates/pane-core/src/launcher.rs`: Pane's own root rows, beside "Manage Extensions" (`:5164`)
  - the window's quit path (`main.rs:342-352`, `app.rs:1845-1849`)
- **Acceptance criteria:**
  - "Quit Pane" is a root search row, found by "quit". Enter quits through the same path the tray's Exit takes: hotkeys released, `quit_cleanly`, tray removed.
  - A window test covers it, without real quitting, via the existing test seam.

**Order:**
1. Items 1 and 2 together fix the user's complaint, so do them first, as one PR or two. Item 2 can note today's workaround: summon the launcher, which shows the same confirmation, and answer it there.
2. Item 3 is the fastest Raycast-parity win.
3. Then the quick wins: 5, 6 and 9.
4. Then 7.
5. Then 4, the broadest change.
6. Then 8. It matters for detail-heavy extensions, but Raycast's own detail keyboard model is unconfirmed.

**Cross-cutting notes:**
- ADR 0035 already requires focus rings on Settings controls, and ADR 0029's accepted look is unaffected if rings use `focus_visible`, so they appear only for keyboard focus.
- ADR 0043 keeps operations and confirmations in the core; items 2 and 3 only add entry points and keys.
- Nothing here needs a new ADR, except item 6's page-switch chord if the user wants it recorded.

---

## Appendix A: every mouse action and its keyboard route

### Method

- **The scan:** a script over `crates/pane/src` (excluding `#[cfg(test)] mod tests` and comments) found every `on_click`, `on_mouse_down`, `on_mouse_down_out`, `capture_any_mouse_down`, `on_mouse_move`, `on_scroll_wheel`, `on_hover` and `tooltip`. By hand I added:
  - the canvas's drag listeners (`on_mouse_move_all` and `on_mouse_up_all`)
  - the native tray / menu-bar menu (`crates/pane-core/src/tray/*`)
  - the regions that scroll only with the wheel
- **What the scan found:**
  - no `on_drag`, `on_drop` or `on_mouse_up`
  - no native context menus other than the tray
  - two right-click handlers: a quick slot's, and the canvas's secondary press
- **Lines:** line numbers are those of the handler call at `main` 069bfab0. Paths are relative to `crates/pane/src` unless they start with `crates/`.
- **Route** names the key and the key context, with where it is bound or handled.

**Status:**
- **Y** — a direct keyboard route does the same.
- **P** — partial or indirect. Another screen, another command, or only some cases.
- **N** — none.
- **E** — passed to the extension. The keyboard equivalent exists only if the extension handles `onKey`.
- **—** — presentation only: a hover wash or pointer tracking. Listed, but not counted.

### Counts

| | Count |
|---|---|
| Mouse actions (excluding the 22 presentation-only handlers) | **163** |
| With a direct keyboard route (Y) | **82** |
| Partial or indirect (P) | **16** |
| Without a route (N) | **59** |
| Extension-defined (E), canvas | **6** |

By section:

| Section | Total | Y | P | N | E |
|---|---|---|---|---|---|
| A.1 Launcher window | 46 | 43 | 3 | 0 | 0 |
| A.2 Designed views | 21 | 11 | 1 | 3 | 6 |
| A.3 Settings window | 81 | 22 | 11 | 48 | 0 |
| A.4 Tooltips | 11 | 3 | 0 | 8 | 0 |
| A.5 Tray / menu bar | 4 | 3 | 1 | 0 | 0 |
| **Total** | **163** | **82** | **16** | **59** | **6** |

By kind:

| Kind | Total | Y | P | N | E |
|---|---|---|---|---|---|
| `on_click` | 126 | 61 | 15 | 50 | 0 |
| `on_mouse_down_out` / `capture_any_mouse_down` | 11 | 10 | 0 | 1 | 0 |
| `on_mouse_down` (incl. 2 right-click) | 3 | 1 | 0 | 0 | 2 |
| `on_mouse_move` (hover selects) | 3 | 3 | 0 | 0 | 0 |
| `on_scroll_wheel` / canvas drag | 3 | 0 | 0 | 0 | 3 |
| `on_hover` with an effect | 2 | 1 | 0 | 0 | 1 |
| `tooltip` | 11 | 3 | 0 | 8 | 0 |
| tray / menu-bar entry | 4 | 3 | 1 | 0 | 0 |
| **Total** | **163** | **82** | **16** | **59** | **6** |

Of the 59 N:
- 48 are in Settings: Extensions 16 (the popover's outside click among them), Launcher page 10, File Search 9, Keyboard 6, General 5, About 2.
- 8 are hover-only tooltips.
- 3 are designed-view controls: tag chip, path chip, Markdown link.

The launcher window itself (A.1) has no action without a route.

### A.1 Launcher window

| # | Where | Kind | What it does | Keyboard route today | St |
|---|---|---|---|---|---|
| 1 | `app.rs:2644` | click | Root row: select and run. A command's row: run. | Arrows, then Enter (`Confirm`, `KEY_CONTEXT`, `keyboard.rs:153`) | Y |
| 2 | `app.rs:2755` | click | Choice row (first setup, collection): preview the extension | Arrows, then Enter | Y |
| 3 | `app.rs:2819` | click | Choice row's tick | Space (`TickChoice`, `CHOICE_CONTEXT`, `lib.rs:87`) | Y |
| 4 | `app.rs:2877` | click | Computed answer card: run | Arrows, then Enter | Y |
| 5 | `app.rs:2912` | click | Footer primary-action button | Enter (`keyboard.rs:153`) | Y |
| 6 | `app.rs:2925` | click | Footer Actions button | Ctrl+K (`OpenActions`, `keyboard.rs:161`) | Y |
| 7 | `app.rs:2624` | mouse_move | Root rows: hover selects | Up/Down (`keyboard.rs:145-146`) | Y |
| 8 | `app.rs:2865` | mouse_move | Answer card: hover selects | Up/Down | Y |
| 9 | `features/actions_panel.rs:1561` | mouse_move | Actions panel: hover selects an entry | Up/Down in the panel field (`actions_panel.rs:194-195`) | Y |
| 10 | `features/actions_panel.rs:1570` | click | Run an entry, or open its submenu | Enter (`LauncherWindow::panel_keys`) | Y |
| 11 | `features/actions_panel.rs:1541` | mouse_down_out | Close the panel | Escape (`StepBack`) or Tab (`CloseActions`, `:196-198`) | Y |
| 12 | `features/footer_menu.rs:309` | click | Open the Pane menu (the mark) | Tab to the button, then Enter/Space (`PressMenuButton`, `footer_menu.rs:82-83`) | Y |
| 13 | `features/footer_menu.rs:472` | click | Menu item (Settings) | Up/Down, Enter (`:72-74`). Ctrl+, does the same. | Y |
| 14 | `features/footer_menu.rs:408` | mouse_down_out | Close the menu | Escape, Tab (`:75-79`) | Y |
| 15 | `features/quick_slots.rs:629` | click | Run a pin (expanded home) | Ctrl+1..5 (`keyboard.rs:221-228`). Tab to the slot, then Enter/Space (`PressSlot`, `quick_slots.rs:92-93`). | Y |
| 16 | `features/quick_slots.rs:632` | right mouse_down | A pin's own Actions panel | Ctrl+K while the slot has focus (`actions_panel.rs:33-35`) | Y |
| 17 | `features/compact_pins.rs:198` | click | Run a pin in the compact row | Ctrl+1..5, first five only. The tiles take no focus, so pins 6+ have no key in compact mode. | P |
| 18 | `features/root_search/arguments.rs:1025` | click | Open an argument dropdown | Enter, Space or Down on the trigger (`arguments.rs:126-128`) | Y |
| 19 | `features/root_search/arguments.rs:1087` | click | Pick an argument choice | Up/Down, Enter (`:129-131`) | Y |
| 20 | `features/root_search/arguments.rs:1134` | mouse_down_out | Close the choices | Escape, Tab (`:132-133`) | Y |
| 21 | `features/root_search/mod.rs:362` | click | Back button in the header or top bar | Escape (`Back`), Backspace in the empty field (`keyboard.rs:154`, `:110-114`) | Y |
| 22 | `features/confirmation.rs:273` | click | Confirm | Enter (`confirmation.rs:64`) | Y |
| 23 | `features/confirmation.rs:302` | click | Dismiss | Escape (`:65`) | Y |
| 24 | `features/confirmation.rs:343` | click | Tick "Don't ask again" | Space (`:66`) | Y |
| 25 | `features/confirmation.rs:393` | mouse_down_out | Click outside = not confirmed | Escape | Y |
| 26 | `features/toast.rs:762` | click | Toast details (^) | Ctrl+T (`toast.rs:119`). Tab, then Enter/Space (`:121-122`). | Y |
| 27 | `features/toast.rs:824` | click | Toast close (×), shown on hover or focus | Tab to it, then Enter/Space (`:123-124`). Escape while the toast has focus (`:120`). | Y |
| 28 | `features/toast.rs:874` | click | Close the details popover | Escape (`:128`), Enter/Space on its close (`:129-130`), Ctrl+T toggles | Y |
| 29 | `features/toast.rs:956` | click | Run a toast action | Up/Down, Enter in the details (`:125-127`), or the action's own shortcut | Y |
| 30 | `features/toast.rs:1015` | mouse_down_out | Close the details | Escape | Y |
| 31 | `features/toast.rs:679` | hover | Pause the toast's timer while hovered | Focusing any toast control pauses it too (`toast.rs:14-16`) | Y |
| 32 | `features/extension_log.rs:664` | click | Select a log line | Up/Down, Page Up/Down, Home/End (`extension_log.rs:87-88`, `:546-547`) | Y |
| 33 | `features/extension_log.rs:702` | click | Log buttons: Copy, Copy All, Clear, Open File | Ctrl+C, Ctrl+Shift+C, Ctrl+L, Ctrl+O (`:83-86`). Enter copies. | Y |
| 34 | `features/update_results.rs:285` | click | Open an extension's page from the results | Arrows, then Enter (`update_results.rs:17-19`) | Y |
| 35 | `features/clipboard_history.rs:824` | click | Back (leaves at once) | Escape clears the query, then leaves. Shift+Escape returns to root (`keyboard.rs:158`). | Y |
| 36 | `features/clipboard_history.rs:1038` | click | Paste | Enter | Y |
| 37 | `features/clipboard_history.rs:1060` | click | Actions | Ctrl+K | Y |
| 38 | `features/clipboard_history.rs:1200` | click | Select a record | Up/Down (`clipboard_history.rs:111-121`) | Y |
| 39 | `features/search_files.rs:472` | click | Back | Shift+Escape, or Escape twice | Y |
| 40 | `features/search_files.rs:504` | click | "Open File Search Settings" (index line) | None to that page. Ctrl+, opens Settings at General; then Ctrl+F "File Search". | P |
| 41 | `features/search_files.rs:555` | click | Same link (empty or loading state) | As #40 | P |
| 42 | `features/search_files.rs:832` | click | Select a file; a double click opens it | Up/Down; Enter | Y |
| 43 | `ui/select.rs:740` | click | Open a select (dropdowns, type filters, preferences) | Enter, Space or Down on the trigger (`select.rs:153-155`) | Y |
| 44 | `ui/select.rs:739` | capture mouse_down | A click on the trigger closes the popup | Escape (`:161`) | Y |
| 45 | `ui/select.rs:1030` | click | Commit a choice | Up/Down, Home/End, Enter (`:156-160`) | Y |
| 46 | `ui/select.rs:958` | mouse_down_out | Cancel | Escape, Tab (`:161-163`) | Y |

### A.2 Designed views (extension UI)

| # | Where | Kind | What it does | Keyboard route today | St |
|---|---|---|---|---|---|
| 47 | `extension_views/designed/components.rs:380` | click | Button | Tab, then Enter/Space (`Press`, `BUTTON_CONTEXT`, `designed/mod.rs:147-148`) | Y |
| 48 | `…/components.rs:526` | click | Link (component) | Tab, then Enter/Space (`BUTTON_CONTEXT`) | Y |
| 49 | `…/components.rs:909` | click | Pressable row or card | Tab, then Enter/Space (`BUTTON_CONTEXT`) | Y |
| 50 | `…/components.rs:1099` | click | Toggle | Enter/Space (`TOGGLE_CONTEXT`, `:149-150`) | Y |
| 51 | `…/components.rs:1187` | click | Checkbox | Enter/Space (`CHECKBOX_CONTEXT`, `:151-152`) | Y |
| 52 | `…/components.rs:1315` | click | Segmented choice | Left/Right (`SEGMENTED_CONTEXT`, `:153-154`) | Y |
| 53 | `…/components.rs:1438` | click | Slider | Left/Right (`SLIDER_CONTEXT`, `:155-156`) | Y |
| 54 | `…/components.rs:2088` | click | List-like row | Tab, then Enter/Space (`BUTTON_CONTEXT`) | Y |
| 55 | `…/fields.rs:215` | click | **Remove a tag (chip)** | **NONE.** `designed_tag_removed` is called only from this click (`designed/mod.rs:803`). | N |
| 56 | `…/fields.rs:249` | click | Add a tag option | Enter (`CommitTag`, `TAGS_CONTEXT`, `:188`) | Y |
| 57 | `…/fields.rs:343` | click | **Remove the last path (chip)** | **NONE** (`designed/mod.rs:909`) | N |
| 58 | `…/fields.rs:430` | click | "Choose…" path dialog | Many paths: Enter in the well (`ChoosePath`, `PATHS_CONTEXT`, `:189`). Single path: none; type the path instead. | P |
| 59 | `…/fields.rs:479` | click | Form submit button | Enter in a single-line field. Ctrl+Enter in a field or text area (`:157`, `:184-185`). | Y |
| 60 | `…/list.rs:470` | click | Grid cell: select and open | Arrows by cell or row (`app.rs:672-735`, `:1854-1861`), then Enter | Y |
| 61 | `…/markdown.rs:402` | click | **Open a Markdown link** (Detail pane, Markdown nodes) | **NONE.** The link has no focus handle. | N |
| 62 | `…/canvas.rs:331` | left mouse_down | Canvas press, double click, drag start | E: the extension's `onKey`, or `onActivate` via Space (`:165-183`) | E |
| 63 | `…/canvas.rs:349` | right mouse_down | Canvas secondary press | E. No key maps to it; the Menu key and Shift+F10 are not forwarded as secondary. | E |
| 64 | `…/canvas.rs:367` | scroll_wheel | Canvas wheel | E (`onKey` arrows, Page keys) | E |
| 65 | `…/canvas.rs:467` | mouse_move_all | Canvas drag move | E | E |
| 66 | `…/canvas.rs:484` | mouse_up_all | Canvas drag release | E | E |
| 67 | `…/canvas.rs:381` | hover | Canvas enter/leave event | E (no keyboard analogue) | E |

### A.3 Settings window

| # | Where | Kind | What it does | Keyboard route today | St |
|---|---|---|---|---|---|
| 68 | `features/settings/mod.rs:550` | click | Sidebar page entry | Up/Down on the sections list (`mod.rs:119-120`) | Y |
| 69 | `features/settings/mod.rs:866` | click | Caption buttons: minimize, maximize, close (Windows) | Ctrl+W (`mod.rs:131-139`), Alt+F4, Win+Down / Win+Up (OS) | Y |
| 70 | `features/settings/search.rs:588` | click | Open a search result | Up/Down, Enter in the search field (`search.rs:109-111`) | Y |
| 71 | `features/settings/shortcuts.rs:1140` | click | Expand or collapse a group | Enter/Space (`ToggleGroup`, `shortcuts.rs:138-139`) | Y |
| 72 | `features/settings/shortcuts.rs:1431` | click | Edit an alias | Enter/Space (`EditAlias`, `:136-137`) | Y |
| 73 | `features/settings/shortcuts.rs:1604` | click | Clear a hotkey | Enter/Space (`ClearHotkey`, `:158-159`) | Y |
| 74 | `features/settings/shortcuts.rs:1663` | click | Start or stop recording a hotkey | Enter/Space (`RecordHotkey`, `:150-153`); Escape/Tab cancel (`:154-156`) | Y |
| 75 | `features/settings/shortcuts.rs:1651` | mouse_down_out | Cancel recording | Escape, Tab | Y |
| 76 | `features/settings/appearance.rs:441` | click | Background: Choose… / Remove | Tab, then Enter/Space (`Choose`, `CHOICE`, `appearance.rs:55-56`, `:419-442`) | Y |
| 77 | `features/settings/appearance.rs:587` | click | Theme, Material and Effect segments | Tab, then Enter/Space (`appearance.rs:55-56`) | Y |
| 78 | `features/settings/general.rs:489` | click | Open Pane recorder | Enter/Space (`ActivateRecorder`, `general.rs:130-133`) | Y |
| 79 | `features/settings/general.rs:482` | mouse_down_out | Cancel recording | Escape, Tab (`:135-137`) | Y |
| 80 | `features/settings/general.rs:503` | click | Reset Open Pane hotkey | NONE | N |
| 81 | `features/settings/general.rs:519` | click | Use the Windows key | NONE | N |
| 82 | `features/settings/general.rs:532` | click | Launch at login switch | NONE | N |
| 83 | `features/settings/general.rs:548` | click | Tray icon switch | NONE | N |
| 84 | `features/settings/general.rs:562` | click | Taskbar switch | NONE | N |
| 85 | `features/settings/launcher.rs:666` | click | Learn from choices switch | NONE | N |
| 86 | `features/settings/launcher.rs:691` | click | Window mode segments | NONE | N |
| 87 | `features/settings/launcher.rs:698` | click | Pinned layout segments | NONE | N |
| 88 | `features/settings/launcher.rs:705` | click | Compact pinned switch | NONE | N |
| 89 | `features/settings/launcher.rs:800` | click | Reset ranking… (arm) | NONE | N |
| 90 | `features/settings/launcher.rs:762` | click | Reset (ranking) | NONE | N |
| 91 | `features/settings/launcher.rs:782` | click | Cancel (ranking reset) | NONE | N |
| 92 | `features/settings/launcher.rs:925` | click | Reset search history… (arm) | NONE | N |
| 93 | `features/settings/launcher.rs:887` | click | Reset (history) | NONE | N |
| 94 | `features/settings/launcher.rs:907` | click | Cancel (history reset) | NONE | N |
| 95 | `features/settings/keyboard.rs:545` | click | Action recorder | Enter/Space (`keyboard.rs:91-94`) | Y |
| 96 | `features/settings/keyboard.rs:538` | mouse_down_out | Cancel recording | Escape, Tab (`:96-98`) | Y |
| 97 | `features/settings/keyboard.rs:555` | click | Reset a binding | NONE | N |
| 98 | `features/settings/keyboard.rs:565` | click | Escape behaviour segments | NONE | N |
| 99 | `features/settings/keyboard.rs:572` | click | Escape closes Settings switch | NONE | N |
| 100 | `features/settings/keyboard.rs:824` | click | Game mode switch | NONE | N |
| 101 | `features/settings/keyboard.rs:856` | click | Remove a game program | NONE | N |
| 102 | `features/settings/keyboard.rs:888` | click | Add a game program (Enter in the field does nothing) | NONE | N |
| 103 | `features/settings/file_search.rs:473` | click | Rebuild Index | NONE | N |
| 104 | `features/settings/file_search.rs:580` | click | Remove an added root folder | NONE | N |
| 105 | `features/settings/file_search.rs:598` | click | Add Folder… | NONE | N |
| 106 | `features/settings/file_search.rs:672` | click | Remove an excluded folder | NONE | N |
| 107 | `features/settings/file_search.rs:708` | click | Remove an excluded pattern | NONE | N |
| 108 | `features/settings/file_search.rs:726` | click | Exclude Folder… | NONE | N |
| 109 | `features/settings/file_search.rs:772` | click | Exclude (add pattern; Enter in the field does nothing) | NONE | N |
| 110 | `features/settings/file_search.rs:824` | click | Built-in exclusion rule switches | NONE | N |
| 111 | `features/settings/file_search.rs:917` | click | Include Again | NONE | N |
| 112 | `features/settings/about.rs:264` | click | Check for updates | Root search row "Check for a Pane update" (`crates/pane-core/src/launcher/application_update.rs:173`) | Y |
| 113 | `features/settings/about.rs:270` | click | Install update | Root search row "Update Pane to X" (`application_update.rs:159`) | Y |
| 114 | `features/settings/about.rs:278` | click | Open documentation | NONE | N |
| 115 | `features/settings/about.rs:298` | click | Copy diagnostics | NONE | N |
| 116 | `features/settings/about.rs:312` | click | Open log folder | Only the crash-notice root row, after a crash (`Entry::OpenLogFolder`) | P |
| 117 | `features/settings/extensions.rs:595` | click | + (install menu) | Indirect: Ctrl+F "Install from…" Enter, or root search's install rows (`extensions.rs:356-363`, `pane-core/src/launcher.rs:4991-5021`) | P |
| 118 | `features/settings/extensions.rs:615` | click | + menu item: Install from Folder / npm / Git | Ctrl+F "Install from npm…" Enter starts it and focuses the field (`extensions.rs:412-423`); root install rows | Y |
| 119 | `features/settings/extensions.rs:670` | mouse_down_out | Close the + or Actions popover | **NONE.** Escape closes the whole window (`mod.rs:143`, `:491-498`). | N |
| 120 | `features/settings/extensions.rs:755` | click | Sidebar extension entry | Up/Down (`mod.rs:312-364`) | Y |
| 121 | `features/settings/extensions.rs:1509` | click | Group page: open an installed extension | Sidebar Up/Down, or Ctrl+F its title | Y |
| 122 | `features/settings/extensions.rs:1577` | click | Group page: runtime and kept-data rows | NONE | N |
| 123 | `features/settings/extensions.rs:1601` | click | Update extensions automatically switch | NONE | N |
| 124 | `features/settings/extensions.rs:1630` | click | Check for updates | Root row "Check for Extension Updates" (`pane-core/src/launcher.rs:5132`) | Y |
| 125 | `features/settings/extensions.rs:1697` | click | Official extension: Install | Indirect: root row "Set Up Official Extensions…" (`launcher.rs:5074`) | P |
| 126 | `features/settings/extensions.rs:1740` | click | Install from Folder… / npm… / Git… buttons | Ctrl+F "Install from…", or root install rows | Y |
| 127 | `features/settings/extensions.rs:1787` | click | Show Package (npm/Git field) | **NONE.** Enter in the field is unbound (`:3523`). | N |
| 128 | `features/settings/extensions.rs:2041` | click | **Extension Enabled switch** | NONE | N |
| 129 | `features/settings/extensions.rs:2057` | click | Extension Update automatically switch | NONE | N |
| 130 | `features/settings/extensions.rs:2159` | click | Requirement fix button | NONE | N |
| 131 | `features/settings/extensions.rs:2401` | click | **Actions… button** | NONE | N |
| 132 | `features/settings/extensions.rs:2439` | click | Actions → Check for Update | NONE | N |
| 133 | `features/settings/extensions.rs:2458` | click | Actions → Reload, Clear Cache, Reset Confirmations, Retry, details, Logs… | NONE | N |
| 134 | `features/settings/extensions.rs:2472` | click | Actions → Show Source Folder | NONE | N |
| 135 | `features/settings/extensions.rs:2487` | click | **Actions → Uninstall** | NONE | N |
| 136 | `features/settings/extensions.rs:2586` | click | Forget a removed command's choices | NONE | N |
| 137 | `features/settings/extensions.rs:2690` | click | Command Fallback switch | NONE | N |
| 138 | `features/settings/extensions.rs:2709` | click | **Command enable switch** | NONE | N |
| 139 | `features/settings/extensions.rs:2850` | click | Checkbox preference | NONE | N |
| 140 | `features/settings/extensions.rs:2888` | click | Preference Choose… / Add… (path, app) | Type the path into the well beside it (`:2938-2951`); no dialog by key | P |
| 141 | `features/settings/extensions.rs:994` | click | **Flow rows: "Uninstall X?" answers, disable confirmations, the preview's Install, Retry** | Indirect: summon the launcher (Open Pane hotkey), which shows the same screen, and answer with arrows and Enter there (ADR 0043) | P |
| 142 | `features/settings/extensions.rs:1005` | click | Flow Back (details or preview) | Indirect: Escape in the launcher window | P |
| 143 | `features/settings/extensions.rs:1146` | click | Collection choice row (preview) | Indirect: the launcher, Enter | P |
| 144 | `features/settings/extensions.rs:1174` | click | Collection choice: Install row | Indirect: the launcher, Enter | P |
| 145 | `features/settings/extensions.rs:1185` | click | Collection choice Back | Indirect: the launcher, Escape | P |
| 146 | `features/settings/extensions.rs:1257` | click | Collection choice tick | Indirect: the launcher, Space (`lib.rs:87`) | P |
| 147 | `features/settings/extensions.rs:1327` | click | Update results row: open the extension's page | Sidebar Up/Down to that extension | Y |
| 148 | `features/settings/extensions.rs:1343` | click | Update results Back | Indirect: the launcher, Escape | P |

### A.4 Tooltips (information on hover)

| # | Where | What it shows | Keyboard route today | St |
|---|---|---|---|---|
| 149 | `ui/result_row.rs:182` | A row's full title (an item's tooltip) | NONE visually; the title is in `aria_label` | N |
| 150 | `ui/result_row.rs:197` | A row's subtitle tooltip | NONE | N |
| 151 | `ui/result_row.rs:296` | Accessory tooltip (tags, dates, icons) | NONE (Raycast's accessory tooltips are hover-only too) | N |
| 152 | `ui/extension_icon.rs:216` | An icon's label | NONE visually (`aria_label`) | N |
| 153 | `features/quick_slots.rs:679` | A pin's detail (what tells it apart) | NONE visually | N |
| 154 | `features/compact_pins.rs:189` | A compact pin's detail | NONE | N |
| 155 | `extension_views/designed/markdown.rs:356` | A Markdown image's alt or title | NONE visually (`aria_label`) | N |
| 156 | `extension_views/designed/markdown.rs:395` | A Markdown link's address | NONE (the link isn't focusable) | N |
| 157 | `features/root_search/mod.rs:358` | Back button: names the back key | The key itself works | Y |
| 158 | `features/clipboard_history.rs:820` | Same | Same | Y |
| 159 | `features/search_files.rs:468` | Same | Same | Y |

### A.5 Native tray and menu-bar entry (`crates/pane-core/src/tray/`)

| # | Where | What it does | Keyboard route today | St |
|---|---|---|---|---|
| 160 | `tray/windows.rs:798` (WM_LBUTTONUP); macOS click | Left click summons the launcher | The Open Pane hotkey. Also Win+B, arrows to the icon, Enter (the shell's keyboard; `WM_CONTEXTMENU` is handled, `:793`). | Y |
| 161 | `tray/windows.rs:754`, `tray/macos.rs:224` | Menu → Open Pane | The Open Pane hotkey | Y |
| 162 | `tray/windows.rs:755` | Menu → Settings | Ctrl+, in the launcher | Y |
| 163 | `tray/windows.rs:756`, `tray/macos.rs:224` | Menu → Exit / Quit Pane | Only through the tray by keyboard: Win+B, arrows, then the menu key or Shift+F10, then Exit. **No in-app Quit command.** With the tray hidden, none. | P |

### A.6 Presentation only (not counted)

Hover washes, which report hover for the wash and do nothing else:
- `app.rs:2610`, `:2907`
- `clipboard_history.rs:1033`, `:1055`, `:1197`
- `compact_pins.rs:177`
- `confirmation.rs:268`, `:297`
- `extension_log.rs:617`, `:699`
- `footer_menu.rs:296`, `:450`
- `quick_slots.rs:608`, `:673`
- `search_files.rs:827`
- `toast.rs:750`, `:819`, `:862`, `:940`
- `update_results.rs:279`

That is 20. Pointer tracking: `app.rs:3542`. A wheel scroll that hides the number hints: `app.rs:3537`; releasing Ctrl does the same.

### A.7 Regions that scroll only with the wheel (no handler; not counted above)

| Where | What | Keyboard route |
|---|---|---|
| `app.rs:3553-3570` | Long details of a confirmation, preview or error overlay: a stack trace, a package preview (40% / 62% of the window, `overflow_y_scroll`) | NONE. The error overlay's Copy row copies the text. |
| `features/toast.rs:101-102` | Toast details text, past 200 px | NONE. Up/Down move the actions. |
| `extension_views/designed/list.rs:31-32`, `designed/markdown.rs` | The Detail pane beside a designed List | NONE (**UNVERIFIED** whether focus can enter it) |
| `ui/split_view.rs:28-29` | The Clipboard History and Search Files detail panes | NONE. The selection drives the preview. |

Lists keep their selection in view when arrows move it (root search, the Actions panel, Clipboard History, the log), so those need nothing.

### What Appendix A adds to Part 3

- **New item 8 (detail links, long details, tooltips):** #61, #149–#156 and A.7.
- **New item 9 (Quit Pane command):** #163.
- **Item 7 now also covers:**
  - tag and path chip removal (#55, #57)
  - the single path picker's Choose… (#58)
  - the File Search link (#40, #41)
  - optionally, forwarding the Menu key / Shift+F10 as a canvas secondary press (#63). Otherwise `docs/designed-tree.md` should tell canvas authors to offer keys.
- **Items 1, 2 and 4 cover every Settings N** (#80–#111, #114–#115, #119, #122–#123, #127–#139).
- **Compact-mode pins past the fifth (#17)** are a minor gap. Fold them into item 6's focus work if the compact row should take Tab, or leave them; Raycast's compact mode also numbers only the first nine.
