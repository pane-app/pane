# Raycast Favorites (pinned root-search items)

Checked 2026-10-05 against the current Raycast manual (Mac + Windows, v2 era), the macOS v1, macOS v2 and Windows changelogs, and the extension API docs. Raycast's desktop host is closed source. This note covers documented behavior only; anything marked **inferred** comes from no Raycast page that states it, and **third-party** marks a non-Raycast source. The current manual sometimes contradicts itself, and those conflicts are called out below.

## Where favorites appear and how they render

- Favorites appear at the top of Root Search **when the search bar is empty**: "Pin any command, application, or item as a Favorite to keep it at the top of Root Search when the search bar is empty", and they "appear in a dedicated section above all other results". [Search Bar](https://manual.raycast.com/search-bar)
- The original intent, from the v0.31.0 release (2020-10-16): "Raycast suggests your recently and frequently [used] commands and apps but sometimes you want to have a fixed selection when opening Raycast. Now you can favorite any command or app to let it appear at the top of the root search." The Suggestions section had arrived in v0.23.0, so Favorites were added above it. [v0.31.0](https://www.raycast.com/changelog/macos-v1/0-31-0), [v0.23.0](https://www.raycast.com/changelog/macos-v1/0-23-0)
- **While typing:** the manual ties the Favorites section to the empty query only. No Raycast page says whether a favorite's ranking is boosted inside typed results. **Inferred:** while typing, favorites rank like any other item, by the documented order (alias, then fuzzy title, then keywords, then frecency). [Search Bar § How Ranking Works](https://manual.raycast.com/search-bar)
- **Rendering (expanded window):** favorites are ordinary list rows inside a labelled section, so they use the standard row with an icon, title, subtitle and a type accessory such as "Command". **Inferred** from the manual's "dedicated section" wording and from v2 row screenshots. No Raycast page shows favorites rendered in a different shape. [Search Bar](https://manual.raycast.com/search-bar), [New in v2](https://manual.raycast.com/new-in-v2)
- **Rendering (compact window):** shown "as icons in the compact window", and only when the user turns the setting on (see below). [Settings § Window Mode](https://manual.raycast.com/settings)
- Other empty-query content: recently used files (when File Search indexing is on) and today's calendar events, which also "appear at the top when the search bar is empty". The manual never states how these sections are ordered relative to Favorites. [Search Bar](https://manual.raycast.com/search-bar)

## What can be favorited

- The manual says "any command, application, or item". The changelog adds specific cases: dashboard/inline script commands ([v0.31.0](https://www.raycast.com/changelog/macos-v1/0-31-0)), locally developed extension commands ([v1.26.0](https://www.raycast.com/changelog/macos-v1/1-26-0)), AI Commands ([v1.49.0](https://www.raycast.com/changelog/macos-v1/1-49-0)), Quicklinks in compact mode ([v2.1](https://www.raycast.com/changelog/macos/2-1)) and games on Windows ([v0.62](https://www.raycast.com/changelog/windows/0-62)).
- Favorites are part of exported preferences/data, alongside aliases and hotkeys. [v1.22.0](https://www.raycast.com/changelog/macos-v1/1-22-0)
- No source says whether files can be favorited. **Unknown.**

## Limit

- **No documented maximum.** None of the manual pages or changelog entries mentions a cap or overflow behavior. The expanded window shows favorites as a vertical list section that scrolls with the rest. **Inferred:** there is no cap, and a long list pushes the other sections down. Compact mode shows favorites as icons, and no source covers what happens with many of them (wrapping, truncation, scrolling). **Unknown.**

## Add / remove / reorder

- **Where:** the Action Panel (`⌘K`/`Ctrl K`) on a Root Search item, under a "Favorites" section that holds add/remove and move up/down. Typing "favorites" in the panel filters to these actions. [Action Panel](https://manual.raycast.com/action-panel), [v0.31.0](https://www.raycast.com/changelog/macos-v1/0-31-0)
- **Action names:** "Add to Favorites", "Remove from Favorites", "Move Favorite Up" and "Move Favorite Down". [Search Bar](https://manual.raycast.com/search-bar)
- **Shortcuts.** The manual contradicts itself here:

| Action | Action Panel § Favorites | Action Panel § Keyboard Shortcuts | Keyboard Shortcuts page |
|---|---|---|---|
| Add to Favorites | `⌘F` / `Ctrl F` | `⇧⌘F` / `Ctrl Shift F` | `⌘F` / `Ctrl F` |
| Move up/down | `⌘↑↓` / `Ctrl ↑↓` | — | `⇧⌘↑↓` / `Ctrl Shift ↑↓` |

  Sources: [Action Panel](https://manual.raycast.com/action-panel), [Keyboard Shortcuts](https://manual.raycast.com/keyboard-shortcuts). The same pages give `⌘↑/⌘↓` as "jump to previous/next section", which makes `⌘↑` for reordering doubtful. A third-party shortcut list for v1.61.2 gives `⌘⇧F` "add or remove … favorites" and `⌘⌥↑/↓` "move a favorite item up/down" ([defkey, third-party](https://defkey.com/raycast-1-61-2-shortcuts)). Those match the API's `Keyboard.Shortcut.Common.MoveUp/MoveDown` = `⌘⌥↑/↓` / `Ctrl Alt ↑/↓` ([Keyboard API](https://developers.raycast.com/api-reference/keyboard)). v2 (beta 0.57/0.58, 2026-05) "Ensured move favorite / move pinned actions use the same shortcuts" ([macOS 0.57](https://www.raycast.com/changelog/macos/0-57), [Windows 0.58](https://www.raycast.com/changelog/windows/0-58)). **Best guess (medium confidence):** add/remove is a single toggle on `⇧⌘F` (v1), and moves follow the common Move Up/Down chord.
- **Drag and drop:** not documented for favorites. Raycast added drag-and-drop reordering only for Fallback Commands, in Settings ([v1.84.0](https://www.raycast.com/changelog/macos-v1/1-84-0)).

## Keyboard access and navigation

- **Number chords:** Root Search got "`⌘ 1...9` shortcut to quickly jump to N-th item in the list" in v1.8.0 (2021-02) ([v1.8.0](https://www.raycast.com/changelog/macos-v1/1-8-0)). On Windows, "All list-based components now support number hotkeys to trigger the nth item (eg. CTRL + 3 will trigger the 3rd item). This works in all list views as well as all menus" ([Windows 0.33](https://www.raycast.com/changelog/windows/0-33)). Number hotkeys are "now supported in compact mode too" ([macOS 0.59](https://www.raycast.com/changelog/macos/0-59)).
- The numbering is positional over the whole list. It is not a favorites-only feature. **Inferred:** because Favorites are the first section, they take the lowest numbers, and the rows after them take the rest up to 9.
- The manual's Keyboard Shortcuts page documents number chords only for AI Chat's sidebar: "`⌘1`…`⌘9`, `⌘0` … Hold `⌘`/`Ctrl` to see the numbers." That is the only documented hold-modifier reveal of the numbers. [Keyboard Shortcuts](https://manual.raycast.com/keyboard-shortcuts)
- **Selection:** a single list. `↑/↓` moves across section boundaries, `⌘↑/⌘↓` (`Ctrl ↑/↓`) jumps to the previous or next section, and `⌥↑/⌥↓` pages. [Keyboard Shortcuts](https://manual.raycast.com/keyboard-shortcuts), [Search Bar](https://manual.raycast.com/search-bar)

## Compact mode

- Compact mode arrived in v1.38.0 (2022-07, "Compact Mode for additional focus") ([v1.38.0](https://www.raycast.com/changelog/macos-v1/1-38-0)). v2 has "a new, simplified compact mode design" ([macOS 0.51](https://www.raycast.com/changelog/macos/0-51)). In compact mode the window "collapses to show only the search bar when the search term is empty" and expands on typing or when the Action Panel opens. [Search Bar § Compact Mode](https://manual.raycast.com/search-bar)
- **Setting:** Settings → General → Appearance → Window Mode → "**Show Favorites in Compact Mode** to show your favorites as icons in the compact window". [Settings](https://manual.raycast.com/settings)
- History: "Root Search: Added ability to show favorites in compact mode" (v0.69, 2026-07-16, shipped on both macOS and Windows) ([macOS 0.69](https://www.raycast.com/changelog/macos/0-69), [Windows 0.69](https://www.raycast.com/changelog/windows/0-69)). This was followed by "Improved icon rendering of Quicklink favourites in compact mode" ([v2.1](https://www.raycast.com/changelog/macos/2-1)). Number chords already worked in compact mode from 0.59.
- The manual does not say whether the setting defaults to on or off. **Inferred: off**, since the feature is opt-in ("Turn on…").

## Ranking, suggestions, hiding

- No setting hides Favorites when some are set; removing them all is the only documented way to clear the section. No Raycast page mentions a "show favorites" toggle for the expanded window. **Inferred.**
- The v2 Windows build excludes commands that have hotkeys from suggestions ([Windows 0.47](https://www.raycast.com/changelog/windows/0-47)). No Raycast page says whether favorited items are also excluded from Suggestions or appear twice. **Unknown.**
- Other root-search settings: Search Sensitivity, Window Mode and Pop to Root Search. [Search Bar § Settings](https://manual.raycast.com/search-bar)

## Timeline

| Version | Date | Change |
|---|---|---|
| v0.23.0 | 2020-08 | Suggestions section added at the top of Root Search |
| v0.31.0 | 2020-10-16 | **Favorites introduced** (commands, apps; dashboards) |
| v1.8.0 | 2021-02-18 | `⌘1…9` jumps to the Nth Root Search item |
| v1.26.0 | 2021-11 | Local development commands can be favorited |
| v1.38.0 | 2022-07-19 | Compact Mode introduced |
| Win 0.33 / 0.34 | 2025-10 | Windows: `Ctrl+N` triggers the Nth item in every list/menu; Favorite action in Root Search |
| v2 0.57–0.58 | 2026-05 | Move favorite and move pinned use the same shortcuts |
| v2 0.59 | 2026-05-13 | Number hotkeys work in compact mode |
| v2 0.69 | 2026-07-16 | **Show favorites in compact mode** (icons) |
| v2.1 | 2026-09-01 | Better Quicklink favorite icons in compact mode |

## Windows

- Favorites reached Windows in 0.34 (2025-10-29): "Root Search: commands now have a Favorite action that let you place your favorite commands at the top" ([Windows 0.34](https://www.raycast.com/changelog/windows/0-34)). The macOS and Windows changelogs are now shared for v2.
- The manual pages are written for both platforms and pair the keys (`⌘` → `Ctrl`, `⌥` → `Alt`, `⇧` → `Shift`). Number chords on Windows are `Ctrl+1…9`. [Keyboard Shortcuts](https://manual.raycast.com/keyboard-shortcuts), [Windows 0.33](https://www.raycast.com/changelog/windows/0-33)
- No documented difference in favorites behavior between platforms.

## Implications for Pane

Pane when this was researched, before [ADR 0027](../adr/0027-quick-slots-are-an-ordered-list.md) replaced it with an ordered list of pins without a limit: five positional **quick slots** with holes, numbered Ctrl+1…5 (empty slots unnumbered), followed by rows up to Ctrl+9. The pinned home is a horizontal strip (default) or a vertical list, it is hidden while a query is typed, and compact mode shows only the search field ([CONTEXT.md](../../CONTEXT.md), [root search § pinned home](../root-search.md#the-pinned-home), `crates/pane-core/src/launcher/quick_slots.rs`).

- **Slots versus an ordered list:** Raycast uses an ordered list with no holes, no fixed positions and no "replace slot" step. Adding appends and Move Up/Down reorders. Holes only make sense if a slot's chord must stay stable when an earlier pin is removed. Pane already numbers holes away, so the holes buy little. An ordered list would remove "Replace a Quick Slot" and the dashed empty tiles.
- **Cap:** Raycast documents none. Raycast's numbering runs 1–9 over the whole list, so items past the ninth simply get no chord. Pane could do the same: no cap, or a soft cap of 9 so every pin has a chord. A cap of 5 is stricter than Raycast.
- **Horizontal overflow:** Raycast's only horizontal form is the compact-mode icon row, and its overflow behavior is undocumented. The expanded form is a vertical section that scrolls with the list. If Pane keeps a horizontal strip, it has to choose between wrap and scroll itself. Raycast suggests the vertical list as the form that scales.
- **Compact mode:** Raycast added an opt-in "Show Favorites in Compact Mode" that renders them as icons, and number chords work in compact mode. That maps directly onto Pane's horizontal strip as the compact-mode presentation.
- **Keys:** Raycast's add/remove and move actions live in the Action Panel with chords. The likely chords are toggle `⇧⌘F` and moves on the common Move Up/Down chord (`Ctrl+Alt+↑/↓` on Windows). Pane's "Move Slot Left/Right" fits a strip. With a vertical list, Up/Down wording and `Ctrl+Alt+↑/↓` (or `Ctrl+Shift+↑/↓`) would match Raycast's API conventions. Avoid plain `Ctrl+↑/↓`, which Raycast uses for section jumps.
- **Number hints:** Pane's hold-Ctrl reveal matches Raycast's documented AI Chat pattern ("Hold ⌘/Ctrl to see the numbers"). Raycast's Root Search numbering is positional over the whole list, which matches Pane's "pinned first, then rows".
