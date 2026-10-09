# Raycast deep dive: what an extension can do, how Raycast is built, and what Pane lacks

Researched 2026-10-06. Unlike the earlier documentation-only notes
([raycast.md](raycast.md), [raycast-extension-ui.md](raycast-extension-ui.md),
[raycast-root-search.md](raycast-root-search.md),
[raycast-inspiration.md](raycast-inspiration.md)), this note also reads the
**installed Raycast for Windows 2.6.1.0** and the **open-source extension
corpus**. It is research, not decisions: items that reverse something Pane
chose on purpose are listed under [Decisions needed](#decisions-needed).

Sources, in order of weight:

- The installed app, read only, from copies: the WebView2 frontend's
  JavaScript and CSS, the Node backend bundle, the `@raycast/api` runtime,
  strings from the Rust `native.dll`, `Raycast.UIAccess.exe` and
  `Raycast.SystemService.exe`, and a decompile of the .NET host
  `Raycast.dll`. Identifiers cited below are minified names from this build
  and will change between builds; quoted strings are stable search handles.
  Nothing was launched, modified or decrypted.
- The developer docs (developers.raycast.com, as Markdown in
  `raycast/extensions/docs`), the `@raycast/api` 2.6.3 typings and the
  `@raycast/utils` 2.3.2 source.
- A census of all 3,379 extensions in `raycast/extensions` (961 list
  Windows), by regex over `package.json` and sources (roughly ±5 points),
  plus 14 extensions read in depth.
- The user manual (manual.raycast.com, read in full from its
  `llms-full.txt`), product pages and the Windows changelog to v2.6.
- Prior art for plugin UI across a sandbox boundary, and Pane's own code.

Raycast's code, icons and themes are its own: this note records
measurements, names and behaviour, and Pane should take ideas, not assets.

## How Raycast for Windows is built

Three tiers, plus native helpers:

| Tier | What it is | Owns |
|---|---|---|
| Host | `Raycast.exe`, .NET WPF (`Raycast.dll`) with an in-process Rust `native.dll` | windows, tray, app discovery, system commands, request handlers (`Raycast.Handlers.*`) |
| Backend | `backend\node.exe backend\index.mjs`, stdio with the host (CRLF-delimited JSON messages) | root search engine, settings, store, file index (Rust `indexer` module), extension workers |
| Frontend | React in WebView2 (`frontend\*.js`) | all of Raycast's own UI, built-in commands' screens, rendering extension trees |
| `Raycast.UIAccess.exe` | Rust helper with `uiAccess="true"`, named pipe to the host | hotkeys and keyboard hook, snippets, simulated input, foreground, UI Automation |
| `Raycast.SystemService.exe` | optional Windows service | elevated helper launch, NTFS MFT scan (shipped switched off in 2.6.1) |

Each extension command runs in a Node `worker_threads` worker (100 MB JS
heap, at most 4 live sessions per command, one warm spare). A React
reconciler turns the extension's tree into `ray-*` host elements and sends
the **whole tree** as JSON on each commit (gzip+base64 above 30 KB), with
callbacks as string ids (`${nodeId}_${name}`); only the selected item's
detail and actions are serialised. Built-in commands are declared in the
frontend (`internal-extensions-*.js`) with the same command and action
model as Node commands; only their screens differ.

All databases are encrypted (key in Credential Manager and a DPAPI file,
each repairing the other); schemas were read from the native module's SQL
text: `node_extensions.commands` holds `mode`, `arguments`, `preferences`,
`interval`, `custom_subtitle`; one `settings_v2.commands` table holds
`enabled, alias, windows_hotkey, favorite_order, fallback_order` for
built-in and extension commands alike.

## What an extension can do

### Command kinds and lifecycle

- `mode` is **required** per command: `view`, `no-view` or `menu-bar`
  (menu-bar does not exist on Windows). The runtime dispatches on it before
  running any code (`A5()` in the API bundle): a no-view default export is
  called as an async function; a view's is rendered; mixing is rejected.
  The host needs the mode at Enter to decide whether to open a screen.
- After launch the extension decides everything: `closeMainWindow({
  clearRootSearch, popToRootType: default | immediate | suspended })`,
  `popToRoot`, `clearSearchBar`, `showHUD` (closes the window first),
  `showToast` (becomes a HUD when the window is hidden), `confirmAlert`
  (destructive style, "don't ask again"), `launchCommand` (with JSON
  context, user-initiated or background, cross-extension behind a prompt),
  `updateCommandMetadata({subtitle})`. Built-in actions are compositions of
  these: `Action.CopyToClipboard` is copy → close → HUD "Copied to
  Clipboard".
- One `LaunchProps` record for every way in (root search, alias,
  `launchCommand`, background interval, form draft, fallback, deeplink):
  `launchType`, `arguments` (≤ 3: text, password, dropdown), `draftValues`,
  `launchContext`, `fallbackText`.
- `interval` (no-view and menu-bar only) for background runs; off after a
  store install until the user first opens the command.
- AI entry points: `tools` with typed input and an optional confirmation,
  plus bundled skills, MCP servers and model providers.

### UI building blocks

Authors write ordinary React, but the leaves are fixed: List, Grid,
Detail, Form, ActionPanel. No HTML, canvas or drawing of any kind. Within
that set composition is free, and any view can be pushed on any other.

- **List**: sections; host fuzzy filtering on title and keywords unless the
  extension takes `onSearchTextChange` (then optional `throttle`, 250 ms in
  the frontend); controlled `searchText`; a `searchBarAccessory` dropdown;
  `pagination`; `isLoading`; `selectedItemId` and `onSelectionChange`;
  `EmptyView` with its own actions; `isShowingDetail`.
- **List.Item**: icon (built-in set of 478, asset, URL, file icon; tint,
  mask, light/dark), title and subtitle with tooltips, `keywords`,
  `accessories` (text, relative date, coloured tag, each with icon and
  tooltip), `detail` (CommonMark plus metadata: label, link, tag list,
  separator), `actions`.
- **Grid**: List with image content, 1–8 columns, aspect ratio, fit, inset.
- **Detail**: Markdown (sized and tinted images, LaTeX) plus metadata.
- **Form**: text field, password, text area, checkbox, date picker,
  dropdown (sections, search), tag picker, file picker, description,
  separator; per-field `info`, `error`, `storeValue`; drafts.
- **ActionPanel**: sections, submenus (lazy, searchable), shortcuts with
  per-platform maps, destructive style. On Windows: Enter primary,
  Ctrl+Enter secondary, Ctrl+Shift+Enter tertiary, Ctrl+K panel.
- **Navigation**: `push`/`pop`, Escape pops; the store rejects extensions
  that swap content where a push is expected.

### Host services and escape hatches

Clipboard (copy with `concealed`, paste into the front app, read), encrypted
LocalStorage, synchronous LRU Cache, `supportPath`, selected text, front
app, `open`, `showInFinder` ("File Explorer"), `trash`, OAuth PKCE (with
Raycast-hosted client ids and proxies for GitHub, Linear, Slack, Asana),
gated AI. Extensions are plain Node, unsandboxed: files, network, child
processes. `@raycast/utils` adds `usePromise` (cancellation, optimistic
mutate with rollback, pagination, failure toast with Retry), cached
stale-while-revalidate variants, `useExec`, `useSQL`,
`runPowerShellScript`, `useFrecencySorting`, favicons, deeplinks. Rust
functions marked `#[raycast]` compile to a bundled Windows executable with
generated TypeScript wrappers (`import … from "rust:../rust"`).

### What real extensions use

Share of all extensions whose source uses the feature (Windows-listing
extensions in brackets where notably different):

| Feature | Share |
|---|---|
| 2+ actions in one ActionPanel | 78% (89%) |
| `isLoading` | 77% |
| Toast | 76% |
| List | 75% |
| Clipboard or copy/paste action | 66% (76%) |
| Item icon | 64% |
| Declared preferences | 61% |
| Shortcuts | 56% (69%) |
| Accessories | 54% |
| Network | 51% |
| Storage or cache | 49% |
| Markdown detail | 48% |
| EmptyView | 46% |
| Push navigation | 45% |
| Sections | 44% |
| closeMainWindow / showHUD / popToRoot | 42% |
| Form | 39% |
| Owns the search bar | 37% |
| Search bar dropdown | 30% |
| confirmAlert | 24% |
| child process | 22% (PowerShell 10% on Windows) |
| launchCommand | 11% |
| Grid | 9% |

Of 10,652 commands, 67% are `view`, 30% `no-view`, 3% `menu-bar`; 10% of
extensions are no-view only. Where authors hit limits they escape to the
OS (processes, PowerShell, FFI DLLs, CLIs downloaded and hash-checked at
run time, Rust modules), relay long work to no-view commands because view
workers "die on Escape and ~90 s after the window deactivates" (winget),
and work around action panels rebuilt on every selection change (lazy
submenus).

## How Raycast behaves

### Root search

- Fuzzy matcher: a matched letter scores 4 at the very start, 3 at a word
  start, 2 elsewhere; each gap costs 1; separators are space, tab, newline
  and `- . / ( ) [ ]`. **Search sensitivity is a threshold** on that score:
  Low any match, Medium ≥ 1.5·(n−2)+4, High (default) > 2n.
- Ranking: exact alias; a few boosted words; exact title (query > 3
  characters); **queries the user previously used for that item** (last 3,
  17 days); exact subtitle; alias prefix; best fuzzy score; frecency
  (10-day half-life); row kind (commands > quicklinks > apps > settings and
  files); title.
- A use is recorded 100 ms after running, not when opened by its own
  hotkey, and not for throwaway rows (calculator answers, files).
- Each provider gets 200 ms, late answers arrive as diffs batched every
  16 ms, and **Enter waits up to 300 ms for the current query's results** so
  it never runs a stale top row.
- `rootSearch: {section, when: empty | searching | always, matching:
  always | url | file-path}` decides where a contributor appears;
  `matching: url` rows only for URL-like queries, `file-path` only for
  queries starting with `/`, `~`, `\\`, `X:\` or `file://`.
- The calculator hides when a result's title or alias starts with the
  query; favourites get no boost while typing; transliteration ("cafe"
  finds "Café"); an app's exe name is an alternate title unless generic;
  same-name apps are disambiguated.
- When nothing matches, the first fallback is auto-selected.

### Keyboard and arguments

- Up to 3 inline argument fields after the query; Tab or Right at the end
  enters them; Enter with a required field empty focuses it; alias + space
  fills the first; dropdown choices remembered; Up recalls the last 64
  queries with their arguments.
- Exact modifier matching; Backspace on an empty query goes back one level
  (not on auto-repeat); Ctrl+C with text selected copies text; Alt+J/K
  style navigation on Windows (no clash with Ctrl+K); Alt+Up/Down moves 5
  rows; Ctrl+Up/Down jumps sections; Shift+Esc to root; Ctrl+[ back.
- Corrections to [raycast-inspiration.md](raycast-inspiration.md): Add to
  Favorites is Ctrl+Shift+F, Move Favorite Ctrl+Alt+Up/Down, Disable
  Command Ctrl+Alt+Shift+D, and there is no Ctrl+W.

### Window, feedback, store

- 750 × 475 (compact 750 × 66), centred, top at 27% of the leftover height;
  hides on focus loss; pop to root after 0–180 s (default 90) from focus
  loss; position remembered per monitor; single instance.
- Toasts are not queued (newest replaces); 3 s in the footer; HUD 1.2 s
  (3 s for failures). An extension crash gets a screen with Reload, Copy
  Error and Report.
- Required preferences: one gate before the first launch with only the
  required, unset fields and the extension's `HELP.md` beside them.
- Store: no permission screen, a one-time terms gate; atomic installs with
  rollback; extensions update silently every hour with no off switch;
  extensions without `platforms` count as macOS-only.
- Self-update: the direct build checks MD5 and force-closes to install; the
  Store build installs silently and relaunches without a window.
- Crash reporting: Sentry in all three tiers, 10% sampling, user and
  machine names redacted, 10 events per minute per site; no user switch.

### Look and motion

Values from the frontend CSS and host, for reference (Pane's own design is
accepted in [ADR 0029](../adr/0029-the-ui-is-accepted-as-built.md)):

- Inter Variable with `ss03`, base 13 px, scale 8/11/13/16/18/24 with small
  per-size tracking; JetBrains Mono.
- Colour is the text colour at fixed alphas: text 100/60/40/20%, separators
  10%, selection 10% with no border, hover 5%. Accent follows the Windows
  system accent (fallback `#4FA3F8`). A 7-colour semantic palette for
  tags, with contrast-corrected tag text (minimum 2.5).
- Search bar 66 tall, query 18 px at weight 350, a 1 px rule and a 1 px
  loading sweep that appears only after 300 ms. Rows 38 tall, radius 6,
  22 px icons, subtitle inline at 60%. Footer 42. Action panel 350 wide,
  ≤ 280 tall, search at the bottom, 32 px rows. Flat outlined 18 px
  keycaps. App icons bare; only built-in commands get gradient tiles.
- Almost nothing animates: navigation and selection are instant. The
  action panel opens in 250 ms (`cubic-bezier(.3,.7,0,1.26)`, scale .97→1)
  and closes in 150 ms; hover fades out in 70 ms; number hints after a
  400 ms hold. The launcher ignores reduced motion.
- Themes are 12 colours (background gradient, text, selection, loader, 7
  semantic colours), shared as a URL.

### Windows integration

- **Hotkeys**: `RegisterHotKey` when Windows accepts the binding, else a
  `WH_KEYBOARD_LL` hook: the Windows key alone (the default on a fresh
  install, with "Replace Start Menu"), double taps, side-specific
  modifiers, push-to-talk, refused shortcuts. A refused shortcut is never
  an error. The hook is guarded by a raw-input liveness watchdog that
  reinstalls it when Windows silently drops it, code pages locked in
  memory, a high-priority thread, stuck-modifier recovery, and tagged
  injected input (reusing PowerToys' or AutoHotkey's tag when running).
- **Game mode**: every 2 s, a game in front (catalog, `GameConfigStore`, or
  full-screen D3D) moves hotkeys to the hook and passes keys through.
- **UI access**: the helper's `uiAccess` lets it hook, inject into and
  foreground elevated apps; without it those features fail silently. An
  optional SYSTEM service launches it elevated.
- **Foreground**: `SetForegroundWindow`, then `AttachThreadInput`, on a
  worker with a timeout, skipping hung windows.
- **Paste**: track the previous foreground window (excluding shell surfaces
  such as `Shell_TrayWnd`, `TaskSwitcherWnd`, `SearchHost.exe`), hide,
  foreground the target and wait until it really is in front, write the
  clipboard tagged not-for-history, `SendInput` Ctrl+V, restore the user's
  clipboard later unless it changed.
- **Selected text**: UI Automation in a separate worker process with a
  2.5 s timeout (disabled 10 min after repeated crashes; Chromium needs its
  accessibility tree woken first), then simulated Ctrl+C with clipboard
  save and restore.
- **Snippets**: keyword matching in the hook's buffer; immediate or
  delimiter modes; cancellable injection delays; `{cursor}`.
- **App discovery**: Start menu and Desktop shortcuts (`.lnk`, `.url`,
  `.appref-ms`, 5 levels), MSIX via `PackageManager`, winget installs, 11
  game launchers, Control Panel, 191 `ms-settings:` pages plus 459
  individual settings filtered by Windows' per-device cache. Identity is a
  hash of the resolved target and arguments with version folders
  wildcarded, so Discord and Slack keep their history across updates.
  Localised names from the shell; icons at 256 px rejecting tiny padded
  ones, one binary cache refreshed in the background. Kept current by
  folder watchers (0.5 s debounce) and package events, with a 5 s grace
  before removing an app.
- **File index**: the MIT-licensed Rust crate `minidex` in a worker; the
  whole home folder, `.gitignore` honoured, caches and `AppData` excluded,
  background priority, churn quarantine, low-disk floor; catch-up from the
  NTFS change journal on start and `ReadDirectoryChangesW` while running,
  without admin rights. Measured here: 450,097 entries in 12.9 s, 61.8 MB.
  Content search defaults to the Windows Search index.
- **Switch Windows**: listing runs in the UIAccess helper. A window counts
  if it is visible and unowned, not `WS_EX_NOACTIVATE`, not a tool window
  (unless `WS_EX_APPWINDOW`), not marked `ITaskList_Deleted`, not a
  `Windows.UI.Core.*` inner window, not the shell window; app-cloaked
  windows are dropped, shell-cloaked ones kept only when the user's Alt+Tab
  shows all desktops (`VirtualDesktopAltTabFilter`) and the documented
  `IVirtualDesktopManager` says they are elsewhere. Window to app: the
  window's AUMID, then exe path disambiguated by AUMID (which separates
  PWAs from their browser), then AUMID alone (UWP under
  `ApplicationFrameHost.exe`). Move and resize correct for invisible
  borders and restore before resizing. Virtual desktops use the
  undocumented `IVirtualDesktopManagerInternal` (through the `winvd` crate,
  13 build-specific tables) and are disabled below Windows 11 23H2; snap
  groups fake the Snap keyboard shortcut.
- **System commands**: 42 handlers. Lock, sleep (displays off on Modern
  Standby), hibernate, HDR, mic mute, Recycle Bin and the rest use
  documented APIs. Undocumented: Night Light (a private registry blob, with
  a UI Automation fallback), Airplane Mode (`IRadioManager`), Energy Saver
  (WNF), default audio device (`IPolicyConfig`, stable since Windows 7).
- **Run**: reads and writes Win+R's `RunMRU` in Explorer's format so both
  histories stay shared; `.cpl` through `control.exe`; bare names resolved
  on the registry PATH, not the process's; everything else through
  `ShellExecuteEx` (`runas` to elevate); "run in shell" prefers a Windows
  Terminal tab.

### Footprint

Measured passively for 592 s with the launcher hidden: 10 processes, about
713 MB committed, 87–99 MB private resident, 6,174 handles, 295 threads,
about 0.4% of one core. Node is 248 MB and WebView2 296 MB of the commit.
Techniques: EcoQoS at idle with short opt-outs (startup, 15 s after a
hotkey, 3 min after wake, pending requests); wake recovery with timeouts
counted in awake time; a handle-count monitor (60 s, 5 s above 5,000);
background runs capped at 8 with failure back-off to 24 h.

## Pane against this

Pane is already ahead on: custom drawn views, per-command and per-item
platform availability, continuing services and short schedules,
operations between packages with dependencies, explicit HTTP limits,
sha512 update integrity, AccessKit, and a single native process without
Node or WebView2.

### Extension contract, ranked by corpus use

| # | Gap | Raycast | Pane today | Size |
|---|---|---|---|---|
| 1 | Several actions per item, action panel | sections, submenus, shortcuts, Enter / Ctrl+Enter | one `run-action` per item | M–L |
| 2 | Host functions for feedback and window control | toast (updatable, actions), HUD, close, pop to root, confirm | result text in the status line | S–M |
| 3 | Row icons and accessories | 478 icons, assets, URLs, file icons; text, date, tag | title and subtitle | M |
| 4 | Copy, paste, open as host actions | built-ins with defined window effects | `copy`, `open-url`, `open-file` for root results only | S (paste M) |
| 5 | Declared typed preferences with a setup gate | 61% of extensions | settings stored by the command's own UI | M |
| 6 | Navigation stack | push/pop any view, Escape pops | one form or custom view, one level | L |
| 7 | Detail pane and Markdown detail | CommonMark plus metadata | none | M–L |
| 8 | No-view mode | 30% of commands | every command opens a list | S |
| 9 | Search bar control | host vs extension filtering, dropdown, pagination, selection | `search: true` with fixed 150 ms wait | M |
| 10 | Typed arguments and launch props | ≤ 3 typed, fallback text, context, launch type | one free-text query | S–M |
| 11 | Richer forms | ~10 field types, validation, drafts | text and single choice | M–L |
| 12 | launchCommand and live subtitles | context, background, cross-extension | operations, static subtitle | S–M |
| 13 | Live re-render (timers, polling) | push re-render | pull only | M (see bridge) |
| 14 | OAuth | PKCE, hosted proxies | none; credentials in plain text | L |
| 15 | Grid | 9% | none | M |

### Extension UI bridge

The user's direction, consistent with [ADR 0003](../adr/0003-gpui-ce-and-extensible-views.md):
a declarative tree rendered by GPUI, going beyond Raycast's fixed set.

- **Layers**: standard views (List, Detail, Form, Grid, action panel); a
  component tree of layout primitives plus Pane's shared components (rich
  row, icon tile, keycap, tag, button, input, select, toggle, Markdown)
  styled by semantic tokens, never raw pixels or hex; a canvas as a leaf
  node inside the tree (paths, clip, images, `measure-text`, hover, wheel).
- **Prior art agrees** (Shopify remote-dom and checkout extensions, Figma
  Widgets, VS Code tree views, Adaptive Cards, Slack and Discord
  components, Flutter's rfw): the host renders a fixed component set the
  guest arranges; semantic styling; the host owns anything that must react
  within a frame (inputs are partially controlled, hover and pressed
  styles are host-applied); authors supply stable keys; explicit component
  versions with fallbacks; escape hatches kept small. Zed has no extension
  UI yet; its community proposals converge on the same shape.
- **Feasibility in Pane**: the UI layer is already presentation-only
  (`crates/pane/src/ui`), theme tokens are central (`ui/theme.rs`), and
  GPUI CE has flex/grid layout, styled text, images from bytes and SVG,
  virtualised lists, an IME-capable `editable_text`, canvas paths,
  AccessKit and focus handles. Missing: image icons, a standalone tag,
  Markdown, extension actions in the action panel.
- **Obstacles**: push re-render needs a runtime that serves instances
  concurrently (today one call at a time across all extensions, and a
  guest runs only inside a host call); exact export type checks make a
  growing typed tree force rebuilds; host state per node key (inputs,
  selects, focus, scroll, accessibility ids).
- **Suggested path**: (1) a view whose `render` returns a tree of column,
  row, text and button with tokens and callback events (S–M); (2)
  `refresh-after-ms` on render answers for timers and polling (S); (3)
  shared components and images (M); (4) keyed host reconciler, inputs,
  selects, focus stops (M); (5) a JSX runtime for JS/TS and a Rust builder
  (S); (6) standard views on the tree (M); (7) canvas on GPUI's `canvas()`
  (M); (8) a concurrent runtime for real push (L).

### Shell and platform

| Gap | Raycast | Pane today | Size |
|---|---|---|---|
| Hook fallback for hotkeys (Win key, double tap, side modifiers) | `RegisterHotKey` then hook, never an error | `RegisterHotKey`, refused shortcut is an error | M |
| App identity by resolved target, version folders wildcarded | yes | shortcut path | S |
| App icons | 256 px, cache, light/dark for MSIX | none | M |
| Localised shortcut names | `SHGetLocalizedName` | file name | S |
| Watch for app changes | folder watchers, package events, 5 s grace | 10 s cache and rescan | S–M |
| Exe names as alternate titles, same-name labels | yes | no | S |
| Settings pages and Control Panel as results | 191 + 459 | none | M |
| Indexed file search | minidex, change journal, watchers | one granted folder, rescanned, 5,000 files | M–L |
| Switch Windows provider and window actions | documented APIs; elevated windows need UI access | none | M |
| Documented system commands (lock, sleep, HDR, volume, mic, Recycle Bin…) | small host calls | none | S–M |
| Run, sharing Win+R's history | `RunMRU`, registry PATH, `ShellExecuteEx` | none | S–M |
| Virtual desktops, snap groups, Night Light, Airplane Mode | undocumented, version-gated | none | defer |
| Paste into previous app, selected text | see above | none | M–L |
| Tray icon survives Explorer restart, theme-aware | yes | vanishes until restart | S |
| Credentials encrypted at rest | DPAPI | plain-text JSON | S |
| Narrator announcement of the selected row | PowerToys Command Palette pattern | blocked on active-descendant | S |
| Extension logs for authors | console to dev terminal | WASI context discards output | M |
| Standalone authoring CLI and published SDKs | `ray` CLI, npm package | needs a Pane checkout | L |
| Utilities library | `@raycast/utils` | none | M |
| Power: EcoQoS at idle, wake recovery, handle monitor | yes | none | S–M |
| Measured footprint | measured here | not measured | S |

Search-bar extras that need no policy change: URL and bare-domain
detection, file paths with Tab to browse, colours in many formats,
`now`/`time`, percentages (`20% off 80`).

## Decisions needed

Each reverses or extends something Pane chose, so it needs the user:

1. **Tree wire format** for the UI bridge: versioned JSON (no WIT change
   per component, graceful unknown nodes) or a typed flat WIT arena with
   one frozen WIT per API version, as Zed does. Measure both on a 500-node
   tree.
2. **Raycast's look**: ADR 0029 accepted Pane's redesign. Options: keep it;
   adopt the colour system and timings only (alpha-based text and
   selection, semantic palette, no tile behind app icons, 300 ms loading
   threshold, toast and HUD durations); or move to Raycast's proportions.
3. **Learned queries and suggestions** in root search; Pane avoids
   recent-use lists.
4. **Auto-selecting the first fallback** when nothing matches; Pane does
   the opposite.
5. **Silent automatic extension updates**: already decided (Q21) and
   shipped (#49, #50), with a global "Update extensions automatically"
   setting and per-package controls; default extensions get no updates
   yet (#53).
6. **Extensions running system tools** (PowerShell, winget, git, elevated
   commands) beyond packaged helpers; 22% of Raycast extensions do.
7. **Script commands**, which conflict with ADR 0013, perhaps as a default
   extension built on helpers.
8. **Indexed file search** beyond one granted folder (ADR 0017).
9. **Per-guest memory cap**: Pane's `GUEST_MEMORY` is 512 MiB, five times
   Raycast's 100 MB worker heap.
10. **Importers** from PowerToys Run or Flow Launcher, which Raycast lacks.

## Open questions that need an interactive session

Not measurable passively: hotkey-to-visible latency (warm and cold), the
cost of opening extension commands and memory returned after closing
them, growth over 50 show/hide cycles, paste and snippets into elevated or
hung apps with and without the elevated helper, Game Mode in practice, how
the Win-key-alone and double-tap hotkeys feel against the Start menu, and
how often foregrounding fails.
