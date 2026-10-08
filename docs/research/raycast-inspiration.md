# What Pane can take from Raycast

Researched 2026-10-05 from Raycast's own manual, developer docs, product pages and changelog. This is documentation research, not an audit of Raycast's closed-source app. It is a list of ideas, not decisions. Several ideas reverse something Pane chose on purpose (no recent use, no manifest keywords, no unit conversion, a fixed Appearance scope); those are marked **needs a user decision**. Earlier Raycast notes: [extension model](raycast.md), [root search boundary](raycast-root-search.md), [extension UI](raycast-extension-ui.md).

Pages were read through a summarizing fetcher. Quoted phrases are as it returned them, so check the wording before quoting it in a spec. Raycast's pages disagree with each other in one place: Settings lists root search sensitivity as "Low or High" ([settings][ST]), while the search-bar page lists High, Medium (default) and Low ([search bar][SB]).

Raycast now ships on Windows (v2.6, Windows 10/11, Microsoft Store or WinGet) and says it still has fewer features there than on macOS ([Windows][WIN], [Windows changelog][CLW]). Its Windows behavior is therefore a fair comparison for most items.

## 1. Pinned home vs Raycast Favorites (the 5-slot question)

> Decided since: [ADR 0027](../adr/0027-quick-slots-are-an-ordered-list.md) made the quick slots an ordered list of pins with no limit and no "replace a slot" step, numbering the first five. The "Pane quick slots" column below describes Pane before that decision, and the recommendation was not the one taken.

| | Raycast Favorites | Pane quick slots |
| --- | --- | --- |
| Count | No limit documented on the search-bar, action-panel, keyboard or settings pages ([search bar][SB], [action panel][AP]) | Exactly 5 (`QUICK_SLOTS`; a record with more is invalid, [ADR 0026](../adr/0026-host-keeps-quick-slots-by-identity.md)) |
| Where shown | Their own section at the top of root search "when the search bar is empty" ([search bar][SB]) | The pinned home above a blank query: a strip of tiles or rows |
| Add | "Add to Favorites" (Ctrl+F) from the Action Panel ([action panel][AP]) | "Pin to Quick Slot" in the Actions panel; when all 5 are full it asks which one to replace |
| Reorder | "Move Favorite Up/Down" (Ctrl+Shift+Up/Down) ([keyboard shortcuts][KS]) | "Move Slot Left/Right" in the slot's own panel; no direct key |
| Number chords | None documented for favorites ([keyboard shortcuts][KS]) | Ctrl+1–5, with hints shown while Ctrl is held |
| Compact mode | "Show Favorites in Compact Mode" shows them "as icons in the compact window" ([settings][ST]) | Compact mode shows only the search field |
| Empty-query extras | Recently used files and today's calendar events also appear ([search bar][SB]) | Nothing: by decision, Pane shows no recent use |

**Recommendation: allow up to 9 pins, not an unlimited list.** Raycast's open-ended Favorites list works because frecency ranking and fuzzy search carry the rest. Pane's pins are different: every pin owns a Ctrl+digit chord, and that one-key reach is the point of pinning. So:

- **Make the slot count 1–9** (the user chooses, default 5). Slot *i* gets Ctrl+*i*. The digits slots don't use keep their current job of picking the first rows below. Nine is the natural ceiling because there are only nine digit chords. Past that, aliases and global hotkeys are the better tools, and both already exist.
- **Horizontal layout:** tiles shrink, or the strip wraps to a second row, past about 6 slots. **Vertical layout** already lists rows and scales to 9 as-is.
- **"Replace a Quick Slot"** should only appear once every slot is full, as today.
- **Effort M:** a new `quick-slots.json` version (count plus slots), layout work, chord and number-hint remapping, and a Launcher-page control.
- **Don't** adopt Raycast's unlimited list as a separate tier. It would bring back the "recents and favorites" home that Pane's pinned home deliberately avoids.

Related quick wins:

- Add direct keys for pinning and reordering (Raycast's are Ctrl+F and Ctrl+Shift+Up/Down ([keyboard shortcuts][KS]); the developer API's common Pin shortcut is Ctrl+. ([Keyboard API][KB])).
- Add a "show pinned in compact mode" option, as icons only.

## 2. Extensions settings UX (avoiding walls of text)

How Raycast presents an extension:

- **A one-sentence description.** Store rules: "In one sentence, what does your extension do?" ([store guidelines][STORE]). Pane's `pane.json` has no description field at all (package: title, version, API, commands; command: title, subtitle).
- **Subtitles only when they add context.** "Don't use a subtitle if it doesn't add context" ([store guidelines][STORE]).
- **Preference help as tooltips.** Each preference has a title, type, default and `required`, and its description "will be displayed as a tooltip" rather than inline text. Types: textfield, password, checkbox, dropdown, appPicker, file, directory ([manifest][MAN]).
- **Setup only when needed.** Required preferences show a setup form before the command first runs. An optional `help.md` is rendered "beside the setup form", and long-form docs belong in the README ([preferences API][PREF], [manifest][MAN]).
- **Per-command switches.** The Extensions tab enables or disables individual commands, holds extension settings and sign-in, and uninstalls ([settings][ST]). A `disabledByDefault` flag ships a command switched off ([manifest][MAN]).
- **Shortcuts in one place.** The Shortcuts tab is "a bird's-eye view of every shortcut", with filters ([settings][ST]). Pane already moved alias, hotkey and fallback rows to its own Shortcuts page.
- **Settings search with Ctrl+F** covers "a setting, a command, an extension, or just a keyword" ([settings][ST]). Pane already has settings search (`features/settings/search.rs`).
- **Grouped update results.** "Check for Extension Updates" groups results as Updated, Skipped, Failed and Removed ([extensions][EX]).

Recommendations, roughly in order:

| Idea | Pane today | Effort |
| --- | --- | --- |
| Optional one-line `description` in `pane.json` (package; optionally per command), shown truncated under the name; full text in a tooltip or detail view | Card shows the name only; nothing explains what an extension does | S |
| Status chips instead of sentences (Paused, Developing, Update available, Uses network); the reason goes in a tooltip or one "Why…" button | Rows such as "Why paused", "Network", "Clear cache" all sit on the card | S |
| Move rare operations (Clear cache, Reload, Uninstall, Network details) behind one "…" menu; leave the enable switch and the update switch on the card | Short buttons for every operation | S |
| Master/detail layout: extension list on the left; on the right the description, a commands table (title · alias · hotkey · enabled), then operations | One card per extension | M |
| Per-command enable/disable, removing that command's root results, alias and hotkey while the rest of the package keeps working | Disable applies to the whole package | M (core and record change) |
| Declared preferences in `pane.json` (typed fields, title, short tooltip description, default, required), rendered by Pane in the detail pane; required fields show a setup [form](../forms.md) before the first run | Extension settings are values a command saves through its own UI; nothing is declared | M–L (manifest, WIT, Settings UI) |
| "Check for extension updates" with grouped results | Updates are automatic, with per-package and global switches | S–M |

## 3. Quick wins (S)

| Idea | Raycast | Pane today | Why it fits |
| --- | --- | --- | --- |
| Jump between sections with Ctrl+Up/Down; page through results with Alt+Up/Down | [search bar][SB], [keyboard shortcuts][KS] | Up/Down only | Pane already labels sections (Commands, Results, Calculator, Fallbacks) |
| Up arrow on an empty query restores recent searches; Settings has "Reset Search History" | "Reset the list of recent searches that Raycast restores when you press the up arrow" ([settings][ST]) | None | Recalls the user's own queries without showing recent use. **Needs a user decision**: it is still history |
| Action-panel items "Disable Command" (Ctrl+Shift+D) and "Configure Command" (Ctrl+Shift+,) | [action panel][AP] | Panel offers pin, alias and hotkey | Pane already has package disable; per-command disable follows §2 |
| Show each action's shortcut on its row in the Actions panel, using standard keys (Copy Ctrl+Shift+C, Open Ctrl+O, Pin Ctrl+., Remove Ctrl+D, Move Up/Down Ctrl+Alt+Up/Down) | [Keyboard API][KB] | No per-action keys | Teaches keys without extra prose; gives extension authors a consistent vocabulary |
| Optional `keywords` on a manifest command, so it can be found by other words | [manifest][MAN] | Deliberately "not done" ([root search](../root-search.md#matching-and-ranking)) | Cheap discoverability (e.g. "rm" finding Uninstall). **Needs a user decision** |
| `disabledByDefault` for commands | [manifest][MAN] | n/a | Lets defaults ship optional commands without cluttering root search |
| Check the Vim navigation binding (Ctrl+K up) against the Open actions binding (Ctrl+K) | Raycast lists Vim as Ctrl+J/K/L/H and its Action Panel as Ctrl+K on Windows ([keyboard shortcuts][KS]) | `NavigationBindings::Vim` uses ctrl-k; Open actions defaults to ctrl-k | Possible conflict; worth a test (not verified here). Later: Raycast for Windows 2.6.1's own Keyboard settings list `Emacs (Alt+B, Alt+F, Alt+P, Alt+N)` and `Vim Motions (Alt+H, Alt+L, Alt+K, Alt+J)`, so Pane moved its sets to Alt (Control on macOS) and the clash is gone; see [root search](../root-search.md#host-behavior) |

Already matched, so no work: pop-to-root timing (Pane: restore, immediately, 90 s, 3 min; Raycast: "Immediately" up to 180 s) ([settings][ST]); Escape "back or close" vs "close and pop to root" ([settings][ST]); Emacs/Vim bindings; Shift+Esc to return to root and Ctrl+W to close ([keyboard shortcuts][KS]); window placement by pointer, active window or primary display; fallback commands; settings search; a unified Shortcuts page; launch at login and the tray icon.

## 4. Medium (M)

| Idea | Raycast | Pane today | Fit / notes |
| --- | --- | --- | --- |
| **Paste into the previous app** as a primary action for clipboard history, the calculator and future emoji or snippets (Enter pastes, Ctrl+Enter copies) | Clipboard "primary action" setting and "Prefer pasting as plain text" ([clipboard][CH]); calculator Cmd/Ctrl+Enter pastes ([calculator][CA]) | Enter copies only | High leverage: one host capability (refocus the previous window, send paste) unlocks several features. Needs per-platform adapters |
| **Fuzzy matching, frecency ranking and "Reset Ranking"** | Rank order: alias exact → alias prefix → fuzzy title → subtitle/keywords → frecency; "Reset Ranking" per item; sensitivity setting ([search bar][SB], [action panel][AP]) | Word-prefix and contains matching; no frequency ([root search](../root-search.md#matching-and-ranking)) | Frecency changes ranking without showing recents. **Needs a user decision.** Add sensitivity only once fuzzy matching exists |
| **Quicklinks with `{argument}` and placeholders** (`{clipboard}`, `{date}`, `{selection}`, `{uuid}`, modifiers like `\| trim \| percent-encode`) | [quicklinks][QL], [dynamic placeholders][DP] | http(s) URLs only; `{query}` is explicitly out of scope ([quicklinks](../quicklinks.md#the-url-format)) | A search quicklink ("gh {query}") is the most common quicklink. Reuse the alias + query-taking path |
| **Command arguments**: up to 3 typed inline fields (text, password, dropdown) beside the search bar, moved between with Tab | [arguments][ARGS] | One free-text query per query-taking command | Generalizes Pane's query-taking commands; alias + space focuses the first field ([aliases][AH]) |
| **Calculator: units, dates, time zones** (currency needs the network) | "10ft in m", "monday in 3 weeks", "5pm ldn in sf" ([calculator][CA]) | Arithmetic only; "Pane has no unit conversion" ([CONTEXT](../../CONTEXT.md)) | Offline units and dates fit the default extension; currency could be a separate network extension. **Needs a user decision** |
| **Clipboard history extras**: pin items, filter by type, rename, plain-text paste; images later | [clipboard][CH] | Text only, retention, exclusions, pause | Pinning and type filter come first; images need view and storage work |
| **Aliases, hotkeys and "hide from search" for applications** (indexed results); Windows "Run as administrator" | Per-app aliases, hotkeys, disable; run as admin ([applications][APPS]) | Aliases only for installed commands; app aliases deferred ([aliases](../aliases.md)) | Apps are the most-launched results |
| **Deeplinks** `pane://…/<command>?arguments=…` with a confirmation, plus "Copy Deeplink" | "Raycast will ask you to confirm"; Copy Deeplink on every command ([deeplinks][DL]) | None | Lets Stream Deck, AutoHotkey or scripts drive Pane; fits the trusted-code model |
| **Import/export** of host records (settings, aliases, hotkeys, quick slots, quicklinks, extension list), with selective import and duplicates skipped | `.rayconfig`, encrypted, cross-platform, selective ([import/export][IE]) | None | All of Pane's host records are already versioned JSON files |
| **First-run onboarding** with a few short steps: set the Open Pane hotkey, pin something, press Ctrl+K, install from npm/Git | 7-step quickstart, e.g. "Alt + the first letter" hotkey tip ([quickstart][QS]) | None | Teaches the chords that number hints otherwise hide |
| **Status when the window is hidden**: a small HUD when an action finishes after the launcher closes | "showToast() will fallback to showHUD() if the Raycast window is closed" ([toast][TOAST]) | Status line inside the window only | Needed once paste or hotkey actions close the window |
| **Extension control of pop-to-root** (Default / Immediate / Suspended) | [window & search bar API][WSB] | User setting only | Small API addition once extensions can close the window |
| **System commands** default extension: lock, sleep, restart, shut down, empty Recycle Bin, mute/volume, toggle dark mode | Built-in "System Actions" on Mac and Windows ([system commands][SC]) | None | Native helpers ([helpers](../helpers.md)) fit this exactly |
| **"Run" replacement** (Windows): Control Panel applets, MMC tools, paths, URIs, Run history | "A modern replacement for the classic Windows Run dialog" ([run][RUN]) | None | Strong fit for a Windows-first launcher |
| **Interface size** (Default / Large / Larger) | [settings][ST] | Density is excluded from Appearance by decision ([CONTEXT](../../CONTEXT.md)) | Accessibility rather than cosmetics. **Needs a user decision** |

## 5. Big bets (L)

| Idea | Raycast | Why / caveats |
| --- | --- | --- |
| **Snippets with keyword expansion** in any app (`{cursor}`, `{clipboard}`, `{date}`) | [snippets][SN], [dynamic placeholders][DP]; listed for Windows on [the Windows page][WIN] | Daily-use value is high, but it needs a system-wide keyboard hook and text injection (Wayland problems again). Start with "search snippets, then paste", which depends on the paste capability above |
| **Window management** (halves, quarters, move to display, saved layouts) | [window management][WM] (Mac and Windows) | Extension plus native helper. Windows Snap already covers the basics, so the value is mainly in layouts |
| **Switch Windows** | "jump to any open window across all your apps" ([navigation][NAV]) | Feasible on Windows and X11; a natural indexed-results provider |
| **Hyper Key** (Caps Lock → Ctrl+Alt on Windows) | [hyper key][HK] | A low-level keyboard hook in the host; it makes global hotkeys conflict-free |
| **Script commands**: a folder of scripts whose header comments become commands (silent / compact / full output / inline modes, arguments, fallback use) | [script commands][SCR] (PowerShell on Windows) | Very low-friction authoring. It conflicts with the WASI-only entry point (ADR 0013), but a default extension with a native helper could run scripts from a granted folder. Needs an ADR |
| **Emoji & symbols picker** (grid, pinned, frecency) | [emoji][EM] | Needs a Grid view and paste; good proof of both |
| **Store / catalog** with category browsing and an "Installed" filter | [extensions][EX] | Deferred by Q35; keep npm/Git/folder installs. Section 2's descriptions are a prerequisite either way |

## 6. Skip (for now)

| Raycast feature | Reason |
| --- | --- |
| AI Chat, Quick AI (Tab), Agents, MCP, Dictation, Screen Awareness ([new in v2][V2]) | Accepted decision: AI belongs in extensions (Q3). Make sure the extension API can stream text into a view, and nothing more |
| Themes Studio / shareable themes ([themes][TH]) | Appearance is deliberately limited to System/Light/Dark plus Glass/Solid; revisit once the UI port (#90) settles |
| Floating Notes ([notes][NO]) | Needs a second persistent window type plus a Markdown editor; better as a later extension once custom views mature |
| Calendar, contacts, Focus, Games, Auto Quit, Stage Manager, Spaces, Menu-bar extras, iOS, Cloud Sync, Teams ([manual index][MI]) | macOS-specific, account or service-bound, or outside a small core |
| Whole-home file indexing that respects `.gitignore` ([file search][FS]) | Pane's bounded granted folder is a deliberate scan policy. Revisit only with a real indexer; an "include files in root search" switch is cheap whenever wanted |
| Recently used files and calendar on the empty query ([search bar][SB]) | Contradicts the pinned home's "no recent use" |

## Top picks, in order

1. Allow 1–9 quick slots, each with its own Ctrl+digit (M).
2. A one-line `description` in `pane.json`, status chips and a "…" menu on Extensions cards (S).
3. Paste-into-previous-app as a host capability (M).
4. A master/detail Extensions page with a per-command table and per-command enable (M).
5. Declared, typed preferences with tooltip help and a required-setup form (M–L).
6. Search quicklinks with `{argument}` and dynamic placeholders (M).
7. Section/page keys and visible per-action shortcuts in the Actions panel (S).
8. Frecency and fuzzy ranking with Reset Ranking (M, needs a user decision).
9. A System Commands default extension and a Windows "Run" replacement (M).
10. Deeplinks, import/export and a short first-run onboarding (M each).

[SB]: https://manual.raycast.com/search-bar
[ST]: https://manual.raycast.com/settings
[AP]: https://manual.raycast.com/action-panel
[KS]: https://manual.raycast.com/keyboard-shortcuts
[AH]: https://manual.raycast.com/command-aliases-and-hotkeys
[EX]: https://manual.raycast.com/extensions
[QL]: https://manual.raycast.com/quicklinks
[SN]: https://manual.raycast.com/snippets
[CH]: https://manual.raycast.com/clipboard-history
[FS]: https://manual.raycast.com/file-search
[CA]: https://manual.raycast.com/calculator
[HK]: https://manual.raycast.com/hyper-key
[WM]: https://manual.raycast.com/window-management
[EM]: https://manual.raycast.com/emoji-symbols
[SC]: https://manual.raycast.com/system-commands
[SCR]: https://manual.raycast.com/script-commands
[TH]: https://manual.raycast.com/themes
[NO]: https://manual.raycast.com/notes
[DP]: https://manual.raycast.com/dynamic-placeholders
[IE]: https://manual.raycast.com/import-export
[APPS]: https://manual.raycast.com/applications
[RUN]: https://manual.raycast.com/run
[QS]: https://manual.raycast.com/quickstart
[NAV]: https://manual.raycast.com/navigation
[V2]: https://manual.raycast.com/new-in-v2
[MI]: https://manual.raycast.com/
[MAN]: https://developers.raycast.com/information/manifest
[PREF]: https://developers.raycast.com/api-reference/preferences
[ARGS]: https://developers.raycast.com/information/lifecycle/arguments
[KB]: https://developers.raycast.com/api-reference/keyboard
[DL]: https://developers.raycast.com/information/lifecycle/deeplinks
[TOAST]: https://developers.raycast.com/api-reference/feedback/toast
[WSB]: https://developers.raycast.com/api-reference/window-and-search-bar
[STORE]: https://developers.raycast.com/basics/prepare-an-extension-for-store
[WIN]: https://www.raycast.com/windows
[CLW]: https://www.raycast.com/changelog/windows
